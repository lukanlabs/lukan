#!/usr/bin/env node
// Email plugin — CLI command handlers
// Usage: node cli.js <command>
// Commands: setup, test-smtp, test-imap

import { ImapFlow } from "imapflow";
import nodemailer from "nodemailer";
import fs from "fs";
import path from "path";
import { createInterface } from "readline";
import { fileURLToPath } from "url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

function loadConfig() {
  try {
    return JSON.parse(fs.readFileSync(path.join(__dirname, "config.json"), "utf8"));
  } catch {
    return {};
  }
}

function saveConfig(config) {
  fs.writeFileSync(path.join(__dirname, "config.json"), JSON.stringify(config, null, 2));
}

function prompt(rl, question) {
  return new Promise((resolve) => rl.question(question, resolve));
}

// ── setup: Interactive Gmail quick-setup ─────────────────────────────

async function cmdSetup() {
  const rl = createInterface({ input: process.stdin, output: process.stdout });
  const config = loadConfig();

  console.log("\n  Email Plugin — Gmail Quick Setup\n");
  console.log("  You'll need a Google App Password.");
  console.log("  Create one at: https://myaccount.google.com/apppasswords\n");

  const email = await prompt(rl, "  Gmail address: ");
  const pass = await prompt(rl, "  App password:  ");

  config.smtpHost = "smtp.gmail.com";
  config.smtpPort = 465;
  config.smtpSecure = true;
  config.smtpUser = email.trim();
  config.smtpPass = pass.trim();

  config.imapHost = "imap.gmail.com";
  config.imapPort = 993;
  config.imapSecure = true;
  config.imapUser = email.trim();
  config.imapPass = pass.trim();

  saveConfig(config);
  rl.close();

  console.log("\n  Config saved. Testing connections...\n");

  // Test SMTP
  process.stdout.write("  SMTP: ");
  try {
    const transport = nodemailer.createTransport({
      host: config.smtpHost,
      port: config.smtpPort,
      secure: config.smtpSecure,
      auth: { user: config.smtpUser, pass: config.smtpPass },
    });
    await transport.verify();
    console.log("OK");
  } catch (err) {
    console.log(`FAILED — ${err.message}`);
  }

  // Test IMAP
  process.stdout.write("  IMAP: ");
  try {
    const client = new ImapFlow({
      host: config.imapHost,
      port: config.imapPort,
      secure: config.imapSecure,
      auth: { user: config.imapUser, pass: config.imapPass },
      logger: false,
      connectionTimeout: 10_000,
    });
    await client.connect();
    const mb = await client.mailboxOpen("INBOX");
    console.log(`OK — ${mb.exists} messages in INBOX`);
    await client.logout();
  } catch (err) {
    console.log(`FAILED — ${err.message}`);
  }

  console.log("\n  Next steps:");
  console.log("    lukan mail config add whitelist user@example.com");
  console.log("    lukan mail start\n");
}

// ── test-smtp ────────────────────────────────────────────────────────

async function cmdTestSmtp() {
  const config = loadConfig();
  if (!config.smtpHost || !config.smtpUser || !config.smtpPass) {
    console.log("SMTP not configured. Run: lukan mail setup");
    process.exit(1);
  }

  console.log(`Testing SMTP connection to ${config.smtpHost}:${config.smtpPort ?? 587}...`);
  try {
    const transport = nodemailer.createTransport({
      host: config.smtpHost,
      port: config.smtpPort ?? 587,
      secure: config.smtpSecure ?? false,
      auth: { user: config.smtpUser, pass: config.smtpPass },
    });
    await transport.verify();
    console.log("SMTP connection successful.");
  } catch (err) {
    console.log(`SMTP connection failed: ${err.message}`);
    process.exit(1);
  }
}

// ── test-imap ────────────────────────────────────────────────────────

async function cmdTestImap() {
  const config = loadConfig();
  if (!config.imapHost || !config.imapUser || !config.imapPass) {
    console.log("IMAP not configured. Run: lukan mail setup");
    process.exit(1);
  }

  console.log(`Testing IMAP connection to ${config.imapHost}:${config.imapPort ?? 993}...`);
  try {
    const client = new ImapFlow({
      host: config.imapHost,
      port: config.imapPort ?? 993,
      secure: config.imapSecure ?? true,
      auth: { user: config.imapUser, pass: config.imapPass },
      logger: false,
      connectionTimeout: 10_000,
    });
    await client.connect();
    const mb = await client.mailboxOpen("INBOX");
    console.log(`IMAP connection successful. INBOX has ${mb.exists} messages.`);
    await client.logout();
  } catch (err) {
    console.log(`IMAP connection failed: ${err.message}`);
    process.exit(1);
  }
}

// ── Main ─────────────────────────────────────────────────────────────

const command = process.argv[2];

switch (command) {
  case "setup":
    cmdSetup();
    break;
  case "test-smtp":
    cmdTestSmtp();
    break;
  case "test-imap":
    cmdTestImap();
    break;
  default:
    console.log(`Unknown command: ${command}`);
    process.exit(1);
}
