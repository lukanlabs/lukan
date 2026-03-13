#!/usr/bin/env node
// Telegram plugin — tool handler
// Usage: node tools.js <tool_name>
// Input:  JSON via stdin
// Output: { "output": "...", "isError": false } via stdout

import fs from "fs";
import path from "path";
import { fileURLToPath } from "url";
import { homedir } from "os";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

const XDG_DATA_HOME = process.env.XDG_DATA_HOME || path.join(homedir(), ".local", "share");
const HISTORY_PATH = path.join(XDG_DATA_HOME, "lukan", "plugins", "telegram", "message_history.json");

// ── Config ──────────────────────────────────────────────────────────

function loadConfig() {
  // Try XDG config dir first (where lukan stores plugin configs)
  const xdgConfig = process.env.XDG_CONFIG_HOME || path.join(homedir(), ".config");
  const configPath = path.join(xdgConfig, "lukan", "plugins", "telegram", "config.json");
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

// ── Telegram Bot API ────────────────────────────────────────────────

const API_BASE = "https://api.telegram.org";

async function apiCall(token, method, body = {}) {
  const url = `${API_BASE}/bot${token}/${method}`;
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

async function apiCallMultipart(token, method, formData) {
  const url = `${API_BASE}/bot${token}/${method}`;
  const res = await fetch(url, {
    method: "POST",
    body: formData,
  });

  const data = await res.json();
  if (!data.ok) {
    throw new Error(`Telegram API error: ${data.description || res.status}`);
  }
  return data.result;
}

// ── Message history ─────────────────────────────────────────────────

function loadHistory() {
  try {
    return JSON.parse(fs.readFileSync(HISTORY_PATH, "utf8"));
  } catch {
    return { chats: {} };
  }
}

// ── Tool handlers ───────────────────────────────────────────────────

const handlers = {
  async TelegramSend(input, config) {
    const { chatId, text } = input;
    const token = config.botToken;
    if (!token) throw new Error("No botToken configured");

    const MAX_LEN = 4096;
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
      try {
        await apiCall(token, "sendMessage", {
          chat_id: chatId,
          text: chunk,
          parse_mode: "Markdown",
        });
      } catch {
        // Retry without Markdown if it fails
        await apiCall(token, "sendMessage", { chat_id: chatId, text: chunk });
      }
    }

    return `Message sent to chat ${chatId} (${text.length} chars).`;
  },

  async TelegramSendMedia(input, config) {
    const { chatId, filePath, mediaType, caption } = input;
    const token = config.botToken;
    if (!token) throw new Error("No botToken configured");

    if (!fs.existsSync(filePath)) {
      throw new Error(`File not found: ${filePath}`);
    }

    const methodMap = {
      photo: "sendPhoto",
      document: "sendDocument",
      audio: "sendAudio",
      video: "sendVideo",
    };
    const fieldMap = {
      photo: "photo",
      document: "document",
      audio: "audio",
      video: "video",
    };

    const method = methodMap[mediaType];
    const field = fieldMap[mediaType];
    if (!method) throw new Error(`Invalid media type: ${mediaType}`);

    const fileBuffer = fs.readFileSync(filePath);
    const fileName = path.basename(filePath);

    const formData = new FormData();
    formData.append("chat_id", chatId);
    formData.append(field, new Blob([fileBuffer]), fileName);
    if (caption) formData.append("caption", caption);

    await apiCallMultipart(token, method, formData);

    return `${mediaType} sent to chat ${chatId}${caption ? ` with caption: "${caption}"` : ""}.`;
  },

  async TelegramGetChats(input, _config) {
    const limit = input.limit || 20;
    const history = loadHistory();
    const chatEntries = Object.entries(history.chats);

    if (chatEntries.length === 0) return "No chats found in history.";

    // Sort by most recent message
    chatEntries.sort((a, b) => {
      const aLast = a[1].messages?.[a[1].messages.length - 1]?.timestamp || 0;
      const bLast = b[1].messages?.[b[1].messages.length - 1]?.timestamp || 0;
      return bLast - aLast;
    });

    const lines = chatEntries.slice(0, limit).map(([id, chat]) => {
      const type = chat.type === "private" ? "[private]" : `[${chat.type || "chat"}]`;
      const lastMsg = chat.messages?.[chat.messages.length - 1];
      const preview = lastMsg ? ` — ${lastMsg.text?.slice(0, 60) || ""}` : "";
      return `${type} ${chat.name || "Unknown"} (${id})${preview}`;
    });

    return lines.join("\n");
  },

  async TelegramReadMessages(input, _config) {
    const { chatId } = input;
    const limit = input.limit || 20;
    const history = loadHistory();

    const chat = history.chats[chatId];
    if (!chat || !chat.messages || chat.messages.length === 0) {
      return `No messages found in chat ${chatId}.`;
    }

    const messages = chat.messages.slice(-limit);
    const lines = messages.map((m) => {
      const time = new Date(m.timestamp).toLocaleString();
      return `[${time}] ${m.sender}: ${m.text}`;
    });

    return lines.join("\n");
  },

  async TelegramGetChatInfo(input, config) {
    const { chatId } = input;
    const token = config.botToken;
    if (!token) throw new Error("No botToken configured");

    const chat = await apiCall(token, "getChat", { chat_id: chatId });

    const lines = [
      `Title: ${chat.title || chat.first_name || "Unknown"}`,
      `Type: ${chat.type}`,
      `ID: ${chat.id}`,
    ];

    if (chat.description) lines.push(`Description: ${chat.description}`);
    if (chat.username) lines.push(`Username: @${chat.username}`);

    // Get member count for groups
    if (chat.type !== "private") {
      try {
        const count = await apiCall(token, "getChatMemberCount", { chat_id: chatId });
        lines.push(`Members: ${count}`);
      } catch {}
    }

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
