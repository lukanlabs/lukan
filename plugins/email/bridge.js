#!/usr/bin/env node
// Email Plugin Bridge
//
// IMAP IDLE channel: listens for new emails, forwards to host, replies via SMTP.
// Follows the lukan plugin protocol (JSON lines on stdin/stdout).

import { ImapFlow } from "imapflow";
import nodemailer from "nodemailer";
import { createInterface } from "readline";

// ── State ───────────────────────────────────────────────────────────

let config = {};
let imapClient = null;
let smtpTransport = null;
let noopInterval = null;
let requestCounter = 0;
const pendingRequests = new Map();  // requestId → { sender, channelId, messageId }
const processing = new Set();       // sender emails currently being processed

// ── Protocol helpers ────────────────────────────────────────────────

function send(msg) {
  process.stdout.write(JSON.stringify(msg) + "\n");
}

function log(level, message) {
  send({ type: "log", level, message });
}

function sendStatus(status) {
  send({ type: "status", status });
}

// ── SMTP ────────────────────────────────────────────────────────────

function initSmtp() {
  smtpTransport = nodemailer.createTransport({
    host: config.smtpHost,
    port: config.smtpPort ?? 587,
    secure: config.smtpSecure ?? false,
    auth: { user: config.smtpUser, pass: config.smtpPass },
  });
}

async function sendReply(to, subject, body, inReplyTo, references) {
  const mailOpts = {
    from: config.smtpUser,
    to,
    subject,
    text: body,
  };
  if (inReplyTo) mailOpts.inReplyTo = inReplyTo;
  if (references) mailOpts.references = references;
  await smtpTransport.sendMail(mailOpts);
}

// ── Email filtering ─────────────────────────────────────────────────

function shouldProcess(fromAddr, subject) {
  const whitelist = config.whitelist ?? [];
  if (whitelist.length === 0) return false;

  const sender = fromAddr.toLowerCase();
  if (!whitelist.some((w) => w.toLowerCase() === sender)) return false;

  const prefix = config.prefix;
  if (prefix && !subject.toLowerCase().startsWith(prefix.toLowerCase())) return false;

  return true;
}

function stripPrefix(subject, prefix) {
  if (!prefix) return subject;
  if (subject.toLowerCase().startsWith(prefix.toLowerCase())) {
    return subject.slice(prefix.length).trim();
  }
  return subject;
}

// ── Body extraction ─────────────────────────────────────────────────

function extractTextBody(raw) {
  const boundaryMatch = /boundary="?([^"\s;]+)"?/i.exec(raw);
  if (boundaryMatch) {
    const boundary = boundaryMatch[1];
    const parts = raw.split(`--${boundary}`);
    for (const part of parts) {
      if (part.toLowerCase().includes("content-type: text/plain")) {
        const bodyStart = part.indexOf("\r\n\r\n");
        if (bodyStart !== -1) {
          return part.slice(bodyStart + 4).replace(/--$/, "").trim();
        }
      }
    }
  }
  const idx = raw.indexOf("\r\n\r\n");
  if (idx !== -1) return raw.slice(idx + 4).trim();
  return raw;
}

async function fetchEmailBody(client, uid) {
  const dl = await client.download(String(uid), undefined, { uid: true });
  const chunks = [];
  for await (const chunk of dl.content) chunks.push(chunk);
  return extractTextBody(Buffer.concat(chunks).toString("utf8"));
}

// ── Process new emails ──────────────────────────────────────────────

async function processNewEmails(client) {
  const lock = await client.getMailboxLock("INBOX");
  try {
    const uids = await client.search({ seen: false }, { uid: true });
    if (!uids || uids.length === 0) return;

    for (const uid of uids) {
      let envelope;
      for await (const msg of client.fetch(String(uid), { envelope: true, uid: true }, { uid: true })) {
        envelope = msg.envelope;
      }
      if (!envelope) continue;

      const from = envelope.from?.[0];
      const fromAddr = from?.address ?? "";
      const subject = envelope.subject ?? "";
      const messageId = envelope.messageId ?? "";

      if (!shouldProcess(fromAddr, subject)) {
        // Mark as seen so we don't re-process
        await client.messageFlagsAdd(String(uid), ["\\Seen"], { uid: true });
        continue;
      }

      if (processing.has(fromAddr)) {
        log("debug", `Skipping email from ${fromAddr} — already processing`);
        continue;
      }

      processing.add(fromAddr);

      const body = await fetchEmailBody(client, uid);
      const cleanSubject = stripPrefix(subject, config.prefix);
      const content = `Subject: ${cleanSubject}\n\n${body}`;

      const requestId = `email-${++requestCounter}`;
      const replySubject = subject.toLowerCase().startsWith("re:") ? subject : `Re: ${subject}`;

      pendingRequests.set(requestId, {
        sender: fromAddr,
        channelId: fromAddr,
        messageId,
        replySubject,
      });

      // Mark as read
      await client.messageFlagsAdd(String(uid), ["\\Seen"], { uid: true });

      log("info", `New email from ${fromAddr}: ${cleanSubject}`);

      send({
        type: "channelMessage",
        requestId,
        sender: fromAddr,
        channelId: fromAddr,
        content,
      });
    }
  } finally {
    lock.release();
  }
}

// ── IMAP connection loop ────────────────────────────────────────────

async function connect() {
  sendStatus("connecting");

  imapClient = new ImapFlow({
    host: config.imapHost,
    port: config.imapPort ?? 993,
    secure: config.imapSecure ?? true,
    auth: { user: config.imapUser, pass: config.imapPass },
    logger: false,
    connectionTimeout: 10_000,
  });

  await imapClient.connect();
  log("info", `IMAP connected to ${config.imapHost}`);

  await imapClient.mailboxOpen("INBOX");
  sendStatus("connected");

  // Process any existing unseen emails
  await processNewEmails(imapClient);

  // Listen for new emails
  imapClient.on("exists", async () => {
    try {
      await processNewEmails(imapClient);
    } catch (err) {
      log("error", `Error processing new emails: ${err.message}`);
    }
  });

  // NOOP keep-alive every 4 minutes
  noopInterval = setInterval(async () => {
    try {
      await imapClient.noop();
    } catch {
      // Will trigger close/reconnect
    }
  }, 4 * 60 * 1000);

  // Handle disconnection
  imapClient.on("close", () => {
    log("warn", "IMAP connection closed, reconnecting in 5s...");
    sendStatus("reconnecting");
    clearInterval(noopInterval);
    setTimeout(connectWithRetry, 5000);
  });
}

async function connectWithRetry() {
  try {
    await connect();
  } catch (err) {
    log("error", `IMAP connection failed: ${err.message}, retrying in 5s...`);
    sendStatus("reconnecting");
    setTimeout(connectWithRetry, 5000);
  }
}

// ── Host message handler ────────────────────────────────────────────

function handleHostMessage(msg) {
  switch (msg.type) {
    case "init":
      log("info", `Received init for plugin "${msg.name}"`);
      config = msg.config || {};
      send({ type: "ready", version: "0.1.0", capabilities: [] });

      // Verify config
      if (!config.imapHost || !config.imapUser || !config.imapPass) {
        log("error", "IMAP not configured. Use: lukan mail config set imap_host/imap_user/imap_pass");
        sendStatus("disconnected");
        return;
      }
      if (!config.smtpHost || !config.smtpUser || !config.smtpPass) {
        log("error", "SMTP not configured. Use: lukan mail config set smtp_host/smtp_user/smtp_pass");
        sendStatus("disconnected");
        return;
      }

      initSmtp();
      connectWithRetry();
      break;

    case "agentResponse": {
      const { requestId, text, isError } = msg;
      const meta = pendingRequests.get(requestId);
      if (!meta) {
        log("warn", `No pending request for ${requestId}`);
        break;
      }
      pendingRequests.delete(requestId);
      processing.delete(meta.sender);

      if (!isError && smtpTransport) {
        const replyBody = text.length > 10_000 ? text.slice(0, 10_000) + "\n...(truncated)" : text;
        sendReply(meta.sender, meta.replySubject, replyBody, meta.messageId, meta.messageId)
          .then(() => log("info", `Reply sent to ${meta.sender}`))
          .catch((err) => log("error", `Failed to reply to ${meta.sender}: ${err.message}`));
      }
      break;
    }

    case "shutdown":
      shutdown();
      break;
  }
}

// ── Stdin reader ────────────────────────────────────────────────────

const rl = createInterface({ input: process.stdin });
rl.on("line", (line) => {
  const trimmed = line.trim();
  if (!trimmed) return;
  try {
    handleHostMessage(JSON.parse(trimmed));
  } catch (err) {
    process.stderr.write(`[email-bridge] Parse error: ${err.message}\n`);
  }
});

rl.on("close", () => {
  process.stderr.write("[email-bridge] stdin closed, exiting\n");
  shutdown();
});

// ── Shutdown ────────────────────────────────────────────────────────

function shutdown() {
  clearInterval(noopInterval);
  if (imapClient) {
    imapClient.close().catch(() => {});
  }
  process.exit(0);
}

process.on("SIGTERM", shutdown);
process.on("SIGINT", shutdown);
