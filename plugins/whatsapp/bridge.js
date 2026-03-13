#!/usr/bin/env node
// WhatsApp Plugin Bridge
//
// Translates between the lukan plugin protocol (JSON lines on stdin/stdout)
// and the whatsapp-connector WebSocket protocol.
//
// Flow:
//   lukan host ←─ stdin/stdout ─→ bridge.js ←─ WebSocket ─→ whatsapp-connector
//
// The bridge auto-starts the connector if it can't connect.
//

import { WebSocket } from "ws";
import { createInterface } from "readline";
import { spawn } from "child_process";
import { existsSync } from "fs";
import { resolve, dirname } from "path";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

// ── State ──────────────────────────────────────────────────────────────

let config = {};
let ws = null;
let reconnectTimer = null;
let shuttingDown = false;
let connectorChild = null; // child process if we started the connector

// Whitelist / group filtering
let whitelist = [];
let allowedGroups = [];
let prefix = null;
let bridgeUrl = "ws://localhost:3001";

// Audio transcription
let openaiApiKey = process.env.OPENAI_API_KEY || null;
let transcriptionBackend = "local"; // "openai" | "local"
let whisperUrl = "http://localhost:8787";
let whisperChild = null; // child process if we started whisper server
let whisperReady = false;

// Track pending requests: requestId is generated per incoming message
let requestCounter = 0;
// Map: requestId → chatId (so we know where to send the agent response)
const pendingRequests = new Map();

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

// ── Connector auto-start ────────────────────────────────────────────────

/** Find the whatsapp-connector index.js in known locations */
function findConnectorPath() {
  const candidates = [
    // Sibling directory inside the plugin (self-contained)
    resolve(__dirname, "whatsapp-connector/index.js"),
    // Relative to bridge.js (dev/repo layout)
    resolve(__dirname, "../../whatsapp-connector/index.js"),
    // CWD fallback
    resolve(process.cwd(), "whatsapp-connector/index.js"),
  ];

  for (const p of candidates) {
    if (existsSync(p)) {
      return p;
    }
  }
  return null;
}

/** Try to start the connector as a child process */
function startConnector() {
  const connectorPath = findConnectorPath();
  if (!connectorPath) {
    log("warn", "Could not find whatsapp-connector/index.js — connector must be started manually");
    return false;
  }

  log("info", `Starting connector from ${connectorPath}`);
  const connectorDir = dirname(connectorPath);

  connectorChild = spawn("node", [connectorPath], {
    cwd: connectorDir,
    stdio: ["ignore", "pipe", "pipe"],
    detached: false,
  });

  connectorChild.stdout.on("data", (data) => {
    // Forward connector stdout as debug logs
    for (const line of data.toString().split("\n").filter(Boolean)) {
      log("debug", `[connector] ${line}`);
    }
  });

  connectorChild.stderr.on("data", (data) => {
    for (const line of data.toString().split("\n").filter(Boolean)) {
      log("warn", `[connector] ${line}`);
    }
  });

  connectorChild.on("exit", (code) => {
    log("warn", `Connector process exited with code ${code}`);
    connectorChild = null;
    if (!shuttingDown) {
      sendStatus("disconnected");
    }
  });

  return true;
}

/** Kill the connector child process if we started it */
function killConnector() {
  if (connectorChild) {
    log("info", "Killing connector child process");
    try {
      connectorChild.kill("SIGTERM");
    } catch {
      // Ignore — process may already be dead
    }
    connectorChild = null;
  }
}

// ── Whisper server auto-start ────────────────────────────────────────

/** Find lukan-whisper binary in known locations */
function findWhisperBinary() {
  const candidates = [
    resolve(process.env.HOME || "", ".config/lukan/plugins/whisper/lukan-whisper"),
    resolve(process.env.HOME || "", ".local/bin/lukan-whisper"),
  ];
  for (const p of candidates) {
    if (existsSync(p)) return p;
  }
  return null;
}

/** Start whisper server and wait until it responds on /health */
async function startWhisperServer() {
  // Already running externally?
  try {
    const res = await fetch(`${whisperUrl}/health`);
    if (res.ok) {
      log("info", "Whisper server already running");
      whisperReady = true;
      return true;
    }
  } catch {
    // Not running — start it
  }

  const bin = findWhisperBinary();
  if (!bin) {
    log("error", "lukan-whisper binary not found — install the whisper plugin");
    return false;
  }

  // Extract port from whisperUrl
  let port = "8787";
  try {
    port = new URL(whisperUrl).port || "8787";
  } catch {}

  log("info", `Starting whisper server: ${bin} serve ${port}`);
  whisperChild = spawn(bin, ["serve", port], {
    stdio: ["ignore", "pipe", "pipe"],
    detached: false,
  });

  whisperChild.stdout.on("data", (data) => {
    for (const line of data.toString().split("\n").filter(Boolean)) {
      log("debug", `[whisper] ${line}`);
    }
  });

  whisperChild.stderr.on("data", (data) => {
    for (const line of data.toString().split("\n").filter(Boolean)) {
      log("debug", `[whisper] ${line}`);
    }
  });

  whisperChild.on("exit", (code) => {
    log("warn", `Whisper server exited with code ${code}`);
    whisperChild = null;
    whisperReady = false;
  });

  // Poll /health until ready (max 30s for model loading)
  for (let i = 0; i < 60; i++) {
    await new Promise((r) => setTimeout(r, 500));
    try {
      const res = await fetch(`${whisperUrl}/health`);
      if (res.ok) {
        log("info", "Whisper server is ready");
        whisperReady = true;
        return true;
      }
    } catch {
      // Not ready yet
    }
  }

  log("error", "Whisper server failed to start within 30s");
  return false;
}

/** Kill the whisper server child process if we started it */
function killWhisper() {
  if (whisperChild) {
    log("info", "Killing whisper server");
    try {
      whisperChild.kill("SIGTERM");
    } catch {
      // Ignore
    }
    whisperChild = null;
    whisperReady = false;
  }
}

// ── WebSocket connection to whatsapp-connector ─────────────────────────

let connectAttempts = 0;
const MAX_CONNECT_ATTEMPTS_BEFORE_AUTO_START = 2;

function connectWs() {
  if (shuttingDown) return;

  log("info", `Connecting to connector at ${bridgeUrl}...`);

  ws = new WebSocket(bridgeUrl);

  ws.on("open", () => {
    log("info", "Connected to whatsapp-connector");
    connectAttempts = 0; // reset on successful connect
  });

  ws.on("message", (raw) => {
    try {
      const event = JSON.parse(raw.toString());
      handleConnectorEvent(event);
    } catch (err) {
      log("warn", `Failed to parse connector message: ${err.message}`);
    }
  });

  ws.on("close", () => {
    log("warn", "Disconnected from connector");
    sendStatus("disconnected");
    scheduleReconnect();
  });

  ws.on("error", (err) => {
    log("error", `WebSocket error: ${err.message}`);
    connectAttempts++;

    // Auto-start connector after a couple of failed attempts
    if (connectAttempts === MAX_CONNECT_ATTEMPTS_BEFORE_AUTO_START && !connectorChild) {
      log("info", "Connector not reachable — attempting auto-start");
      startConnector();
    }

    scheduleReconnect();
  });
}

function scheduleReconnect() {
  if (shuttingDown || reconnectTimer) return;
  log("info", "Reconnecting in 3s...");
  sendStatus("reconnecting");
  reconnectTimer = setTimeout(() => {
    reconnectTimer = null;
    connectWs();
  }, 3000);
}

// ── Handle events from whatsapp-connector ──────────────────────────────

function handleConnectorEvent(event) {
  switch (event.type) {
    case "status":
      // Map connector status to plugin status
      if (event.status === "connected") {
        sendStatus("connected");
      } else if (event.status === "disconnected") {
        sendStatus("disconnected");
      } else if (event.status === "qr") {
        sendStatus("authenticating");
        log("info", "QR code displayed on connector — scan with WhatsApp");
      }
      break;

    case "message": {
      const { sender, chatId, content, isGroup } = event;

      // Filter: whitelist / allowed groups
      if (!shouldProcess(sender, chatId, isGroup)) {
        return;
      }

      // Strip prefix if configured
      const message = stripPrefix(content);

      // Generate requestId and track it
      const requestId = `wa-${++requestCounter}`;
      pendingRequests.set(requestId, chatId);

      log("info", `Message from ${sender} in ${chatId}: ${message.slice(0, 80)}`);

      // Send to host — messages queue in the Rust channel loop and process sequentially
      send({
        type: "channelMessage",
        requestId,
        sender,
        channelId: chatId,
        content: message,
      });
      break;
    }

    case "audio": {
      const { sender, chatId, audioBase64, seconds, ptt, isGroup } = event;
      if (!shouldProcess(sender, chatId, isGroup)) return;

      // Check if transcription is available based on backend
      if (transcriptionBackend === "local" && !whisperReady) {
        log("warn", `Audio from ${sender} ignored — whisper server not ready yet`);
        if (ws && ws.readyState === WebSocket.OPEN) {
          ws.send(
            JSON.stringify({
              type: "send",
              to: chatId,
              text: "Audio transcription is starting up, try again in a moment.",
            }),
          );
        }
        break;
      }
      if (transcriptionBackend === "openai" && !openaiApiKey) {
        log("warn", `Audio from ${sender} ignored — no OPENAI_API_KEY configured`);
        if (ws && ws.readyState === WebSocket.OPEN) {
          ws.send(
            JSON.stringify({
              type: "send",
              to: chatId,
              text: "Audio transcription is not configured. Set OPENAI_API_KEY to enable it.",
            }),
          );
        }
        break;
      }

      if (!audioBase64) {
        log("warn", `Audio from ${sender} has no audioBase64 data`);
        break;
      }

      log("info", `Audio from ${sender} (${seconds || "?"}s, ptt=${!!ptt}) — transcribing...`);

      // Transcribe and forward as channelMessage
      transcribeAudio(audioBase64, event.mimetype || "audio/ogg; codecs=opus")
        .then((transcript) => {
          if (!transcript || !transcript.trim()) {
            log("warn", `Empty transcription for audio from ${sender}`);
            return;
          }

          log("info", `Transcription: ${transcript.slice(0, 80)}${transcript.length > 80 ? "..." : ""}`);

          // Strip prefix if configured
          const message = stripPrefix(transcript);

          const requestId = `wa-${++requestCounter}`;
          pendingRequests.set(requestId, chatId);

          // Send transcribed text to host as a regular channelMessage
          send({
            type: "channelMessage",
            requestId,
            sender,
            channelId: chatId,
            content: `[Audio ${seconds || "?"}s] ${message}`,
          });
        })
        .catch((err) => {
          log("error", `Transcription failed: ${err.message}`);
          if (ws && ws.readyState === WebSocket.OPEN) {
            ws.send(
              JSON.stringify({
                type: "send",
                to: chatId,
                text: `Failed to transcribe audio: ${err.message}`,
              }),
            );
          }
        });

      break;
    }

    case "groups":
      log("info", `Received ${event.groups?.length || 0} groups from connector`);
      break;

    default:
      break;
  }
}

// ── Filtering helpers ──────────────────────────────────────────────────

function shouldProcess(sender, chatId, isGroup) {
  // Default-deny: require at least one whitelist or group
  if (whitelist.length === 0 && allowedGroups.length === 0) {
    return false;
  }

  if (isGroup) {
    return allowedGroups.includes(chatId);
  }
  return whitelist.includes(sender);
}

function stripPrefix(content) {
  if (!prefix) return content;
  const trimmed = content.trim();
  if (trimmed.startsWith(prefix)) {
    return trimmed.slice(prefix.length).trim();
  }
  return content;
}

// ── Audio transcription ──────────────────────────────────────────────

async function transcribeAudio(audioBase64, mimetype) {
  if (transcriptionBackend === "local") {
    return transcribeLocal(audioBase64, mimetype);
  }
  return transcribeOpenAI(audioBase64, mimetype);
}

/** Transcribe via local Whisper server (whisper.cpp HTTP API) */
async function transcribeLocal(audioBase64, mimetype) {
  const buffer = Buffer.from(audioBase64, "base64");

  const ext = mimetype.includes("ogg")
    ? "ogg"
    : mimetype.includes("mp4")
      ? "m4a"
      : mimetype.includes("mpeg")
        ? "mp3"
        : "ogg";

  const boundary = `----FormBoundary${Date.now()}`;
  const filename = `audio.${ext}`;

  // Whisper server only needs the file field (no model field)
  const fileHeader = Buffer.from(
    `--${boundary}\r\nContent-Disposition: form-data; name="file"; filename="${filename}"\r\nContent-Type: ${mimetype}\r\n\r\n`,
  );
  const footer = Buffer.from(`\r\n--${boundary}--\r\n`);
  const body = Buffer.concat([fileHeader, buffer, footer]);

  const url = `${whisperUrl}/v1/audio/transcriptions`;
  const response = await fetch(url, {
    method: "POST",
    headers: {
      "Content-Type": `multipart/form-data; boundary=${boundary}`,
    },
    body,
  });

  if (!response.ok) {
    const text = await response.text();
    throw new Error(`Whisper server error ${response.status}: ${text}`);
  }

  const result = await response.json();
  return result.text || "";
}

/** Transcribe via OpenAI API */
async function transcribeOpenAI(audioBase64, mimetype) {
  const buffer = Buffer.from(audioBase64, "base64");

  const ext = mimetype.includes("ogg")
    ? "ogg"
    : mimetype.includes("mp4")
      ? "m4a"
      : mimetype.includes("mpeg")
        ? "mp3"
        : "ogg";

  const boundary = `----FormBoundary${Date.now()}`;
  const filename = `audio.${ext}`;

  const parts = [];
  parts.push(
    `--${boundary}\r\nContent-Disposition: form-data; name="model"\r\n\r\ngpt-4o-transcribe\r\n`,
  );
  parts.push(
    `--${boundary}\r\nContent-Disposition: form-data; name="file"; filename="${filename}"\r\nContent-Type: ${mimetype}\r\n\r\n`,
  );

  const header = Buffer.from(parts.join(""));
  const footer = Buffer.from(`\r\n--${boundary}--\r\n`);
  const body = Buffer.concat([header, buffer, footer]);

  const response = await fetch("https://api.openai.com/v1/audio/transcriptions", {
    method: "POST",
    headers: {
      Authorization: `Bearer ${openaiApiKey}`,
      "Content-Type": `multipart/form-data; boundary=${boundary}`,
    },
    body,
  });

  if (!response.ok) {
    const text = await response.text();
    throw new Error(`OpenAI API error ${response.status}: ${text}`);
  }

  const result = await response.json();
  return result.text || "";
}

// ── Handle messages from the lukan host (stdin) ────────────────────────

function handleHostMessage(msg) {
  switch (msg.type) {
    case "init":
      log("info", `Received Init for plugin "${msg.name}"`);

      // Merge config from Init
      config = msg.config || {};
      bridgeUrl = config.bridgeUrl || bridgeUrl;
      whitelist = config.whitelist || [];
      allowedGroups = config.allowedGroups || [];
      prefix = config.prefix || null;
      // API key: config > env
      openaiApiKey = config.openaiApiKey || process.env.OPENAI_API_KEY || null;
      // Transcription backend: "openai" (default) or "local" (whisper.cpp)
      transcriptionBackend = config.transcriptionBackend || "openai";
      whisperUrl = config.whisperUrl || "http://localhost:8787";

      log(
        "info",
        `Config: bridge=${bridgeUrl}, whitelist=[${whitelist.join(",")}], groups=[${allowedGroups.join(",")}], prefix=${prefix || "(none)"}`,
      );

      if (transcriptionBackend === "local") {
        log("info", `Audio transcription: local whisper (${whisperUrl})`);
        // Auto-start whisper server in background
        startWhisperServer().catch((err) => {
          log("error", `Failed to start whisper server: ${err.message}`);
        });
      } else {
        log("info", `Audio transcription: ${openaiApiKey ? "openai (enabled)" : "openai (disabled — no OPENAI_API_KEY)"}`);
      }

      // Send Ready
      send({ type: "ready", version: "0.1.0", capabilities: [] });

      // Connect to whatsapp-connector (auto-starts if needed)
      connectWs();
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
        break;
      }

      // Send response back to WhatsApp via connector
      if (ws && ws.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify({ type: "send", to: chatId, text }));
        log("info", `Sent response to ${chatId} (${text.length} chars)`);
      } else {
        log("error", `Cannot send response — not connected to connector`);
      }
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
  killConnector();
  killWhisper();
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
    process.stderr.write(`[bridge] Failed to parse host message: ${err.message}\n`);
  }
});

rl.on("close", () => {
  process.stderr.write("[bridge] stdin closed, exiting\n");
  shutdown();
});

// Keep process alive
process.on("SIGTERM", shutdown);
process.on("SIGINT", shutdown);
