#!/usr/bin/env node
// Slack Plugin Bridge
//
// Translates between the lukan plugin protocol (JSON lines on stdin/stdout)
// and the Slack API via Socket Mode (WebSocket — no public URL needed).
//
// Flow:
//   lukan host <-- stdin/stdout --> bridge.js <-- WebSocket --> Slack Socket Mode API
//
// Requires:
//   - Bot Token (xoxb-...) with chat:write, app_mentions:read, channels:history, im:history scopes
//   - App-Level Token (xapp-...) with connections:write scope for Socket Mode
//

import { WebSocket } from "ws";
import { createInterface } from "readline";

// ── State ──────────────────────────────────────────────────────────────

let config = {};
let botToken = "";
let appToken = "";
let botUserId = "";
let shuttingDown = false;
let ws = null;
let reconnectTimer = null;

// Filtering
let allowedChannels = [];
let allowedUsers = [];
let prefix = null;

// Track pending requests
let requestCounter = 0;
const pendingRequests = new Map(); // requestId → { channel, threadTs }

// ── Plugin protocol helpers ────────────────────────────────────────────

function send(msg) {
  process.stdout.write(JSON.stringify(msg) + "\n");
}

function log(level, message) {
  send({ type: "log", level, message });
}

function sendError(message, recoverable = true) {
  send({ type: "error", message, recoverable });
}

function sendStatus(status) {
  send({ type: "status", status });
}

// ── Slack Web API ──────────────────────────────────────────────────────

async function slackApi(method, body = {}) {
  const res = await fetch(`https://slack.com/api/${method}`, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${botToken}`,
      "Content-Type": "application/json; charset=utf-8",
    },
    body: JSON.stringify(body),
  });

  const data = await res.json();
  if (!data.ok) {
    throw new Error(`Slack API ${method}: ${data.error}`);
  }
  return data;
}

/** Send a message to a Slack channel/thread, splitting if needed */
async function sendSlackMessage(channel, text, threadTs = null) {
  const MAX_LEN = 4000; // Slack limit is ~4000 for best rendering
  const chunks = [];

  let remaining = text;
  while (remaining.length > 0) {
    if (remaining.length <= MAX_LEN) {
      chunks.push(remaining);
      break;
    }
    let splitAt = remaining.lastIndexOf("\n", MAX_LEN);
    if (splitAt <= 0) splitAt = MAX_LEN;
    chunks.push(remaining.slice(0, splitAt));
    remaining = remaining.slice(splitAt).trimStart();
  }

  for (const chunk of chunks) {
    const payload = { channel, text: chunk };
    if (threadTs) payload.thread_ts = threadTs;
    await slackApi("chat.postMessage", payload);
  }
}

// ── Socket Mode (WebSocket) ────────────────────────────────────────────

async function getSocketUrl() {
  const res = await fetch("https://slack.com/api/apps.connections.open", {
    method: "POST",
    headers: {
      Authorization: `Bearer ${appToken}`,
      "Content-Type": "application/x-www-form-urlencoded",
    },
  });
  const data = await res.json();
  if (!data.ok) {
    throw new Error(`Socket Mode connect failed: ${data.error}`);
  }
  return data.url;
}

async function connectSocket() {
  if (shuttingDown) return;

  try {
    const url = await getSocketUrl();
    log("info", "Connecting to Slack Socket Mode...");

    ws = new WebSocket(url);

    ws.on("open", () => {
      log("info", "Connected to Slack Socket Mode");
      sendStatus("connected");
    });

    ws.on("message", (raw) => {
      try {
        const data = JSON.parse(raw.toString());
        handleSocketMessage(data);
      } catch (err) {
        log("warn", `Failed to parse Socket Mode message: ${err.message}`);
      }
    });

    ws.on("close", () => {
      log("warn", "Socket Mode disconnected");
      sendStatus("disconnected");
      scheduleReconnect();
    });

    ws.on("error", (err) => {
      log("error", `Socket Mode error: ${err.message}`);
      scheduleReconnect();
    });
  } catch (err) {
    log("error", `Failed to connect: ${err.message}`);
    scheduleReconnect();
  }
}

function scheduleReconnect() {
  if (shuttingDown || reconnectTimer) return;
  log("info", "Reconnecting in 5s...");
  sendStatus("reconnecting");
  reconnectTimer = setTimeout(() => {
    reconnectTimer = null;
    connectSocket();
  }, 5000);
}

// ── Handle Socket Mode messages ────────────────────────────────────────

function handleSocketMessage(data) {
  // Acknowledge envelope immediately (Socket Mode requirement)
  if (data.envelope_id) {
    ws.send(JSON.stringify({ envelope_id: data.envelope_id }));
  }

  if (data.type === "hello") {
    log("info", "Socket Mode handshake complete");
    return;
  }

  if (data.type === "disconnect") {
    log("info", `Socket Mode disconnect requested: ${data.reason}`);
    return; // Will auto-reconnect on ws close
  }

  if (data.type !== "events_api") return;

  const event = data.payload?.event;
  if (!event) return;

  // Handle app_mention and message events
  if (event.type === "app_mention" || event.type === "message") {
    handleSlackMessage(event);
  }
}

function handleSlackMessage(event) {
  // Skip bot's own messages and message_changed subtypes
  if (event.bot_id || event.subtype) return;

  const channel = event.channel;
  const userId = event.user || "";
  // Always reply in thread — keeps conversations organized in both channels and DMs
  const threadTs = event.thread_ts || event.ts;
  let content = event.text || "";

  if (!content) return;

  // Filter by channel and user
  if (!shouldProcess(userId, channel)) return;

  // Strip bot mention from content
  content = content.replace(new RegExp(`<@${botUserId}>`, "g"), "").trim();

  // Strip prefix
  content = stripPrefix(content);
  if (content === null) return;

  if (!content) return;

  const requestId = `sk-${++requestCounter}`;
  pendingRequests.set(requestId, { channel, threadTs });

  log(
    "info",
    `Message from ${userId} in ${channel}: ${content.slice(0, 80)}`,
  );

  // Send to host — messages queue in the Rust channel loop and process sequentially
  send({
    type: "channelMessage",
    requestId,
    sender: userId,
    channelId: channel,
    content,
  });
}

// ── Filtering helpers ──────────────────────────────────────────────────

function shouldProcess(userId, channel) {
  if (allowedChannels.length > 0 && !allowedChannels.includes(channel)) {
    return false;
  }
  if (allowedUsers.length > 0 && !allowedUsers.includes(userId)) {
    return false;
  }
  return true;
}

function stripPrefix(content) {
  if (!prefix) return content;
  const trimmed = content.trim();
  if (trimmed.startsWith(prefix)) {
    return trimmed.slice(prefix.length).trim();
  }
  return null;
}

// ── Handle messages from the lukan host (stdin) ────────────────────────

function handleHostMessage(msg) {
  switch (msg.type) {
    case "init":
      log("info", `Received Init for plugin "${msg.name}"`);

      config = msg.config || {};
      botToken = config.botToken || "";
      appToken = config.appToken || "";
      allowedChannels = config.allowedChannels || [];
      allowedUsers = config.allowedUsers || [];
      prefix = config.prefix || null;

      if (!botToken) {
        sendError(
          "No bot token configured. Create a Slack app and add a Bot User OAuth Token (xoxb-...).",
          false,
        );
        return;
      }

      if (!appToken) {
        sendError(
          "No app-level token configured. Generate an App-Level Token (xapp-...) with connections:write scope.",
          false,
        );
        return;
      }

      log(
        "info",
        `Config: channels=[${allowedChannels.join(",")}], users=[${allowedUsers.join(",")}], prefix=${prefix || "(none)"}`,
      );

      // Verify token and get bot identity
      slackApi("auth.test")
        .then((res) => {
          botUserId = res.user_id;
          log("info", `Bot authenticated as <@${botUserId}> in workspace ${res.team}`);
          send({ type: "ready", version: "0.1.0", capabilities: [] });
          connectSocket();
        })
        .catch((err) => {
          sendError(`Failed to authenticate: ${err.message}`, false);
        });
      break;

    case "agentResponse": {
      const { requestId, text, isError } = msg;
      const target = pendingRequests.get(requestId);

      if (!target) {
        log("warn", `No pending request for ${requestId}`);
        break;
      }

      pendingRequests.delete(requestId);

      const responseText = isError ? `Error: ${text}` : text;

      sendSlackMessage(target.channel, responseText, target.threadTs).catch(
        (err) =>
          log(
            "error",
            `Failed to send response to ${target.channel}: ${err.message}`,
          ),
      );
      log("info", `Sent response to ${target.channel} (${text.length} chars)`);
      break;
    }

    case "shutdown":
      log("info", "Received Shutdown");
      shutdown();
      break;

    default:
      log("warn", `Unknown host message type: ${msg.type}`);
      break;
  }
}

// ── Shutdown ────────────────────────────────────────────────────────────

function shutdown() {
  shuttingDown = true;
  if (reconnectTimer) clearTimeout(reconnectTimer);
  if (ws) ws.close();
  process.exit(0);
}

// ── Stdin reader (JSON lines) ──────────────────────────────────────────

const rl = createInterface({ input: process.stdin });

rl.on("line", (line) => {
  const trimmed = line.trim();
  if (!trimmed) return;

  try {
    const msg = JSON.parse(trimmed);
    handleHostMessage(msg);
  } catch (err) {
    process.stderr.write(
      `[bridge] Failed to parse host message: ${err.message}\n`,
    );
  }
});

rl.on("close", () => {
  process.stderr.write("[bridge] stdin closed, exiting\n");
  shutdown();
});

process.on("SIGTERM", shutdown);
process.on("SIGINT", shutdown);
