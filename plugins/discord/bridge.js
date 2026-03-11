#!/usr/bin/env node
// Discord Plugin Bridge
//
// Translates between the lukan plugin protocol (JSON lines on stdin/stdout)
// and the Discord API via Gateway WebSocket (discord.js).
//
// Flow:
//   lukan host <-- stdin/stdout --> bridge.js <-- WebSocket --> Discord Gateway
//
// Requires:
//   - Bot Token from Discord Developer Portal
//   - Message Content Intent enabled (privileged intent)
//   - Bot invited with scopes: bot
//   - Permissions: Send Messages, Read Message History, Create Public Threads

import { Client, GatewayIntentBits, Partials } from "discord.js";
import { createInterface } from "readline";

// ── State ──────────────────────────────────────────────────────────────

let config = {};
let botToken = "";
let shuttingDown = false;
let client = null;
let botUserId = "";

// Filtering
let allowedChannels = [];
let allowedUsers = [];
let prefix = null;
let replyInThread = true;

// Track pending requests
let requestCounter = 0;
const pendingRequests = new Map(); // requestId → { channel, message }

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

// ── Discord message sending ────────────────────────────────────────────

async function sendDiscordMessage(channel, message, text) {
  const MAX_LEN = 2000; // Discord limit
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

  let target = channel;

  // Create/use thread if configured and not already in a thread
  if (replyInThread && !channel.isThread()) {
    try {
      target = await message.startThread({ name: "Lukan" });
    } catch {
      // Fall back to replying in channel if thread creation fails (e.g. DMs)
      target = channel;
    }
  }

  for (const chunk of chunks) {
    await target.send(chunk);
  }
}

// ── Discord Gateway via discord.js ─────────────────────────────────────

function createClient() {
  client = new Client({
    intents: [
      GatewayIntentBits.Guilds,
      GatewayIntentBits.GuildMessages,
      GatewayIntentBits.MessageContent,
      GatewayIntentBits.DirectMessages,
    ],
    partials: [Partials.Channel], // Required to receive DMs without cache
  });

  client.on("ready", () => {
    botUserId = client.user.id;
    log("info", `Bot online as ${client.user.tag} (${botUserId})`);
    sendStatus("connected");
  });

  client.on("messageCreate", handleDiscordMessage);

  client.on("error", (err) => {
    log("error", `Discord client error: ${err.message}`);
  });

  // discord.js handles reconnection automatically
  client.on("shardDisconnect", () => {
    log("warn", "Discord gateway disconnected");
    sendStatus("disconnected");
  });

  client.on("shardReconnecting", () => {
    log("info", "Reconnecting to Discord gateway...");
    sendStatus("reconnecting");
  });

  client.on("shardResume", () => {
    log("info", "Reconnected to Discord gateway");
    sendStatus("connected");
  });
}

// ── Handle incoming Discord messages ───────────────────────────────────

function handleDiscordMessage(message) {
  // Skip all bot messages (including own)
  if (message.author.bot) return;

  const channelId = message.channel.id;
  const userId = message.author.id;
  let content = message.content || "";

  if (!content) return;

  // Filter by channel and user
  if (!shouldProcess(userId, channelId)) return;

  // Strip bot mention (handles both <@id> and <@!id> formats)
  content = content.replace(new RegExp(`<@!?${botUserId}>`, "g"), "").trim();

  // Strip prefix
  content = stripPrefix(content);
  if (content === null) return;

  if (!content) return;

  const requestId = `dc-${++requestCounter}`;
  pendingRequests.set(requestId, { channel: message.channel, message });

  const sender = `${message.author.username} (${userId})`;
  log(
    "info",
    `Message from ${sender} in ${channelId}: ${content.slice(0, 80)}`,
  );

  // Send typing indicator
  message.channel.sendTyping().catch(() => {});

  // Send to host
  send({
    type: "channelMessage",
    requestId,
    sender,
    channelId,
    content,
  });
}

// ── Filtering helpers ──────────────────────────────────────────────────

function shouldProcess(userId, channelId) {
  if (allowedChannels.length > 0 && !allowedChannels.includes(channelId)) {
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
      allowedChannels = config.allowedChannels || [];
      allowedUsers = config.allowedUsers || [];
      prefix = config.prefix || null;
      replyInThread = config.replyInThread !== false; // default true

      if (!botToken) {
        sendError(
          "No bot token configured. Create a bot at https://discord.com/developers/applications and copy the token.",
          false,
        );
        return;
      }

      log(
        "info",
        `Config: channels=[${allowedChannels.join(",")}], users=[${allowedUsers.join(",")}], prefix=${prefix || "(none)"}, threads=${replyInThread}`,
      );

      createClient();

      client
        .login(botToken)
        .then(() => {
          send({ type: "ready", version: "0.1.0", capabilities: [] });
        })
        .catch((err) => {
          sendError(`Failed to login to Discord: ${err.message}`, false);
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

      const responseText = isError ? `**Error:** ${text}` : text;

      sendDiscordMessage(target.channel, target.message, responseText).catch(
        (err) =>
          log(
            "error",
            `Failed to send response to ${target.channel.id}: ${err.message}`,
          ),
      );
      log("info", `Sent response to ${target.channel.id} (${text.length} chars)`);
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
  if (client) client.destroy();
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
