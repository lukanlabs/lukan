#!/usr/bin/env node
// WhatsApp Plugin CLI — handles custom commands: auth, logout, groups
//
// Usage:
//   node cli.js auth    — Authenticate by scanning QR code
//   node cli.js logout  — Delete WhatsApp session
//   node cli.js groups  — List available WhatsApp groups

import { join } from "path";
import { homedir } from "os";
import { existsSync, rmSync, readFileSync, writeFileSync } from "fs";
import { resolve, dirname } from "path";
import { fileURLToPath } from "url";
import { spawn } from "child_process";
import { createServer } from "net";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

const RESET = "\x1b[0m";
const BOLD = "\x1b[1m";
const DIM = "\x1b[2m";
const GREEN = "\x1b[32m";
const YELLOW = "\x1b[33m";
const CYAN = "\x1b[36m";
const RED = "\x1b[31m";

const XDG_DATA_HOME =
  process.env.XDG_DATA_HOME || join(homedir(), ".local", "share");
const AUTH_DIR = join(XDG_DATA_HOME, "lukan", "whatsapp-auth");
const CREDS_FILE = join(AUTH_DIR, "creds.json");

// Load plugin config to get bridge URL
const XDG_CONFIG_HOME =
  process.env.XDG_CONFIG_HOME || join(homedir(), ".config");
const PLUGIN_CONFIG = join(XDG_CONFIG_HOME, "lukan", "plugins", "whatsapp", "config.json");

function loadConfig() {
  try {
    if (existsSync(PLUGIN_CONFIG)) {
      return JSON.parse(readFileSync(PLUGIN_CONFIG, "utf-8"));
    }
  } catch {}
  return {};
}

// ── Find connector path ─────────────────────────────────────────────

function findConnectorPath() {
  const candidates = [
    resolve(__dirname, "whatsapp-connector/index.js"),
    resolve(__dirname, "../../whatsapp-connector/index.js"),
    resolve(process.cwd(), "whatsapp-connector/index.js"),
  ];
  for (const p of candidates) {
    if (existsSync(p)) return p;
  }
  return null;
}

// ── Port check ──────────────────────────────────────────────────────

function isPortInUse(port) {
  return new Promise((resolve) => {
    const srv = createServer();
    srv.once("error", (err) => {
      if (err.code === "EADDRINUSE") resolve(true);
      else resolve(false);
    });
    srv.once("listening", () => {
      srv.close(() => resolve(false));
    });
    srv.listen(port);
  });
}

// ── Auth command ─────────────────────────────────────────────────────

async function cmdAuth() {
  if (existsSync(CREDS_FILE)) {
    console.log(`${GREEN}✓${RESET} WhatsApp already authenticated.`);
    console.log(`${DIM}To re-authenticate, run: lukan wa logout${RESET}`);
    return;
  }

  const connectorPath = findConnectorPath();
  if (!connectorPath) {
    console.error(
      `${RED}Could not find whatsapp-connector/index.js${RESET}`
    );
    process.exit(1);
  }

  // Check if connector port is already in use (plugin daemon running)
  const config = loadConfig();
  const bridgeUrl = config.bridgeUrl || "ws://localhost:3001";
  const portMatch = bridgeUrl.match(/:(\d+)/);
  const port = portMatch ? parseInt(portMatch[1], 10) : 3001;

  if (await isPortInUse(port)) {
    console.log(
      `${RED}✗${RESET} Port ${port} is already in use — the WhatsApp connector is already running.`
    );
    console.log(`\nStop the plugin first, then retry:`);
    console.log(`  ${CYAN}lukan wa stop${RESET}`);
    console.log(`  ${CYAN}lukan wa auth${RESET}`);
    process.exit(1);
  }

  console.log("Starting connector for QR authentication...\n");

  const connectorDir = dirname(connectorPath);
  const child = spawn("node", [connectorPath], {
    cwd: connectorDir,
    stdio: ["ignore", "inherit", "inherit"],
  });

  // Poll for auth file
  const maxWait = 120_000; // 2 minutes
  const start = Date.now();
  let authenticated = false;

  while (Date.now() - start < maxWait) {
    await new Promise((r) => setTimeout(r, 2000));
    if (existsSync(CREDS_FILE)) {
      authenticated = true;
      break;
    }
  }

  child.kill("SIGTERM");

  if (authenticated) {
    console.log(`\n${GREEN}✓${RESET} WhatsApp authenticated successfully!`);
    console.log("\nStart the daemon with:");
    console.log("  lukan wa start");
  } else {
    console.log(`\n${RED}✗${RESET} Authentication timed out (2 minutes).`);
    console.log("Try again with: lukan wa auth");
  }
}

// ── Logout command ───────────────────────────────────────────────────

function cmdLogout() {
  if (!existsSync(AUTH_DIR)) {
    console.log(`${YELLOW}No WhatsApp session found.${RESET}`);
    return;
  }

  try {
    rmSync(AUTH_DIR, { recursive: true, force: true });
    console.log(`${GREEN}✓${RESET} WhatsApp session deleted.`);
    console.log(`${DIM}Run: lukan wa auth${RESET}`);
  } catch (err) {
    console.error(`${RED}Failed to delete session: ${err.message}${RESET}`);
  }
}

// ── Groups command ───────────────────────────────────────────────────

async function cmdGroups() {
  const config = loadConfig();
  const bridgeUrl = config.bridgeUrl || "ws://localhost:3001";
  const allowedGroups = config.allowedGroups || [];

  console.log(`Connecting to connector at ${bridgeUrl}...`);

  // Dynamic import of ws
  const { WebSocket } = await import("ws");

  return new Promise((resolvePromise) => {
    const ws = new WebSocket(bridgeUrl);
    const timeout = setTimeout(() => {
      ws.close();
      console.log(
        `${RED}Timeout waiting for groups list from connector.${RESET}`
      );
      resolvePromise();
    }, 5000);

    ws.on("error", () => {
      clearTimeout(timeout);
      console.log(
        `${RED}Could not connect to connector. Is it running?${RESET}`
      );
      resolvePromise();
    });

    ws.on("open", () => {
      ws.send(JSON.stringify({ type: "list_groups" }));
    });

    ws.on("message", (raw) => {
      try {
        const data = JSON.parse(raw.toString());
        if (data.type === "groups") {
          clearTimeout(timeout);
          const groups = data.groups || [];

          if (groups.length === 0) {
            console.log(`${YELLOW}No groups found.${RESET}`);
          } else {
            console.log(
              `${BOLD}${groups.length} groups available:${RESET}\n`
            );
            for (const g of groups) {
              const mark = allowedGroups.includes(g.id)
                ? `${GREEN} ✓${RESET}`
                : "  ";
              console.log(`${mark} ${CYAN}${g.id}${RESET}`);
              console.log(
                `    ${g.subject} (${g.participants} members)\n`
              );
            }
            console.log(`${DIM}Use: lukan wa allowed_groups add <id>${RESET}`);
          }
          ws.close();
          resolvePromise();
        }
      } catch {}
    });
  });
}

// ── Groups JSON (for Rust picker) ────────────────────────────────────

async function cmdGroupsJson() {
  const config = loadConfig();
  const bridgeUrl = config.bridgeUrl || "ws://localhost:3001";
  const { WebSocket } = await import("ws");

  return new Promise((resolvePromise) => {
    const ws = new WebSocket(bridgeUrl);
    const timeout = setTimeout(() => {
      ws.close();
      console.log("[]");
      resolvePromise();
    }, 5000);

    ws.on("error", () => {
      clearTimeout(timeout);
      console.log("[]");
      resolvePromise();
    });

    ws.on("open", () => {
      ws.send(JSON.stringify({ type: "list_groups" }));
    });

    ws.on("message", (raw) => {
      try {
        const data = JSON.parse(raw.toString());
        if (data.type === "groups") {
          clearTimeout(timeout);
          console.log(JSON.stringify(data.groups || []));
          ws.close();
          resolvePromise();
        }
      } catch {}
    });
  });
}

// ── Main ─────────────────────────────────────────────────────────────

const command = process.argv[2];

switch (command) {
  case "auth":
    cmdAuth().catch((err) => {
      console.error(`${RED}Error: ${err.message}${RESET}`);
      process.exit(1);
    });
    break;
  case "logout":
    cmdLogout();
    break;
  case "groups":
    cmdGroups().catch((err) => {
      console.error(`${RED}Error: ${err.message}${RESET}`);
      process.exit(1);
    });
    break;
  case "groups-json":
    cmdGroupsJson().catch((err) => {
      console.log("[]");
      process.exit(1);
    });
    break;
  default:
    console.error(`Unknown command: ${command}`);
    console.error("Available: auth, logout, groups");
    process.exit(1);
}
