#!/usr/bin/env node
// Telegram Plugin Bridge
//
// Translates between the lukan plugin protocol (JSON lines on stdin/stdout)
// and the Telegram Bot API (long polling).
//
// Flow:
//   lukan host <-- stdin/stdout --> bridge.js <-- HTTP long poll --> Telegram Bot API
//

import { createInterface } from "readline";

// ── State ──────────────────────────────────────────────────────────────

let config = {};
let botToken = "";
let shuttingDown = false;
let pollingActive = false;
let lastUpdateId = 0;

// Filtering
let whitelist = []; // Telegram user IDs (strings)
let allowedGroups = []; // Telegram chat IDs (strings)
let prefix = null;

// Track pending requests
let requestCounter = 0;
const pendingRequests = new Map(); // requestId → chatId

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

// ── Telegram Bot API ───────────────────────────────────────────────────

const API_BASE = "https://api.telegram.org";

async function apiCall(method, body = {}) {
  const url = `${API_BASE}/bot${botToken}/${method}`;
  const res = await fetch(url, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });

  const data = await res.json();
  if (!data.ok) {
    throw new Error(`Telegram API error: ${data.description || res.status}`);
  }
  return data.result;
}

/** Send a text message, splitting if > 4096 chars (Telegram limit) */
async function sendMessage(chatId, text) {
  const MAX_LEN = 4096;
  const chunks = [];

  let remaining = text;
  while (remaining.length > 0) {
    if (remaining.length <= MAX_LEN) {
      chunks.push(remaining);
      break;
    }
    // Split at last newline before limit
    let splitAt = remaining.lastIndexOf("\n", MAX_LEN);
    if (splitAt <= 0) splitAt = MAX_LEN;
    chunks.push(remaining.slice(0, splitAt));
    remaining = remaining.slice(splitAt).trimStart();
  }

  for (const chunk of chunks) {
    await apiCall("sendMessage", {
      chat_id: chatId,
      text: chunk,
      parse_mode: "Markdown",
    }).catch(async () => {
      // Retry without parse_mode if Markdown fails
      await apiCall("sendMessage", { chat_id: chatId, text: chunk });
    });
  }
}

/** Send "typing..." indicator */
async function sendTyping(chatId) {
  await apiCall("sendChatAction", {
    chat_id: chatId,
    action: "typing",
  }).catch(() => {});
}

// ── Long Polling ───────────────────────────────────────────────────────

async function startPolling() {
  pollingActive = true;
  sendStatus("connected");
  log("info", "Started long polling for updates");

  while (pollingActive && !shuttingDown) {
    try {
      const updates = await apiCall("getUpdates", {
        offset: lastUpdateId + 1,
        timeout: 30,
        allowed_updates: ["message"],
      });

      for (const update of updates) {
        lastUpdateId = update.update_id;
        if (update.message) {
          handleMessage(update.message);
        }
      }
    } catch (err) {
      if (shuttingDown) break;
      log("error", `Polling error: ${err.message}`);
      sendStatus("reconnecting");
      // Wait before retrying
      await new Promise((r) => setTimeout(r, 5000));
      sendStatus("connected");
    }
  }
}

function stopPolling() {
  pollingActive = false;
}

// ── Handle incoming Telegram messages ──────────────────────────────────

function handleMessage(msg) {
  const chatId = String(msg.chat.id);
  const userId = String(msg.from?.id || "");
  const isGroup = msg.chat.type === "group" || msg.chat.type === "supergroup";
  const senderName = msg.from
    ? [msg.from.first_name, msg.from.last_name].filter(Boolean).join(" ")
    : "Unknown";

  // Get text content
  let content = msg.text || msg.caption || "";
  if (!content) return; // Skip non-text messages

  // Filter
  if (!shouldProcess(userId, chatId, isGroup)) return;

  // Strip prefix
  content = stripPrefix(content);
  if (content === null) return; // prefix required but not present

  // In groups, also respond to @botname mentions
  if (isGroup && prefix === null) {
    // Without a prefix in groups, require @mention or reply to bot
    const botMentioned =
      msg.entities?.some(
        (e) => e.type === "mention" || e.type === "bot_command",
      ) || msg.reply_to_message?.from?.is_bot;
    if (!botMentioned) return;

    // Strip @botname from content
    content = content.replace(/@\w+/g, "").trim();
  }

  if (!content) return;

  const requestId = `tg-${++requestCounter}`;
  pendingRequests.set(requestId, chatId);

  const sender = `${senderName} (${userId})`;
  log("info", `Message from ${sender} in ${chatId}: ${content.slice(0, 80)}`);

  // Send typing indicator
  sendTyping(chatId);

  // Send to host — messages queue in the Rust channel loop and process sequentially
  send({
    type: "channelMessage",
    requestId,
    sender,
    channelId: chatId,
    content,
  });
}

// ── Filtering helpers ──────────────────────────────────────────────────

function shouldProcess(userId, chatId, isGroup) {
  // If no filters configured, allow all
  if (whitelist.length === 0 && allowedGroups.length === 0) {
    return true;
  }

  if (isGroup) {
    return allowedGroups.includes(chatId);
  }
  return whitelist.includes(userId);
}

function stripPrefix(content) {
  if (!prefix) return content;
  const trimmed = content.trim();
  // Support both "prefix message" and "/prefix message" for bot commands
  if (trimmed.startsWith(prefix)) {
    return trimmed.slice(prefix.length).trim();
  }
  return null; // prefix required but not present
}

// ── Handle messages from the lukan host (stdin) ────────────────────────

function handleHostMessage(msg) {
  switch (msg.type) {
    case "init":
      log("info", `Received Init for plugin "${msg.name}"`);

      config = msg.config || {};
      botToken = config.botToken || "";
      whitelist = (config.whitelist || []).map(String);
      allowedGroups = (config.allowedGroups || []).map(String);
      prefix = config.prefix || null;

      if (!botToken) {
        sendError("No bot token configured. Get one from @BotFather on Telegram.", false);
        return;
      }

      log(
        "info",
        `Config: whitelist=[${whitelist.join(",")}], groups=[${allowedGroups.join(",")}], prefix=${prefix || "(none)"}`,
      );

      // Verify token and get bot info
      apiCall("getMe")
        .then((me) => {
          log("info", `Bot authenticated as @${me.username} (${me.first_name})`);
          send({ type: "ready", version: "0.1.0", capabilities: [] });
          startPolling();
        })
        .catch((err) => {
          sendError(`Failed to authenticate bot: ${err.message}`, false);
        });
      break;

    case "agentResponse": {
      const { requestId, text, isError } = msg;
      const chatId = pendingRequests.get(requestId);

      if (!chatId) {
        log("warn", `No pending request for ${requestId}`);
        break;
      }

      pendingRequests.delete(requestId);

      if (isError) {
        log("error", `Agent error for ${requestId}: ${text}`);
        sendMessage(chatId, `Error: ${text}`).catch((err) =>
          log("error", `Failed to send error: ${err.message}`),
        );
        break;
      }

      sendMessage(chatId, text).catch((err) =>
        log("error", `Failed to send response to ${chatId}: ${err.message}`),
      );
      log("info", `Sent response to ${chatId} (${text.length} chars)`);
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
  stopPolling();
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
