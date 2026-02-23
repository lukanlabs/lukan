#!/usr/bin/env node
// Gmail tools handler
// Usage: node tools.js <tool_name>
// Input: JSON via stdin
// Output: JSON via stdout { "output": "...", "isError": false }

const fs = require("fs");
const path = require("path");

// ── Config ──────────────────────────────────────────────────────────────

function loadConfig() {
  const configPath = path.join(__dirname, "config.json");
  try {
    return JSON.parse(fs.readFileSync(configPath, "utf8"));
  } catch {
    return {};
  }
}

function saveConfig(config) {
  const configPath = path.join(__dirname, "config.json");
  fs.writeFileSync(configPath, JSON.stringify(config, null, 2));
}

function loadGoogleWorkspaceConfig() {
  const gwPath = path.join(__dirname, "..", "google-workspace", "config.json");
  try {
    return JSON.parse(fs.readFileSync(gwPath, "utf8"));
  } catch {
    return {};
  }
}

// ── Auth ────────────────────────────────────────────────────────────────

const TOKEN_URL = "https://oauth2.googleapis.com/token";

async function getAccessToken(config) {
  // Fallback to google-workspace credentials
  const gwConfig = loadGoogleWorkspaceConfig();
  const clientId = config.clientId || gwConfig.clientId;
  const clientSecret = config.clientSecret || gwConfig.clientSecret;
  const accessToken = config.accessToken;
  const refreshToken = config.refreshToken;
  const tokenExpiry = config.tokenExpiry;

  if (!clientId || !clientSecret) {
    throw new Error("Gmail not configured. Run: lukan gmail auth");
  }

  if (!accessToken) {
    throw new Error("Gmail not authenticated. Run: lukan gmail auth");
  }

  // Check if token needs refresh (within 5 minutes of expiry)
  const now = Date.now();
  if (refreshToken && tokenExpiry && tokenExpiry < now + 5 * 60 * 1000) {
    const resp = await fetch(TOKEN_URL, {
      method: "POST",
      headers: { "Content-Type": "application/x-www-form-urlencoded" },
      body: new URLSearchParams({
        grant_type: "refresh_token",
        refresh_token: refreshToken,
        client_id: clientId,
        client_secret: clientSecret,
      }),
    });

    if (!resp.ok) {
      const text = await resp.text();
      throw new Error(`Token refresh failed: ${resp.status} ${text}`);
    }

    const data = await resp.json();
    config.accessToken = data.access_token;
    if (data.refresh_token) config.refreshToken = data.refresh_token;
    config.tokenExpiry = now + (data.expires_in || 3600) * 1000;
    saveConfig(config);
    return data.access_token;
  }

  return accessToken;
}

// ── HTTP helpers ────────────────────────────────────────────────────────

async function gmailGet(url, token) {
  const resp = await fetch(url, {
    headers: { Authorization: `Bearer ${token}` },
  });
  if (!resp.ok) {
    const text = await resp.text();
    throw new Error(`Gmail API error ${resp.status}: ${text}`);
  }
  return resp.json();
}

async function gmailPost(url, body, token) {
  const resp = await fetch(url, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${token}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify(body),
  });
  if (!resp.ok) {
    const text = await resp.text();
    throw new Error(`Gmail API error ${resp.status}: ${text}`);
  }
  return resp.json();
}

// ── Decoding helpers ────────────────────────────────────────────────────

function base64urlDecode(str) {
  if (!str) return "";
  const padded = str.replace(/-/g, "+").replace(/_/g, "/");
  return Buffer.from(padded, "base64").toString("utf8");
}

function base64urlEncode(str) {
  return Buffer.from(str, "utf8")
    .toString("base64")
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=/g, "");
}

function getHeader(headers, name) {
  if (!headers) return "";
  const h = headers.find((h) => h.name.toLowerCase() === name.toLowerCase());
  return h ? h.value : "";
}

function extractBody(payload) {
  // Direct body data
  if (payload.body?.data) {
    return base64urlDecode(payload.body.data);
  }

  // Multipart — look for text/plain first, then text/html
  if (payload.parts) {
    // Check top-level parts
    const textPart = payload.parts.find((p) => p.mimeType === "text/plain" && p.body?.data);
    if (textPart) return base64urlDecode(textPart.body.data);

    const htmlPart = payload.parts.find((p) => p.mimeType === "text/html" && p.body?.data);
    if (htmlPart) return stripHtml(base64urlDecode(htmlPart.body.data));

    // Check nested multipart/alternative
    for (const part of payload.parts) {
      if (part.parts) {
        const nestedText = part.parts.find((p) => p.mimeType === "text/plain" && p.body?.data);
        if (nestedText) return base64urlDecode(nestedText.body.data);

        const nestedHtml = part.parts.find((p) => p.mimeType === "text/html" && p.body?.data);
        if (nestedHtml) return stripHtml(base64urlDecode(nestedHtml.body.data));
      }
    }
  }

  return "(no body content)";
}

function stripHtml(html) {
  return html
    .replace(/<style[^>]*>[\s\S]*?<\/style>/gi, "")
    .replace(/<script[^>]*>[\s\S]*?<\/script>/gi, "")
    .replace(/<br\s*\/?>/gi, "\n")
    .replace(/<\/p>/gi, "\n\n")
    .replace(/<\/div>/gi, "\n")
    .replace(/<\/li>/gi, "\n")
    .replace(/<li[^>]*>/gi, "- ")
    .replace(/<[^>]+>/g, "")
    .replace(/&nbsp;/g, " ")
    .replace(/&amp;/g, "&")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}

// ── MIME builder ────────────────────────────────────────────────────────

function buildMimeMessage({ to, cc, bcc, subject, body, inReplyTo, references }) {
  const lines = [];
  lines.push(`To: ${to}`);
  if (cc) lines.push(`Cc: ${cc}`);
  if (bcc) lines.push(`Bcc: ${bcc}`);
  lines.push(`Subject: ${subject}`);
  lines.push("MIME-Version: 1.0");
  lines.push("Content-Type: text/plain; charset=UTF-8");
  if (inReplyTo) {
    lines.push(`In-Reply-To: ${inReplyTo}`);
    lines.push(`References: ${references || inReplyTo}`);
  }
  lines.push("");
  lines.push(body);

  return base64urlEncode(lines.join("\r\n"));
}

// ── Tool handlers ───────────────────────────────────────────────────────

const GMAIL_BASE = "https://gmail.googleapis.com/gmail/v1/users/me";

const handlers = {
  async GmailSearch(input, token) {
    const { query, pageToken } = input;
    const maxResults = input.maxResults || 10;

    let url = `${GMAIL_BASE}/messages?q=${encodeURIComponent(query)}&maxResults=${maxResults}`;
    if (pageToken) url += `&pageToken=${encodeURIComponent(pageToken)}`;

    const data = await gmailGet(url, token);
    const messages = data.messages || [];

    if (messages.length === 0) return "No emails found matching the query.";

    // Fetch headers for each message
    const results = [];
    for (const msg of messages) {
      const detail = await gmailGet(
        `${GMAIL_BASE}/messages/${msg.id}?format=metadata&metadataHeaders=From&metadataHeaders=Subject&metadataHeaders=Date`,
        token
      );

      const from = getHeader(detail.payload?.headers, "From");
      const subject = getHeader(detail.payload?.headers, "Subject") || "(no subject)";
      const date = getHeader(detail.payload?.headers, "Date");
      const labels = (detail.labelIds || []).join(", ");

      let line = `[${msg.id}] ${subject}`;
      line += `\n  From: ${from}`;
      line += `\n  Date: ${date}`;
      if (detail.snippet) line += `\n  Preview: ${detail.snippet}`;
      if (labels) line += `\n  Labels: ${labels}`;

      results.push(line);
    }

    let output = results.join("\n\n");
    if (data.nextPageToken) {
      output += `\n\n--- More results available. Use pageToken: "${data.nextPageToken}" ---`;
    }
    return output;
  },

  async GmailRead(input, token) {
    const { messageId } = input;

    const data = await gmailGet(`${GMAIL_BASE}/messages/${messageId}?format=full`, token);

    const from = getHeader(data.payload?.headers, "From");
    const to = getHeader(data.payload?.headers, "To");
    const cc = getHeader(data.payload?.headers, "Cc");
    const subject = getHeader(data.payload?.headers, "Subject") || "(no subject)";
    const date = getHeader(data.payload?.headers, "Date");
    const messageIdHeader = getHeader(data.payload?.headers, "Message-ID") || getHeader(data.payload?.headers, "Message-Id");
    const labels = (data.labelIds || []).join(", ");

    const body = extractBody(data.payload);

    // List attachments
    const attachments = [];
    if (data.payload?.parts) {
      for (const part of data.payload.parts) {
        if (part.filename && part.filename.length > 0) {
          attachments.push(`${part.filename} (${part.mimeType})`);
        }
      }
    }

    let output = `Subject: ${subject}`;
    output += `\nFrom: ${from}`;
    output += `\nTo: ${to}`;
    if (cc) output += `\nCc: ${cc}`;
    output += `\nDate: ${date}`;
    output += `\nMessage-ID: ${messageIdHeader}`;
    output += `\nThread ID: ${data.threadId}`;
    if (labels) output += `\nLabels: ${labels}`;
    if (attachments.length > 0) output += `\nAttachments: ${attachments.join(", ")}`;
    output += `\n\n${body}`;

    return output;
  },

  async GmailSend(input, token) {
    const { to, subject, body, cc, bcc } = input;

    const raw = buildMimeMessage({ to, subject, body, cc, bcc });
    const data = await gmailPost(`${GMAIL_BASE}/messages/send`, { raw }, token);

    return `Email sent successfully.\nMessage ID: ${data.id}\nThread ID: ${data.threadId}`;
  },

  async GmailReply(input, token) {
    const { messageId, body } = input;

    // Fetch original message to get threadId, subject, sender, and Message-ID
    const original = await gmailGet(`${GMAIL_BASE}/messages/${messageId}?format=metadata&metadataHeaders=From&metadataHeaders=Subject&metadataHeaders=Message-ID&metadataHeaders=Message-Id`, token);

    const threadId = original.threadId;
    const originalFrom = getHeader(original.payload?.headers, "From");
    const originalSubject = getHeader(original.payload?.headers, "Subject") || "";
    const originalMessageId = getHeader(original.payload?.headers, "Message-ID") || getHeader(original.payload?.headers, "Message-Id");

    // Build reply subject
    const replySubject = originalSubject.startsWith("Re:") ? originalSubject : `Re: ${originalSubject}`;

    const raw = buildMimeMessage({
      to: originalFrom,
      subject: replySubject,
      body,
      inReplyTo: originalMessageId,
      references: originalMessageId,
    });

    const data = await gmailPost(`${GMAIL_BASE}/messages/send`, { raw, threadId }, token);

    return `Reply sent successfully.\nMessage ID: ${data.id}\nThread ID: ${data.threadId}\nTo: ${originalFrom}`;
  },

  async GmailModify(input, token) {
    const { messageId, action } = input;

    const actionMap = {
      read:       { removeLabelIds: ["UNREAD"] },
      unread:     { addLabelIds: ["UNREAD"] },
      archive:    { removeLabelIds: ["INBOX"] },
      unarchive:  { addLabelIds: ["INBOX"] },
      trash:      { addLabelIds: ["TRASH"] },
      untrash:    { removeLabelIds: ["TRASH"] },
      star:       { addLabelIds: ["STARRED"] },
      unstar:     { removeLabelIds: ["STARRED"] },
    };

    const modification = action ? actionMap[action] : {};
    if (action && !modification) {
      throw new Error(`Unknown action: ${action}. Valid actions: ${Object.keys(actionMap).join(", ")}`);
    }

    // Merge custom label changes
    if (input.addLabelIds) {
      modification.addLabelIds = [...(modification.addLabelIds || []), ...input.addLabelIds];
    }
    if (input.removeLabelIds) {
      modification.removeLabelIds = [...(modification.removeLabelIds || []), ...input.removeLabelIds];
    }

    if (!modification.addLabelIds && !modification.removeLabelIds) {
      throw new Error("No modification specified. Provide an action or addLabelIds/removeLabelIds.");
    }

    await gmailPost(`${GMAIL_BASE}/messages/${messageId}/modify`, modification, token);

    const parts = [];
    if (action) parts.push(action);
    if (input.addLabelIds) parts.push(`+labels: ${input.addLabelIds.join(", ")}`);
    if (input.removeLabelIds) parts.push(`-labels: ${input.removeLabelIds.join(", ")}`);
    return `Message ${messageId} modified: ${parts.join(", ")}`;
  },

  async GmailLabelsList(_input, token) {
    const data = await gmailGet(`${GMAIL_BASE}/labels`, token);
    const labels = data.labels || [];

    if (labels.length === 0) return "No labels found.";

    const system = [];
    const user = [];

    for (const l of labels) {
      const line = `[${l.id}] ${l.name}`;
      if (l.type === "system") system.push(line);
      else user.push(line);
    }

    let output = "";
    if (user.length > 0) {
      output += "User labels:\n" + user.join("\n");
    }
    if (system.length > 0) {
      if (output) output += "\n\n";
      output += "System labels:\n" + system.join("\n");
    }
    return output;
  },

  async GmailLabelsCreate(input, token) {
    const { name } = input;

    const body = {
      name,
      labelListVisibility: `labelShow${input.showInList === "hide" ? "IfCreated" : input.showInList === "showIfUnread" ? "IfUnread" : ""}` .replace("labelShow", "labelShow") || "labelShow",
      messageListVisibility: input.showInMessageList === "hide" ? "hide" : "show",
    };

    // Simpler mapping
    const visMap = { show: "labelShow", hide: "labelHide", showIfUnread: "labelShowIfUnread" };
    body.labelListVisibility = visMap[input.showInList || "show"];
    body.messageListVisibility = input.showInMessageList === "hide" ? "hide" : "show";

    const data = await gmailPost(`${GMAIL_BASE}/labels`, body, token);

    return `Label created: ${data.name}\nID: ${data.id}`;
  },
};

// ── Main ────────────────────────────────────────────────────────────────

async function main() {
  const toolName = process.argv[2];
  if (!toolName) {
    console.log(JSON.stringify({ output: "No tool name provided", isError: true }));
    process.exit(1);
  }

  const handler = handlers[toolName];
  if (!handler) {
    console.log(
      JSON.stringify({ output: `Unknown tool: ${toolName}`, isError: true })
    );
    process.exit(1);
  }

  // Read input from stdin
  let inputData = "";
  for await (const chunk of process.stdin) {
    inputData += chunk;
  }

  let input;
  try {
    input = JSON.parse(inputData);
  } catch {
    console.log(
      JSON.stringify({ output: "Invalid JSON input", isError: true })
    );
    process.exit(1);
  }

  try {
    const config = loadConfig();
    const token = await getAccessToken(config);
    const result = await handler(input, token);
    console.log(JSON.stringify({ output: result, isError: false }));
  } catch (err) {
    console.log(
      JSON.stringify({ output: err.message || String(err), isError: true })
    );
  }
}

main();
