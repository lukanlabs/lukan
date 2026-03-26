#!/usr/bin/env node
// GitHub Plugin Bridge — keeps the plugin alive for webview

import { createInterface } from "readline";

const rl = createInterface({ input: process.stdin });

// Signal ready
process.stdout.write(JSON.stringify({ type: "ready", version: "0.1.0", capabilities: [] }) + "\n");

// Keep alive — process stdin messages
rl.on("line", (line) => {
  try {
    const msg = JSON.parse(line);
    if (msg.type === "ping") {
      process.stdout.write(JSON.stringify({ type: "pong" }) + "\n");
    }
  } catch {}
});

// Prevent exit
process.stdin.resume();
