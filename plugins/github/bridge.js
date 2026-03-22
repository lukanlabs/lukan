#!/usr/bin/env node
// GitHub Plugin Bridge
// Minimal bridge — the plugin primarily uses webview for UI.
// This bridge just keeps the plugin process alive.

import { createInterface } from "readline";

const rl = createInterface({ input: process.stdin });

rl.on("line", (line) => {
  try {
    const msg = JSON.parse(line);
    if (msg.type === "configure") {
      // Store config
      process.stdout.write(JSON.stringify({ type: "log", level: "info", message: "GitHub plugin configured" }) + "\n");
    }
  } catch {}
});

// Keep alive
process.stdout.write(JSON.stringify({ type: "log", level: "info", message: "GitHub plugin started" }) + "\n");
