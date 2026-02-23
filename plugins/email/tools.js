#!/usr/bin/env node
// Email plugin — tool handler
// Usage: node tools.js <tool_name>
// Input:  JSON via stdin
// Output: { "output": "...", "isError": false } via stdout

import { ImapFlow } from "imapflow";
import nodemailer from "nodemailer";
import fs from "fs";
import path from "path";
import { fileURLToPath } from "url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const PENDING_PATH = path.join(__dirname, "pending.json");

// ── Config ──────────────────────────────────────────────────────────

function loadConfig() {
  try {
    return JSON.parse(fs.readFileSync(path.join(__dirname, "config.json"), "utf8"));
  } catch {
    return {};
  }
}

// ── Pending emails (file-backed) ────────────────────────────────────

function loadPending() {
  try {
    const data = JSON.parse(fs.readFileSync(PENDING_PATH, "utf8"));
    return { map: new Map(Object.entries(data.map || {})), nextId: data.nextId || 1 };
  } catch {
    return { map: new Map(), nextId: 1 };
  }
}

function savePending({ map, nextId }) {
  const obj = { map: Object.fromEntries(map), nextId };
  fs.writeFileSync(PENDING_PATH, JSON.stringify(obj, null, 2));
}

// ── IMAP helpers ────────────────────────────────────────────────────

function createImapClient(config) {
  const imap = config;
  return new ImapFlow({
    host: imap.imapHost,
    port: imap.imapPort ?? 993,
    secure: imap.imapSecure ?? true,
    auth: { user: imap.imapUser, pass: imap.imapPass },
    logger: false,
    connectionTimeout: 10_000,
  });
}

async function withImap(config, fn) {
  const client = createImapClient(config);
  await client.connect();
  try {
    return await fn(client);
  } finally {
    await client.logout();
  }
}

// ── SMTP helpers ────────────────────────────────────────────────────

function createSmtpTransport(config) {
  return nodemailer.createTransport({
    host: config.smtpHost,
    port: config.smtpPort ?? 587,
    secure: config.smtpSecure ?? false,
    auth: { user: config.smtpUser, pass: config.smtpPass },
  });
}

async function sendEmail(config, { to, subject, body, attachments, inReplyTo, references }) {
  const transport = createSmtpTransport(config);
  const mailOpts = {
    from: config.smtpUser,
    to,
    subject,
    text: body,
  };
  if (inReplyTo) mailOpts.inReplyTo = inReplyTo;
  if (references) mailOpts.references = references;
  if (attachments && attachments.length > 0) {
    mailOpts.attachments = attachments.map((p) => ({ path: p }));
  }
  await transport.sendMail(mailOpts);
  const attMsg = attachments?.length ? ` with ${attachments.length} attachment(s)` : "";
  return `Email sent to ${to}${attMsg}.`;
}

// ── Multipart body extraction ───────────────────────────────────────

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
  // Fallback: split on first blank line
  const idx = raw.indexOf("\r\n\r\n");
  if (idx !== -1) return raw.slice(idx + 4).trim();
  return raw;
}

// ── Tool handlers ───────────────────────────────────────────────────

const handlers = {
  async EmailList(input, config) {
    const folder = input.folder || "INBOX";
    const unseen = input.unseen ?? false;
    const limit = input.limit ?? 20;

    return withImap(config, async (client) => {
      const mb = await client.mailboxOpen(folder);
      const total = mb.exists;
      if (total === 0) return "No emails in folder.";

      const start = Math.max(1, total - limit + 1);
      const range = `${start}:${total}`;
      const lines = [];

      for await (const msg of client.fetch(range, { envelope: true, flags: true, uid: true })) {
        if (unseen && msg.flags.has("\\Seen")) continue;
        const env = msg.envelope;
        const from = env.from?.[0];
        const fromStr = from?.name
          ? `${from.name} <${from.address}>`
          : from?.address ?? "unknown";
        const date = env.date ? new Date(env.date).toISOString().slice(0, 16).replace("T", " ") : "?";
        const unread = msg.flags.has("\\Seen") ? "" : " *UNREAD*";
        lines.push(`[UID:${msg.uid}] ${date}${unread} | From: ${fromStr} | Subject: ${env.subject ?? "(no subject)"}`);
      }

      if (lines.length === 0) return unseen ? "No unread emails." : "No emails found.";
      return lines.join("\n");
    });
  },

  async EmailRead(input, config) {
    const uid = input.uid;
    const folder = input.folder || "INBOX";

    return withImap(config, async (client) => {
      await client.mailboxOpen(folder);

      // Fetch envelope
      let envelope;
      for await (const msg of client.fetch(String(uid), { envelope: true, uid: true }, { uid: true })) {
        envelope = msg.envelope;
      }
      if (!envelope) return `Email UID ${uid} not found.`;

      // Download body
      const dl = await client.download(String(uid), undefined, { uid: true });
      const chunks = [];
      for await (const chunk of dl.content) chunks.push(chunk);
      const raw = Buffer.concat(chunks).toString("utf8");
      const body = extractTextBody(raw);

      // Mark as read
      await client.messageFlagsAdd(String(uid), ["\\Seen"], { uid: true });

      const from = envelope.from?.[0];
      const fromStr = from?.name ? `${from.name} <${from.address}>` : from?.address ?? "unknown";
      const to = envelope.to?.map((t) => t.address).join(", ") ?? "";
      const date = envelope.date ? new Date(envelope.date).toISOString().slice(0, 16).replace("T", " ") : "?";

      const header = [
        `From: ${fromStr}`,
        `To: ${to}`,
        `Date: ${date}`,
        `Subject: ${envelope.subject ?? "(no subject)"}`,
        `Message-ID: ${envelope.messageId ?? ""}`,
      ].join("\n");

      const truncated = body.length > 10_000 ? body.slice(0, 10_000) + "\n...(truncated)" : body;

      return `${header}\n\n${truncated}`;
    });
  },

  async EmailSend(input, config) {
    const { to, subject, body, attachments } = input;

    const pending = loadPending();
    const id = pending.nextId++;
    pending.map.set(String(id), { id, to, subject, body, attachments });
    savePending(pending);

    const preview = [
      `Draft #${id} created (use EmailConfirm to send, EmailCancel to discard):`,
      `To: ${to}`,
      `Subject: ${subject}`,
      `Body: ${body.slice(0, 200)}${body.length > 200 ? "..." : ""}`,
    ];
    if (attachments?.length) preview.push(`Attachments: ${attachments.join(", ")}`);
    return preview.join("\n");
  },

  async EmailReply(input, config) {
    const { uid, body, attachments } = input;
    const folder = input.folder || "INBOX";

    // Fetch original envelope for threading
    const { subject: origSubject, from: origFrom, messageId } = await withImap(config, async (client) => {
      await client.mailboxOpen(folder);
      let env;
      for await (const msg of client.fetch(String(uid), { envelope: true, uid: true }, { uid: true })) {
        env = msg.envelope;
      }
      if (!env) throw new Error(`Email UID ${uid} not found.`);
      return {
        subject: env.subject ?? "",
        from: env.from?.[0]?.address ?? "",
        messageId: env.messageId ?? "",
      };
    });

    const replySubject = origSubject.toLowerCase().startsWith("re:") ? origSubject : `Re: ${origSubject}`;
    const to = origFrom;

    const pending = loadPending();
    const id = pending.nextId++;
    pending.map.set(String(id), {
      id, to, subject: replySubject, body, attachments,
      replyToUid: uid, folder, messageId,
    });
    savePending(pending);

    const preview = [
      `Reply draft #${id} created (use EmailConfirm to send, EmailCancel to discard):`,
      `To: ${to}`,
      `Subject: ${replySubject}`,
      `Body: ${body.slice(0, 200)}${body.length > 200 ? "..." : ""}`,
    ];
    if (attachments?.length) preview.push(`Attachments: ${attachments.join(", ")}`);
    return preview.join("\n");
  },

  async EmailFolders(_input, config) {
    return withImap(config, async (client) => {
      const folders = await client.list();
      return folders.map((f) => f.path).join("\n");
    });
  },

  async EmailConfirm(input, config) {
    const id = String(input.id);
    const pending = loadPending();
    const email = pending.map.get(id);
    if (!email) return `No pending email with ID ${input.id}.`;

    const opts = {
      to: email.to,
      subject: email.subject,
      body: email.body,
      attachments: email.attachments,
    };

    if (email.messageId) {
      opts.inReplyTo = email.messageId;
      opts.references = email.messageId;
    }

    const result = await sendEmail(config, opts);

    pending.map.delete(id);
    savePending(pending);

    return result;
  },

  async EmailCancel(input, _config) {
    const id = String(input.id);
    const pending = loadPending();
    if (!pending.map.has(id)) return `No pending email with ID ${input.id}.`;

    const email = pending.map.get(id);
    pending.map.delete(id);
    savePending(pending);

    return `Cancelled draft #${input.id} (To: ${email.to}, Subject: ${email.subject}).`;
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
