#!/usr/bin/env node
// WhatsApp Connector — connects WhatsApp Web via baileys to lukan via WebSocket

import {
  default as baileys,
  useMultiFileAuthState,
  DisconnectReason,
  makeCacheableSignalKeyStore,
  fetchLatestWaWebVersion,
  Browsers,
  downloadMediaMessage,
} from "@whiskeysockets/baileys";
const makeWASocket = baileys.default || baileys.makeWASocket || baileys;
import { WebSocketServer } from "ws";
import qrcode from "qrcode-terminal";
import pino from "pino";
import { join } from "path";
import { homedir } from "os";
import { mkdirSync } from "fs";

const logger = pino({ level: "silent" });

const XDG_DATA_HOME = process.env.XDG_DATA_HOME || join(homedir(), ".local", "share");
const AUTH_DIR = join(XDG_DATA_HOME, "lukan", "whatsapp-auth");
const WS_PORT = parseInt(process.env.CONNECTOR_PORT || "3001", 10);

mkdirSync(AUTH_DIR, { recursive: true });

// --- Message store for retry handling ---
const messageStore = new Map();

function getMessage(key) {
  const msg = messageStore.get(key.id);
  return msg?.message || undefined;
}

// --- Group metadata cache for session establishment ---
const groupCache = new Map();

// --- WebSocket server for lukan ---
const wss = new WebSocketServer({ port: WS_PORT });
const clients = new Set();

wss.on("connection", (ws) => {
  console.log("[connector] lukan connected");
  clients.add(ws);

  const status = sock?.user ? "connected" : "disconnected";
  ws.send(JSON.stringify({ type: "status", status }));

  // Send available groups on connect
  if (groupCache.size > 0) {
    const groups = [...groupCache.entries()].map(([id, meta]) => ({
      id,
      subject: meta.subject || "",
      participants: meta.participants?.length || 0,
    }));
    ws.send(JSON.stringify({ type: "groups", groups }));
  }

  ws.on("message", async (raw) => {
    try {
      const msg = JSON.parse(raw.toString());
      if (msg.type === "send" && msg.to && msg.text) {
        await sendWhatsAppMessage(msg.to, msg.text);
      } else if (msg.type === "list_groups") {
        const groups = [...groupCache.entries()].map(([id, meta]) => ({
          id,
          subject: meta.subject || "",
          participants: meta.participants?.length || 0,
        }));
        ws.send(JSON.stringify({ type: "groups", groups }));
      }
    } catch (err) {
      console.error("[connector] Error processing outgoing message:", err.message);
    }
  });

  ws.on("close", () => {
    console.log("[connector] lukan disconnected");
    clients.delete(ws);
  });
});

function broadcast(data) {
  const json = JSON.stringify(data);
  for (const ws of clients) {
    if (ws.readyState === 1) {
      ws.send(json);
    }
  }
}

console.log(`[connector] WebSocket server listening on ws://localhost:${WS_PORT}`);

// --- WhatsApp connection ---
let sock = null;

async function sendWhatsAppMessage(to, text, retries = 3) {
  if (!sock) {
    console.error("[connector] Cannot send — WhatsApp not connected");
    return;
  }

  // For groups, always refresh metadata before sending
  if (to.endsWith("@g.us")) {
    try {
      const metadata = await sock.groupMetadata(to);
      groupCache.set(to, metadata);
      console.log(
        `[connector] Group ${to}: ${metadata.participants?.length} participants, subject="${metadata.subject}"`,
      );
    } catch (e) {
      console.log(`[connector] Could not fetch group metadata: ${e.message}`);
    }
  }

  for (let attempt = 1; attempt <= retries; attempt++) {
    try {
      await sock.sendMessage(to, { text });
      console.log(`[connector] Sent to ${to} (${text.length} chars)`);
      return;
    } catch (err) {
      console.error(`[connector] Attempt ${attempt}/${retries} failed for ${to}: ${err.message}`);
      if (attempt < retries) {
        await new Promise((r) => setTimeout(r, 2000 * attempt));
      }
    }
  }
}

function extractSender(jid) {
  return jid?.split("@")[0] || "";
}

async function startWhatsApp() {
  const { state, saveCreds } = await useMultiFileAuthState(AUTH_DIR);

  let version;
  try {
    const fetched = await fetchLatestWaWebVersion({});
    version = fetched.version;
    console.log(`[connector] Using WA Web version: ${version.join(".")}`);
  } catch {
    console.log("[connector] Could not fetch latest version, using default");
  }

  sock = makeWASocket({
    auth: {
      creds: state.creds,
      keys: makeCacheableSignalKeyStore(state.keys, logger),
    },
    logger,
    browser: Browsers.ubuntu("Chrome"),
    ...(version ? { version } : {}),
    printQRInTerminal: false,
    generateHighQualityLinkPreview: false,
    // Message store for retry handling
    getMessage,
    // Group metadata cache for session establishment
    cachedGroupMetadata: async (jid) => groupCache.get(jid),
  });

  sock.ev.on("creds.update", saveCreds);

  // Cache group metadata on updates
  sock.ev.on("groups.update", (updates) => {
    for (const update of updates) {
      if (update.id) {
        const existing = groupCache.get(update.id);
        if (existing) {
          groupCache.set(update.id, { ...existing, ...update });
        }
      }
    }
  });

  sock.ev.on("group-participants.update", async ({ id }) => {
    try {
      const metadata = await sock.groupMetadata(id);
      groupCache.set(id, metadata);
    } catch {}
  });

  sock.ev.on("connection.update", (update) => {
    const { connection, lastDisconnect, qr } = update;

    if (qr) {
      console.log("\n[connector] Scan this QR code with WhatsApp:\n");
      qrcode.generate(qr, { small: true });
      broadcast({ type: "status", status: "qr" });
    }

    if (connection === "open") {
      console.log("[connector] WhatsApp connected as", sock.user?.id);
      broadcast({ type: "status", status: "connected" });

      // Pre-cache all group metadata on connect
      sock
        .groupFetchAllParticipating()
        .then((groups) => {
          for (const [id, metadata] of Object.entries(groups)) {
            groupCache.set(id, metadata);
          }
          console.log(`[connector] Cached metadata for ${Object.keys(groups).length} groups`);
        })
        .catch(() => {});
    }

    if (connection === "close") {
      const statusCode = lastDisconnect?.error?.output?.statusCode;
      const shouldReconnect = statusCode !== DisconnectReason.loggedOut;
      console.log(`[connector] Connection closed (code ${statusCode}), reconnect: ${shouldReconnect}`);
      broadcast({ type: "status", status: "disconnected" });

      if (shouldReconnect) {
        setTimeout(() => startWhatsApp(), 3000);
      } else {
        console.log("[connector] Logged out. Delete auth dir and restart to re-authenticate.");
        process.exit(1);
      }
    }
  });

  // Store messages for retry handling
  sock.ev.on("messages.upsert", async ({ messages: msgs, type }) => {
    for (const msg of msgs) {
      if (msg.key.id) {
        messageStore.set(msg.key.id, msg);
      }
    }
    // Keep store bounded
    if (messageStore.size > 1000) {
      const keys = [...messageStore.keys()];
      for (let i = 0; i < keys.length - 500; i++) {
        messageStore.delete(keys[i]);
      }
    }

    if (type !== "notify") return;

    for (const msg of msgs) {
      if (msg.key.remoteJid === "status@broadcast") continue;
      if (msg.message?.reactionMessage) continue;
      if (msg.message?.protocolMessage) continue;

      const chatId = msg.key.remoteJid || "";
      const isGroup = chatId.endsWith("@g.us");

      // Allow fromMe in groups (so you can use /lukan yourself)
      // Skip fromMe in DMs (would create loops)
      if (msg.key.fromMe && !isGroup) continue;

      // Audio/voice messages — download and broadcast as "audio" event
      const audioMsg = msg.message?.audioMessage;
      if (audioMsg) {
        const sender = isGroup ? extractSender(msg.key.participant || "") : extractSender(chatId);

        console.log(
          `[connector] Audio from ${sender}${isGroup ? ` [group ${chatId}]` : ""} (${audioMsg.seconds || "?"}s, ptt=${!!audioMsg.ptt})`,
        );

        try {
          const buffer = await downloadMediaMessage(msg, "buffer", {});
          const audioBase64 = buffer.toString("base64");
          broadcast({
            type: "audio",
            sender,
            chatId,
            audioBase64,
            mimetype: audioMsg.mimetype || "audio/ogg; codecs=opus",
            seconds: audioMsg.seconds || 0,
            ptt: !!audioMsg.ptt,
            isGroup,
            messageId: msg.key.id,
          });
        } catch (err) {
          console.error(`[connector] Failed to download audio: ${err.message}`);
        }
        continue;
      }

      const text = msg.message?.conversation || msg.message?.extendedTextMessage?.text || "";

      if (!text) continue;

      const sender = isGroup ? extractSender(msg.key.participant || "") : extractSender(chatId);

      console.log(`[connector] ${sender}${isGroup ? ` [group ${chatId}]` : ""}: ${text.slice(0, 80)}`);

      broadcast({
        type: "message",
        sender,
        chatId,
        content: text,
        isGroup,
        messageId: msg.key.id,
      });
    }
  });
}

startWhatsApp().catch((err) => {
  console.error("[connector] Fatal error:", err);
  process.exit(1);
});
