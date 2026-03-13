#!/usr/bin/env node
// WhatsApp plugin — tool handler
// Usage: node tools.js <tool_name>
// Input:  JSON via stdin
// Output: { "output": "...", "isError": false } via stdout

import fs from "fs";
import path from "path";
import { fileURLToPath } from "url";
import { homedir } from "os";
import { WebSocket } from "ws";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

// ── Config ──────────────────────────────────────────────────────────

function loadConfig() {
  // Try XDG config dir first (where lukan stores plugin configs)
  const xdgConfig = process.env.XDG_CONFIG_HOME || path.join(homedir(), ".config");
  const configPath = path.join(xdgConfig, "lukan", "plugins", "whatsapp", "config.json");
  try {
    return JSON.parse(fs.readFileSync(configPath, "utf8"));
  } catch {}
  // Fallback to plugin install dir
  try {
    return JSON.parse(fs.readFileSync(path.join(__dirname, "config.json"), "utf8"));
  } catch {
    return {};
  }
}

// ── WS command helper ───────────────────────────────────────────────

function connectorCommand(url, command, responseType, timeout = 5000) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(url);
    const timer = setTimeout(() => {
      ws.close();
      reject(new Error("Timeout waiting for connector response"));
    }, timeout);

    ws.on("error", (err) => {
      clearTimeout(timer);
      reject(new Error(`Could not connect to WhatsApp connector: ${err.message}`));
    });

    ws.on("open", () => {
      ws.send(JSON.stringify(command));
    });

    ws.on("message", (raw) => {
      try {
        const data = JSON.parse(raw.toString());
        if (data.type === responseType) {
          clearTimeout(timer);
          ws.close();
          resolve(data);
        }
      } catch {}
    });
  });
}

// ── Phone normalization ─────────────────────────────────────────────

function normalizeJid(to) {
  if (to.includes("@")) return to;
  // Strip leading + and non-digits
  const digits = to.replace(/[^\d]/g, "");
  return `${digits}@s.whatsapp.net`;
}

// ── Tool handlers ───────────────────────────────────────────────────

const handlers = {
  async WhatsAppSend(input, config) {
    const { text } = input;
    const to = normalizeJid(input.to);
    const url = config.bridgeUrl || "ws://localhost:3001";

    const requestId = `tool-${Date.now()}`;
    const result = await connectorCommand(
      url,
      { type: "send_message", requestId, to, text },
      "send_message_result",
    );

    if (result.success) {
      return `Message sent to ${to} (${text.length} chars).`;
    }
    throw new Error(result.error || "Failed to send message");
  },

  async WhatsAppSendMedia(input, config) {
    const { filePath, mediaType, caption, fileName } = input;
    const to = normalizeJid(input.to);
    const url = config.bridgeUrl || "ws://localhost:3001";

    if (!fs.existsSync(filePath)) {
      throw new Error(`File not found: ${filePath}`);
    }

    const base64 = fs.readFileSync(filePath).toString("base64");
    const resolvedFileName = fileName || path.basename(filePath);

    const requestId = `tool-${Date.now()}`;
    const result = await connectorCommand(
      url,
      {
        type: "send_media",
        requestId,
        to,
        mediaType,
        base64,
        fileName: resolvedFileName,
        caption: caption || "",
      },
      "send_media_result",
      15000,
    );

    if (result.success) {
      return `${mediaType} sent to ${to}${caption ? ` with caption: "${caption}"` : ""}.`;
    }
    throw new Error(result.error || "Failed to send media");
  },

  async WhatsAppListChats(input, config) {
    const url = config.bridgeUrl || "ws://localhost:3001";
    const limit = input.limit || 20;

    const result = await connectorCommand(
      url,
      { type: "list_chats" },
      "chats",
    );

    const chats = (result.chats || []).slice(0, limit);
    if (chats.length === 0) return "No chats found.";

    const lines = chats.map((c) => {
      const label = c.name || c.jid;
      const type = c.isGroup ? "[group]" : "[contact]";
      const last = c.lastMessage ? ` — ${c.lastMessage.slice(0, 60)}` : "";
      return `${type} ${label} (${c.jid})${last}`;
    });

    return lines.join("\n");
  },

  async WhatsAppReadMessages(input, config) {
    const { chatId } = input;
    const limit = input.limit || 20;
    const url = config.bridgeUrl || "ws://localhost:3001";

    const result = await connectorCommand(
      url,
      { type: "read_messages", chatId, limit },
      "chat_messages",
    );

    const messages = result.messages || [];
    if (messages.length === 0) return `No messages found in chat ${chatId}.`;

    const lines = messages.map((m) => {
      const time = new Date(m.timestamp).toLocaleString();
      const sender = m.fromMe ? "You" : m.sender;
      return `[${time}] ${sender}: ${m.text}`;
    });

    return lines.join("\n");
  },

  async WhatsAppSearchContacts(input, config) {
    const { query } = input;
    const url = config.bridgeUrl || "ws://localhost:3001";

    const result = await connectorCommand(
      url,
      { type: "search_contacts", query },
      "contacts_search",
    );

    const contacts = result.contacts || [];
    if (contacts.length === 0) return `No contacts matching "${query}".`;

    const lines = contacts.map((c) => {
      const type = c.isGroup ? "[group]" : "[contact]";
      return `${type} ${c.name} — ${c.jid}`;
    });

    return lines.join("\n");
  },
};

// ── Main ────────────────────────────────────────────────────────────

async function main() {
  const toolName = process.argv[2];
  if (!toolName || !handlers[toolName]) {
    console.log(JSON.stringify({ output: `Unknown tool: ${toolName}`, isError: true }));
    process.exit(1);
  }

  let inputData = "";
  for await (const chunk of process.stdin) inputData += chunk;

  const input = JSON.parse(inputData || "{}");
  const config = loadConfig();

  try {
    const result = await handlers[toolName](input, config);
    console.log(JSON.stringify({ output: result, isError: false }));
  } catch (err) {
    console.log(JSON.stringify({ output: err.message, isError: true }));
  }
}

main();
