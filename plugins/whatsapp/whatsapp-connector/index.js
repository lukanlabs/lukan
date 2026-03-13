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
import { mkdirSync, writeFileSync, readFileSync, unlinkSync, existsSync } from "fs";

const logger = pino({ level: "silent" });

const XDG_DATA_HOME = process.env.XDG_DATA_HOME || join(homedir(), ".local", "share");
const AUTH_DIR = process.env.PLUGIN_DATA_DIR || join(XDG_DATA_HOME, "lukan", "plugins", "whatsapp");
const WS_PORT = parseInt(process.env.CONNECTOR_PORT || "3001", 10);

mkdirSync(AUTH_DIR, { recursive: true });

const QR_FILE = join(AUTH_DIR, "current-qr.txt");

// --- Message store for retry handling ---
const messageStore = new Map();

// --- Chat history for tools (persisted to disk) ---
const HISTORY_PATH = join(AUTH_DIR, "chat_history.json");
const MAX_MESSAGES_PER_CHAT = 50;
const MAX_CHATS = 200;
let chatHistory = new Map();

function loadChatHistory() {
  try {
    if (existsSync(HISTORY_PATH)) {
      const data = JSON.parse(readFileSync(HISTORY_PATH, "utf8"));
      chatHistory = new Map(Object.entries(data));
      console.log(`[connector] Loaded chat history: ${chatHistory.size} chats`);
    }
  } catch (e) {
    console.error(`[connector] Failed to load chat history: ${e.message}`);
  }
}

let historySaveTimer = null;
function saveChatHistory() {
  // Debounce saves to avoid excessive disk I/O
  if (historySaveTimer) return;
  historySaveTimer = setTimeout(() => {
    historySaveTimer = null;
    try {
      const obj = Object.fromEntries(chatHistory);
      writeFileSync(HISTORY_PATH, JSON.stringify(obj));
    } catch (e) {
      console.error(`[connector] Failed to save chat history: ${e.message}`);
    }
  }, 2000);
}

// Load persisted history on startup
loadChatHistory();

function getMessage(key) {
  const msg = messageStore.get(key.id);
  return msg?.message || undefined;
}

// --- Group metadata cache for session establishment ---
const groupCache = new Map();

// --- Contact name cache (persisted to disk) ---
const CONTACTS_PATH = join(AUTH_DIR, "contact_names.json");
let contactNames = new Map(); // jid → name

function loadContactNames() {
  try {
    if (existsSync(CONTACTS_PATH)) {
      const data = JSON.parse(readFileSync(CONTACTS_PATH, "utf8"));
      contactNames = new Map(Object.entries(data));
      console.log(`[connector] Loaded ${contactNames.size} contact names from disk`);
    }
  } catch (e) {
    console.error(`[connector] Failed to load contact names: ${e.message}`);
  }
}

let contactsSaveTimer = null;
function saveContactNames() {
  if (contactsSaveTimer) return;
  contactsSaveTimer = setTimeout(() => {
    contactsSaveTimer = null;
    try {
      writeFileSync(CONTACTS_PATH, JSON.stringify(Object.fromEntries(contactNames)));
    } catch (e) {
      console.error(`[connector] Failed to save contact names: ${e.message}`);
    }
  }, 2000);
}

loadContactNames();

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
      } else if (msg.type === "send_message") {
        try {
          await sendWhatsAppMessage(msg.to, msg.text);
          ws.send(JSON.stringify({ type: "send_message_result", requestId: msg.requestId, success: true }));
        } catch (err) {
          ws.send(JSON.stringify({ type: "send_message_result", requestId: msg.requestId, success: false, error: err.message }));
        }
      } else if (msg.type === "send_media") {
        try {
          await sendWhatsAppMedia(msg.to, msg.mediaType, msg.base64, msg.fileName, msg.caption);
          ws.send(JSON.stringify({ type: "send_media_result", requestId: msg.requestId, success: true }));
        } catch (err) {
          ws.send(JSON.stringify({ type: "send_media_result", requestId: msg.requestId, success: false, error: err.message }));
        }
      } else if (msg.type === "list_chats") {
        const chats = getChatsFromHistory();
        ws.send(JSON.stringify({ type: "chats", chats }));
      } else if (msg.type === "read_messages") {
        const messages = chatHistory.get(msg.chatId) || [];
        const limited = messages.slice(-(msg.limit || 20));
        ws.send(JSON.stringify({ type: "chat_messages", chatId: msg.chatId, messages: limited }));
      } else if (msg.type === "search_contacts") {
        const contacts = searchContacts(msg.query);
        ws.send(JSON.stringify({ type: "contacts_search", contacts }));
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

async function sendWhatsAppMedia(to, mediaType, base64Data, fileName, caption) {
  if (!sock) throw new Error("WhatsApp not connected");

  const buffer = Buffer.from(base64Data, "base64");
  let content;

  switch (mediaType) {
    case "image":
      content = { image: buffer, caption: caption || undefined };
      break;
    case "document":
      content = { document: buffer, fileName: fileName || "file", caption: caption || undefined };
      break;
    case "audio":
      content = { audio: buffer, mimetype: "audio/mpeg" };
      break;
    case "video":
      content = { video: buffer, caption: caption || undefined };
      break;
    default:
      throw new Error(`Unknown media type: ${mediaType}`);
  }

  await sock.sendMessage(to, content);
  console.log(`[connector] Sent ${mediaType} to ${to}`);
}

function addToChatHistory(chatId, entry) {
  // Bound total chats
  if (!chatHistory.has(chatId) && chatHistory.size >= MAX_CHATS) {
    const oldest = chatHistory.keys().next().value;
    chatHistory.delete(oldest);
  }

  let messages = chatHistory.get(chatId) || [];
  // Deduplicate by messageId
  if (entry.messageId && messages.some((m) => m.messageId === entry.messageId)) {
    return;
  }
  messages.push(entry);
  if (messages.length > MAX_MESSAGES_PER_CHAT) {
    messages = messages.slice(-MAX_MESSAGES_PER_CHAT);
  }
  chatHistory.set(chatId, messages);
  saveChatHistory();
}

function getContactName(jid) {
  // Try contact names cache first, then group metadata, then phone number
  if (contactNames.has(jid)) return contactNames.get(jid);
  const groupMeta = groupCache.get(jid);
  if (groupMeta?.subject) return groupMeta.subject;
  return jid.split("@")[0];
}

function getChatsFromHistory() {
  const chats = [];
  for (const [jid, messages] of chatHistory) {
    const isGroup = jid.endsWith("@g.us");
    const name = getContactName(jid);
    const lastMsg = messages[messages.length - 1];
    chats.push({
      jid,
      name,
      isGroup,
      lastMessage: lastMsg?.text || "",
    });
  }
  // Sort by most recent
  chats.sort((a, b) => {
    const aTime = chatHistory.get(a.jid)?.slice(-1)[0]?.timestamp || 0;
    const bTime = chatHistory.get(b.jid)?.slice(-1)[0]?.timestamp || 0;
    return bTime - aTime;
  });
  return chats;
}

function searchContacts(query) {
  const q = query.toLowerCase();
  const results = [];
  const seen = new Set();

  // Search groups by subject
  for (const [jid, meta] of groupCache) {
    if ((meta.subject || "").toLowerCase().includes(q)) {
      results.push({ jid, name: meta.subject || jid, isGroup: true });
      seen.add(jid);
    }
  }

  // Search all known contacts by name (from Baileys contacts events)
  for (const [jid, name] of contactNames) {
    if (seen.has(jid)) continue;
    if (jid.endsWith("@g.us")) continue; // groups handled above
    const phone = jid.split("@")[0];
    if (name.toLowerCase().includes(q) || phone.includes(q)) {
      results.push({ jid, name, isGroup: false });
      seen.add(jid);
    }
  }

  // Also search chat history contacts not yet in contactNames
  for (const [jid] of chatHistory) {
    if (seen.has(jid) || jid.endsWith("@g.us")) continue;
    const phone = jid.split("@")[0];
    if (phone.includes(q)) {
      results.push({ jid, name: phone, isGroup: false });
    }
  }

  return results;
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
      writeFileSync(QR_FILE, qr, "utf-8");
      console.log(`[connector] QR saved to ${QR_FILE}`);
      broadcast({ type: "status", status: "qr" });
    }

    if (connection === "open") {
      console.log("[connector] WhatsApp connected as", sock.user?.id);
      try { unlinkSync(QR_FILE); } catch {}
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
      try { unlinkSync(QR_FILE); } catch {}
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

      // Capture pushName and chat history from ALL message types (not just notify)
      if (msg.key.remoteJid && msg.key.remoteJid !== "status@broadcast") {
        const chatId = msg.key.remoteJid;
        const isGroup = chatId.endsWith("@g.us");
        const fromMe = !!msg.key.fromMe;

        if (msg.pushName) {
          const senderJid = isGroup ? (msg.key.participant || "") : chatId;
          if (senderJid && !fromMe) {
            contactNames.set(senderJid, msg.pushName);
          }
        }

        const text = msg.message?.conversation || msg.message?.extendedTextMessage?.text || "";
        if (text && !msg.message?.reactionMessage && !msg.message?.protocolMessage) {
          const senderJid = isGroup ? (msg.key.participant || "") : chatId;
          const senderName = fromMe ? "You" : (contactNames.get(senderJid) || senderJid.split("@")[0]);
          addToChatHistory(chatId, {
            sender: senderName,
            text,
            timestamp: (msg.messageTimestamp || Math.floor(Date.now() / 1000)) * 1000,
            messageId: msg.key.id,
            fromMe,
          });
        }
      }
    }

    // Save contacts if any were updated
    saveContactNames();

    // Keep message store bounded
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
      const fromMe = !!msg.key.fromMe;

      // Allow fromMe in groups (so you can use /lukan yourself)
      // Skip fromMe in DMs (would create loops)
      if (fromMe && !isGroup) continue;

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
