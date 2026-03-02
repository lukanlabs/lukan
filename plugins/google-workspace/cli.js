#!/usr/bin/env node
// Google Workspace plugin CLI commands
// Usage: node cli.js <handler> [args...]

const fs = require("fs");
const path = require("path");
const { createHash, randomBytes } = require("crypto");
const http = require("http");
const { execSync } = require("child_process");

const CONFIG_PATH = path.join(__dirname, "config.json");

function loadConfig() {
  try {
    return JSON.parse(fs.readFileSync(CONFIG_PATH, "utf8"));
  } catch {
    return {};
  }
}

function saveConfig(config) {
  fs.writeFileSync(CONFIG_PATH, JSON.stringify(config, null, 2));
}

// ── OAuth2 PKCE ─────────────────────────────────────────────────────────

const AUTH_URL = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL = "https://oauth2.googleapis.com/token";
const REDIRECT_PORT = 1456;
const REDIRECT_URI = `http://localhost:${REDIRECT_PORT}/auth/callback`;
const SCOPES = [
  "https://www.googleapis.com/auth/spreadsheets",
  "https://www.googleapis.com/auth/calendar",
  "https://www.googleapis.com/auth/documents",
  "https://www.googleapis.com/auth/presentations",
  "https://www.googleapis.com/auth/drive",
].join(" ");

function base64url(buf) {
  return buf.toString("base64").replace(/\+/g, "-").replace(/\//g, "_").replace(/=/g, "");
}

async function authenticate() {
  const config = loadConfig();

  const clientId = config.clientId || process.env.GOOGLE_CLIENT_ID;
  const clientSecret = config.clientSecret || process.env.GOOGLE_CLIENT_SECRET;

  if (!clientId) {
    console.error(
      "\x1b[31mGoogle Client ID not configured.\x1b[0m\n" +
      "Set it via: lukan google client_id set \"your-client-id\"\n" +
      "Or: export GOOGLE_CLIENT_ID=...\n" +
      "Create credentials at https://console.cloud.google.com/apis/credentials"
    );
    process.exit(1);
  }

  if (!clientSecret) {
    console.error(
      "\x1b[31mGoogle Client Secret not configured.\x1b[0m\n" +
      "Set it via: lukan google client_secret set \"your-client-secret\""
    );
    process.exit(1);
  }

  // Generate PKCE verifier and challenge
  const verifier = base64url(randomBytes(32));
  const challenge = base64url(createHash("sha256").update(verifier).digest());
  const state = base64url(randomBytes(32));

  const authUrl = `${AUTH_URL}?${new URLSearchParams({
    response_type: "code",
    client_id: clientId,
    redirect_uri: REDIRECT_URI,
    scope: SCOPES,
    code_challenge: challenge,
    code_challenge_method: "S256",
    state,
    access_type: "offline",
    prompt: "consent",
  })}`;

  console.log("\n\x1b[1m\x1b[36m  lukan google auth\x1b[0m");
  console.log("\x1b[2m  Google Workspace authentication (OAuth2 PKCE)\x1b[0m\n");
  console.log("\x1b[2mOpening browser for Google authentication...\x1b[0m\n");

  // Open browser
  try {
    const platform = process.platform;
    if (platform === "darwin") execSync(`open "${authUrl}"`);
    else if (platform === "win32") execSync(`start "" "${authUrl}"`);
    else execSync(`xdg-open "${authUrl}"`);
  } catch {
    console.log(`Open this URL in your browser:\n${authUrl}\n`);
  }

  // Wait for callback
  const code = await new Promise((resolve, reject) => {
    const connections = new Set();
    const server = http.createServer((req, res) => {
      const url = new URL(req.url, `http://localhost:${REDIRECT_PORT}`);

      if (url.pathname !== "/auth/callback") {
        res.writeHead(404);
        res.end("Not found");
        return;
      }

      const receivedState = url.searchParams.get("state");
      const receivedCode = url.searchParams.get("code");
      const error = url.searchParams.get("error");

      if (error) {
        res.writeHead(200, { "Content-Type": "text/html" });
        res.end("<h2>Authentication failed</h2><p>You can close this tab.</p>");
        clearTimeout(timeout);
        server.close();
        for (const c of connections) c.destroy();
        reject(new Error(`Google auth error: ${error}`));
        return;
      }

      if (receivedState !== state) {
        res.writeHead(200, { "Content-Type": "text/html" });
        res.end("<h2>State mismatch</h2><p>Please try again.</p>");
        clearTimeout(timeout);
        server.close();
        for (const c of connections) c.destroy();
        reject(new Error("State mismatch — possible CSRF attack"));
        return;
      }

      res.writeHead(200, { "Content-Type": "text/html", "Connection": "close" });
      res.end(
        "<h2>Authentication successful!</h2><p>You can close this tab and return to the terminal.</p>"
      );
      clearTimeout(timeout);
      server.close();
      for (const c of connections) c.destroy();
      resolve(receivedCode);
    });

    server.on("connection", (conn) => {
      connections.add(conn);
      conn.on("close", () => connections.delete(conn));
    });

    server.on("error", (err) => {
      if (err.code === "EADDRINUSE") {
        reject(
          new Error(
            `Port ${REDIRECT_PORT} is already in use.\n` +
              `A previous auth session may still be running. Try:\n` +
              `  lsof -ti :${REDIRECT_PORT} | xargs kill\n` +
              `Then run the auth command again.`
          )
        );
      } else {
        reject(new Error(`Server error: ${err.message}`));
      }
    });

    const timeout = setTimeout(() => {
      server.close();
      for (const c of connections) c.destroy();
      reject(new Error("Authentication timed out (5 minutes)"));
    }, 5 * 60 * 1000);

    server.listen(REDIRECT_PORT, "127.0.0.1");
  });

  // Exchange code for tokens
  const tokenResp = await fetch(TOKEN_URL, {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({
      grant_type: "authorization_code",
      code,
      redirect_uri: REDIRECT_URI,
      client_id: clientId,
      client_secret: clientSecret,
      code_verifier: verifier,
    }),
  });

  if (!tokenResp.ok) {
    const text = await tokenResp.text();
    throw new Error(`Token exchange failed: ${tokenResp.status} ${text}`);
  }

  const tokens = await tokenResp.json();

  // Save tokens to config
  config.clientId = clientId;
  config.clientSecret = clientSecret;
  config.accessToken = tokens.access_token;
  config.refreshToken = tokens.refresh_token || config.refreshToken;
  config.tokenExpiry = Date.now() + (tokens.expires_in || 3600) * 1000;
  saveConfig(config);

  console.log("\x1b[32m✓\x1b[0m Google authentication successful!");
  console.log(`\x1b[32m✓\x1b[0m Credentials saved to \x1b[2m${CONFIG_PATH}\x1b[0m`);
  console.log(
    "\n\x1b[2mGoogle Workspace tools (Sheets, Calendar, Docs, Slides, Drive) are now available.\x1b[0m\n"
  );
}

// ── Dispatch ────────────────────────────────────────────────────────────

const commands = {
  auth: authenticate,
};

async function main() {
  const handler = process.argv[2];
  if (!handler || !commands[handler]) {
    process.stderr.write(`Unknown command: ${handler}\n`);
    process.exitCode = 1;
    return;
  }

  try {
    await commands[handler]();
  } catch (err) {
    process.stderr.write(`Error: ${err.message}\n`);
    process.exitCode = 1;
  }
}

main();
