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
//     (+ Connect, Speak for voice notes)

import { Client, GatewayIntentBits, Partials } from "discord.js";
import { createInterface } from "readline";
import { Buffer } from "buffer";
import { writeFile, mkdir, readdir } from "fs/promises";
import { statSync } from "fs";
import { join, dirname } from "path";
import { fileURLToPath } from "url";
import { spawn } from "child_process";

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

// Voice / notes
let enableVoice = false;
let whisperUrl = "http://localhost:8787";
let openaiApiKey = "";
let transcriptionBackend = "local";
let notesAutoSummary = true;
let notesChannel = null; // text channel to post summary
let activeVoiceSession = null; // { guildId, channelId, channelName, helperProcess, startedAt, startedBy }

// Track pending requests
let requestCounter = 0;
const pendingRequests = new Map(); // requestId → { channel, message }
const pendingNotesMeta = new Map(); // requestId → { channelName, duration, startedBy, startedAt, rawTranscript }

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

  // Handle !notes commands before prefix stripping
  const notesMatch = content.match(/^!notes\s+(start|stop)$/i);
  if (notesMatch) {
    const action = notesMatch[1].toLowerCase();
    log("info", `[notes] !notes ${action} from ${message.author.username} (${message.author.id}) msg_id=${message.id}`);
    if (action === "start") {
      startNotes(message).catch((err) => log("error", `startNotes error: ${err.message}`));
    } else {
      stopNotes(message).catch((err) => log("error", `stopNotes error: ${err?.message || err}`));
    }
    return;
  }

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

// ── Notes persistence ─────────────────────────────────────────────────

const dataDir = process.env.PLUGIN_DATA_DIR || join(process.env.HOME || ".", ".config", "lukan", "plugins", "discord", "data");
const notesDir = join(dataDir, "notes");

async function saveNotesLocal(meta, rawTranscript, summary) {
  await mkdir(notesDir, { recursive: true });

  const date = new Date(meta.startedAt);
  const dateStr = date.toISOString().replace(/[:.]/g, "-").slice(0, 19);
  const safeName = meta.channelName.replace(/[^a-zA-Z0-9-_]/g, "_");
  const filename = `${dateStr}_${safeName}.md`;
  const filepath = join(notesDir, filename);

  const lines = [
    `# Meeting Notes — ${meta.channelName}`,
    ``,
    `- **Date**: ${date.toLocaleDateString("en-US", { weekday: "long", year: "numeric", month: "long", day: "numeric" })}`,
    `- **Duration**: ${formatDuration(meta.duration)}`,
    `- **Started by**: ${meta.startedBy}`,
    `- **Entries**: ${meta.entries}`,
    ``,
  ];

  if (summary) {
    lines.push(`## Summary`, ``, summary, ``);
  }

  lines.push(`## Transcript`, ``, rawTranscript, ``);

  await writeFile(filepath, lines.join("\n"), "utf-8");
  return filepath;
}

// ── Voice / Meeting Notes ──────────────────────────────────────────────

// ── Voice helper binary path ──────────────────────────────────────────

function findVoiceHelper() {
  const pluginDir = process.env.PLUGIN_DIR || dirname(fileURLToPath(import.meta.url));
  const candidates = [
    join(pluginDir, "voice-helper"),
    join(pluginDir, "voice-helper", "build", "voice-helper"),
    join(pluginDir, "voice-helper", "voice-helper"),
  ];
  return candidates.find((p) => {
    try { const s = statSync(p); return s.isFile(); } catch { return false; }
  });
}

async function startNotes(message) {
  if (!enableVoice) {
    await message.reply("Voice features are disabled. Enable `enableVoice` in plugin config.");
    return;
  }

  if (activeVoiceSession) {
    await message.reply("Already taking notes. Use `!notes stop` to end the current session.");
    return;
  }

  const member = message.member;
  if (!member?.voice?.channel) {
    await message.reply("You need to be in a voice channel first.");
    return;
  }

  const voiceChannel = member.voice.channel;
  notesChannel = message.channel;

  const helperBin = findVoiceHelper();
  if (!helperBin) {
    await message.reply("Voice helper binary not found. Install it in the plugin directory.");
    log("error", "voice-helper binary not found");
    return;
  }

  const audioDir = join(
    process.env.PLUGIN_DATA_DIR || join(process.env.HOME || ".", ".config", "lukan", "plugins", "discord", "data"),
    "audio",
    `session-${Date.now()}`,
  );

  log("info", `Spawning voice-helper for ${voiceChannel.name} (${voiceChannel.id})`);

  try {
    const helperProc = spawn(helperBin, [
      botToken,
      voiceChannel.guild.id.toString(),
      voiceChannel.id.toString(),
      audioDir,
    ], {
      stdio: ["pipe", "pipe", "pipe"],
    });

    // Wait for "joined" event from helper
    const joined = await new Promise((resolve, reject) => {
      const timeout = setTimeout(() => reject(new Error("Voice helper timed out")), 30_000);

      const rlHelper = createInterface({ input: helperProc.stdout });
      rlHelper.on("line", (line) => {
        try {
          const msg = JSON.parse(line);
          if (msg.type === "joined") {
            clearTimeout(timeout);
            resolve(rlHelper);
          } else if (msg.type === "error") {
            clearTimeout(timeout);
            reject(new Error(msg.message));
          }
        } catch {}
      });

      helperProc.on("error", (err) => {
        clearTimeout(timeout);
        reject(err);
      });

      helperProc.on("close", (code) => {
        clearTimeout(timeout);
        if (code !== 0) reject(new Error(`voice-helper exited with code ${code}`));
      });

      // Log stderr
      const rlErr = createInterface({ input: helperProc.stderr });
      rlErr.on("line", (line) => log("debug", `[voice-helper] ${line}`));
    });

    activeVoiceSession = {
      guildId: voiceChannel.guild.id.toString(),
      channelId: voiceChannel.id.toString(),
      channelName: voiceChannel.name,
      helperProcess: helperProc,
      helperReader: joined,
      audioDir,
      startedAt: new Date(),
      startedBy: message.author.username,
    };

    log("info", `Notes started in ${voiceChannel.name} by ${message.author.username}`);
    await message.reply(`Taking notes in **${voiceChannel.name}**. Say \`!notes stop\` when done.`);

  } catch (err) {
    log("error", `Failed to start voice helper: ${err.message}`);
    await message.reply(`Failed to join voice channel: ${err.message}`);
    activeVoiceSession = null;
  }
}

async function stopNotes(message) {
  if (!activeVoiceSession) {
    await message.reply("No active notes session.");
    return;
  }

  const session = activeVoiceSession;
  const duration = Math.round((Date.now() - session.startedAt.getTime()) / 1000);
  activeVoiceSession = null;

  const target = notesChannel || message.channel;
  await target.send(`Stopping recording (${formatDuration(duration)})...`);

  // Tell helper to stop — it will save WAV files and send audio events
  const audioFiles = await new Promise((resolve) => {
    const files = [];
    const timeout = setTimeout(() => resolve(files), 15_000);

    session.helperReader.on("line", (line) => {
      try {
        const msg = JSON.parse(line);
        if (msg.type === "audio") {
          files.push({ user: msg.user, userId: msg.userId, file: msg.file });
        } else if (msg.type === "left") {
          clearTimeout(timeout);
          resolve(files);
        }
      } catch {}
    });

    // Send stop command
    try {
      session.helperProcess.stdin.write("stop\n");
    } catch (e) {
      log("error", `Failed to send stop to voice helper: ${e.message}`);
      clearTimeout(timeout);
      resolve(files);
    }

    // Force-kill helper if it doesn't exit within the timeout
    session.helperProcess.once("exit", () => clearTimeout(timeout));
  });

  // Give the helper 2s to exit gracefully, then force-kill
  setTimeout(() => {
    try { session.helperProcess.kill("SIGKILL"); } catch {}
  }, 2000);

  if (audioFiles.length === 0) {
    await target.send("Notes session ended. No speech was captured.");
    log("info", "Notes session ended — no audio files");
    return;
  }

  log("info", `Voice helper produced ${audioFiles.length} audio files`);

  // Transcribe each user's audio
  const transcript = [];
  for (const af of audioFiles) {
    try {
      const text = await transcribeWavFile(af.file);
      if (text && text.trim()) {
        transcript.push({ speaker: af.user, text: text.trim() });
        log("info", `[notes] ${af.user}: ${text.trim().slice(0, 80)}`);
      }
    } catch (err) {
      log("error", `Transcription error for ${af.user}: ${err.message}`);
    }
  }

  if (transcript.length === 0) {
    await target.send("Notes session ended. Could not transcribe any audio.");
    log("info", "Notes session ended — no transcriptions");
    return;
  }

  const rawTranscript = transcript
    .map((e) => `**${e.speaker}**: ${e.text}`)
    .join("\n\n");

  log("info", `Notes session ended. ${transcript.length} speakers, ${duration}s duration`);

  const notesMeta = {
    channelName: session.channelName,
    duration,
    startedBy: session.startedBy,
    startedAt: session.startedAt.toISOString(),
    entries: transcript.length,
  };
  const savedPath = await saveNotesLocal(notesMeta, rawTranscript, null);
  log("info", `Raw transcript saved to ${savedPath}`);

  if (notesAutoSummary) {
    const requestId = `dc-notes-${++requestCounter}`;
    pendingRequests.set(requestId, { channel: target, message });
    pendingNotesMeta.set(requestId, { ...notesMeta, rawTranscript, savedPath });

    const content = [
      `[Meeting Notes Request]`,
      `Channel: ${session.channelName}`,
      `Duration: ${formatDuration(duration)}`,
      `Started by: ${session.startedBy}`,
      `Speakers: ${transcript.length}`,
      ``,
      `Transcript:`,
      rawTranscript,
      ``,
      `Please generate a meeting summary with: key points discussed, decisions made, and action items.`,
    ].join("\n");

    send({
      type: "channelMessage",
      requestId,
      sender: `${message.author.username} (system/notes)`,
      channelId: message.channel.id,
      content,
    });

    await target.send(`Transcribed ${transcript.length} speaker(s). Generating summary...`);
  } else {
    await sendDiscordMessage(target, message, `**Meeting Notes — ${session.channelName}**\n${formatDuration(duration)}\n\n${rawTranscript}`);
  }
}

// ── Transcription ─────────────────────────────────────────────────────

async function transcribeWavFile(filepath) {
  if (transcriptionBackend === "local") {
    return transcribeLocal(filepath);
  }
  const { readFile } = await import("fs/promises");
  const wavBuffer = await readFile(filepath);
  return transcribeOpenAI(wavBuffer);
}

async function transcribeLocal(filepath) {
  log("info", `[transcribe] Sending ${filepath} to ${whisperUrl}`);
  const { readFile } = await import("fs/promises");
  const buffer = await readFile(filepath);
  log("info", `[transcribe] File size: ${buffer.length} bytes`);

  // Use same multipart approach as WhatsApp plugin (proven to work)
  const boundary = `----FormBoundary${Date.now()}`;
  const fileHeader = Buffer.from(
    `--${boundary}\r\nContent-Disposition: form-data; name="file"; filename="audio.wav"\r\nContent-Type: audio/wav\r\n\r\n`,
  );
  const footer = Buffer.from(`\r\n--${boundary}--\r\n`);
  const body = Buffer.concat([fileHeader, buffer, footer]);

  const url = `${whisperUrl}/v1/audio/transcriptions`;
  const response = await fetch(url, {
    method: "POST",
    headers: { "Content-Type": `multipart/form-data; boundary=${boundary}` },
    body,
  });

  if (!response.ok) {
    const text = await response.text();
    throw new Error(`Whisper server error ${response.status}: ${text}`);
  }

  const result = await response.json();
  log("info", `[transcribe] Result: ${(result.text || "").slice(0, 100)}`);
  return result.text || "";
}

async function transcribeOpenAI(wavBuffer) {
  if (!openaiApiKey) throw new Error("No OPENAI_API_KEY configured");

  const boundary = `----FormBoundary${Date.now()}`;
  const parts = [];
  parts.push(
    `--${boundary}\r\nContent-Disposition: form-data; name="model"\r\n\r\ngpt-4o-transcribe\r\n`,
  );
  parts.push(
    `--${boundary}\r\nContent-Disposition: form-data; name="file"; filename="audio.wav"\r\nContent-Type: audio/wav\r\n\r\n`,
  );

  const header = Buffer.from(parts.join(""));
  const footer = Buffer.from(`\r\n--${boundary}--\r\n`);
  const body = Buffer.concat([header, wavBuffer, footer]);

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

function formatDuration(seconds) {
  if (seconds < 60) return `${seconds}s`;
  const min = Math.floor(seconds / 60);
  const sec = seconds % 60;
  return sec > 0 ? `${min}m ${sec}s` : `${min}m`;
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

      // Voice config (value may arrive as boolean or string "true")
      enableVoice = config.enableVoice === true || config.enableVoice === "true";
      whisperUrl = config.whisperUrl || "http://localhost:8787";
      openaiApiKey = config.openaiApiKey || process.env.OPENAI_API_KEY || "";
      transcriptionBackend = config.transcriptionBackend || "local";
      notesAutoSummary = config.notesAutoSummary !== false; // default true

      if (!botToken) {
        sendError(
          "No bot token configured. Create a bot at https://discord.com/developers/applications and copy the token.",
          false,
        );
        return;
      }

      log(
        "info",
        `Config: channels=[${allowedChannels.join(",")}], users=[${allowedUsers.join(",")}], prefix=${prefix || "(none)"}, threads=${replyInThread}, voice=${enableVoice}`,
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

      // If this is a notes summary response, save it locally
      const notesMeta = pendingNotesMeta.get(requestId);
      if (notesMeta && !isError) {
        pendingNotesMeta.delete(requestId);
        saveNotesLocal(notesMeta, notesMeta.rawTranscript, text)
          .then((path) => log("info", `Meeting summary saved to ${path}`))
          .catch((err) => log("error", `Failed to save notes summary: ${err.message}`));
      } else {
        pendingNotesMeta.delete(requestId);
      }

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
  if (activeVoiceSession && activeVoiceSession.helperProcess) {
    activeVoiceSession.helperProcess.stdin.write("stop\n");
    activeVoiceSession.helperProcess.kill();
    activeVoiceSession = null;
  }
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
