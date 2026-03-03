#!/usr/bin/env node
// Google Workspace tools handler
// Usage: node tools.js <tool_name>
// Input: JSON via stdin
// Output: JSON via stdout { "output": "...", "isError": false }

const fs = require("fs");
const path = require("path");
const { createHash } = require("crypto");

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

function loadLukanConfig() {
  const configDir = path.join(
    process.env.XDG_CONFIG_HOME || path.join(require("os").homedir(), ".config"),
    "lukan"
  );
  try {
    return JSON.parse(fs.readFileSync(path.join(configDir, "config.json"), "utf8"));
  } catch {
    return {};
  }
}

function getTimezone() {
  const lukanConfig = loadLukanConfig();
  return lukanConfig.timezone || Intl.DateTimeFormat().resolvedOptions().timeZone;
}

// ── Auth ────────────────────────────────────────────────────────────────

const TOKEN_URL = "https://oauth2.googleapis.com/token";

async function getAccessToken(config) {
  const clientId = config.clientId;
  const clientSecret = config.clientSecret;
  const accessToken = config.accessToken;
  const refreshToken = config.refreshToken;
  const tokenExpiry = config.tokenExpiry;

  if (!clientId || !clientSecret) {
    throw new Error(
      "Google not configured. Run: lukan google auth"
    );
  }

  if (!accessToken) {
    throw new Error(
      "Google not authenticated. Run: lukan google auth"
    );
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

async function googleGet(url, token) {
  const resp = await fetch(url, {
    headers: { Authorization: `Bearer ${token}` },
  });
  if (!resp.ok) {
    const text = await resp.text();
    throw new Error(`API error ${resp.status}: ${text}`);
  }
  return resp.json();
}

async function googlePost(url, body, token) {
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
    throw new Error(`API error ${resp.status}: ${text}`);
  }
  return resp.json();
}

async function googlePut(url, body, token) {
  const resp = await fetch(url, {
    method: "PUT",
    headers: {
      Authorization: `Bearer ${token}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify(body),
  });
  if (!resp.ok) {
    const text = await resp.text();
    throw new Error(`API error ${resp.status}: ${text}`);
  }
  return resp.json();
}

async function googlePatch(url, body, token) {
  const resp = await fetch(url, {
    method: "PATCH",
    headers: {
      Authorization: `Bearer ${token}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify(body),
  });
  if (!resp.ok) {
    const text = await resp.text();
    throw new Error(`API error ${resp.status}: ${text}`);
  }
  return resp.json();
}

async function googleDelete(url, token) {
  const resp = await fetch(url, {
    method: "DELETE",
    headers: { Authorization: `Bearer ${token}` },
  });
  if (!resp.ok && resp.status !== 204) {
    const text = await resp.text();
    throw new Error(`API error ${resp.status}: ${text}`);
  }
}

async function googleGetBytes(url, token) {
  const resp = await fetch(url, {
    headers: { Authorization: `Bearer ${token}` },
  });
  if (!resp.ok) {
    const text = await resp.text();
    throw new Error(`API error ${resp.status}: ${text}`);
  }
  return Buffer.from(await resp.arrayBuffer());
}

// ── Formatting helpers ──────────────────────────────────────────────────

function parseFormatting(raw) {
  const spans = [];
  let plain = "";
  let i = 0;

  while (i < raw.length) {
    if (raw.startsWith("**", i)) {
      const end = raw.indexOf("**", i + 2);
      if (end !== -1) {
        const text = raw.slice(i + 2, end);
        spans.push({ start: plain.length, end: plain.length + text.length, bold: true });
        plain += text;
        i = end + 2;
        continue;
      }
    }
    if (raw[i] === "*" && (i === 0 || raw[i - 1] !== "*") && i + 1 < raw.length && raw[i + 1] !== "*") {
      const end = raw.indexOf("*", i + 1);
      if (end !== -1 && raw[end - 1] !== "*") {
        const text = raw.slice(i + 1, end);
        spans.push({ start: plain.length, end: plain.length + text.length, italic: true });
        plain += text;
        i = end + 1;
        continue;
      }
    }
    plain += raw[i];
    i++;
  }

  return { plain, spans };
}

function buildFormattedInsertRequests(raw, insertIndex) {
  const { plain, spans } = parseFormatting(raw);
  const requests = [{ insertText: { location: { index: insertIndex }, text: plain } }];

  for (const span of spans) {
    const style = {};
    if (span.bold) style.bold = true;
    if (span.italic) style.italic = true;
    requests.push({
      updateTextStyle: {
        range: {
          startIndex: insertIndex + span.start,
          endIndex: insertIndex + span.end,
        },
        textStyle: style,
        fields: Object.keys(style).join(","),
      },
    });
  }

  return requests;
}

function formatBytes(bytes) {
  if (!bytes) return "";
  const n = parseInt(bytes, 10);
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

// Strip UTC 'Z' suffix or offset from an ISO datetime so Google interprets
// it in the timeZone we provide (i.e. as local time).
function stripTzSuffix(dt) {
  if (!dt) return dt;
  return dt.replace(/Z$/i, "").replace(/[+-]\d{2}:\d{2}$/, "");
}

// ── Slides helpers ──────────────────────────────────────────────────────

const SLIDE_WIDTH_EMU = 9144000;
const SLIDE_HEIGHT_EMU = 5143500;

function hexToRgb(hex) {
  const h = hex.replace(/^#/, "");
  const n = parseInt(h, 16);
  return {
    red: ((n >> 16) & 255) / 255,
    green: ((n >> 8) & 255) / 255,
    blue: (n & 255) / 255,
  };
}

function percentToEmu(pct, totalEmu) {
  return Math.round((pct / 100) * totalEmu);
}

function buildTransform(x, y, width, height) {
  return {
    size: {
      width: { magnitude: percentToEmu(width, SLIDE_WIDTH_EMU), unit: "EMU" },
      height: { magnitude: percentToEmu(height, SLIDE_HEIGHT_EMU), unit: "EMU" },
    },
    transform: {
      scaleX: 1,
      scaleY: 1,
      translateX: percentToEmu(x, SLIDE_WIDTH_EMU),
      translateY: percentToEmu(y, SLIDE_HEIGHT_EMU),
      unit: "EMU",
    },
  };
}

function buildSlideRequests(slideData, slideId, ts) {
  const requests = [];
  const titleId = `title_${ts}`;
  const bodyId = `body_${ts}`;

  // Auto-detect layout
  let layout = slideData.layout;
  if (!layout) {
    if (slideData.title && slideData.body) layout = "TITLE_AND_BODY";
    else if (slideData.title) layout = "TITLE_ONLY";
    else layout = "BLANK";
  }

  const createReq = {
    createSlide: {
      objectId: slideId,
      slideLayoutReference: { predefinedLayout: layout },
    },
  };

  if (layout === "TITLE_AND_BODY") {
    createReq.createSlide.placeholderIdMappings = [
      { layoutPlaceholder: { type: "TITLE", index: 0 }, objectId: titleId },
      { layoutPlaceholder: { type: "BODY", index: 0 }, objectId: bodyId },
    ];
  } else if (layout === "TITLE_ONLY") {
    createReq.createSlide.placeholderIdMappings = [
      { layoutPlaceholder: { type: "TITLE", index: 0 }, objectId: titleId },
    ];
  }

  requests.push(createReq);

  // Insert title text
  if (slideData.title && layout !== "BLANK") {
    requests.push({
      insertText: { objectId: titleId, text: slideData.title, insertionIndex: 0 },
    });
  }

  // Insert body text
  if (slideData.body && layout === "TITLE_AND_BODY") {
    requests.push({
      insertText: { objectId: bodyId, text: slideData.body, insertionIndex: 0 },
    });
  }

  // Background color
  if (slideData.backgroundColor) {
    requests.push({
      updatePageProperties: {
        objectId: slideId,
        pageProperties: {
          pageBackgroundFill: {
            solidFill: { color: { rgbColor: hexToRgb(slideData.backgroundColor) } },
          },
        },
        fields: "pageBackgroundFill.solidFill.color",
      },
    });
  }

  // Background image
  if (slideData.backgroundImageUrl) {
    requests.push({
      updatePageProperties: {
        objectId: slideId,
        pageProperties: {
          pageBackgroundFill: {
            stretchedPictureFill: { contentUrl: slideData.backgroundImageUrl },
          },
        },
        fields: "pageBackgroundFill.stretchedPictureFill",
      },
    });
  }

  // Image element
  if (slideData.imageUrl) {
    const imgX = slideData.imageX ?? 10;
    const imgY = slideData.imageY ?? 10;
    const imgW = slideData.imageWidth ?? 80;
    const imgH = slideData.imageHeight ?? 80;
    requests.push({
      createImage: {
        url: slideData.imageUrl,
        elementProperties: {
          pageObjectId: slideId,
          ...buildTransform(imgX, imgY, imgW, imgH),
        },
      },
    });
  }

  return requests;
}

// ── Tool handlers ───────────────────────────────────────────────────────

const handlers = {
  async SheetsRead(input, token) {
    const { spreadsheetId, range } = input;
    const url = `https://sheets.googleapis.com/v4/spreadsheets/${spreadsheetId}/values/${encodeURIComponent(range)}`;
    const data = await googleGet(url, token);

    const rows = data.values || [];
    if (rows.length === 0) return "No data found in range.";

    return rows.map((r) => r.join("\t")).join("\n");
  },

  async SheetsWrite(input, token) {
    const { spreadsheetId, range, values, append } = input;
    const encoded = encodeURIComponent(range);

    if (append) {
      const url = `https://sheets.googleapis.com/v4/spreadsheets/${spreadsheetId}/values/${encoded}:append?valueInputOption=USER_ENTERED`;
      const data = await googlePost(url, { values }, token);
      const updates = data.updates || {};
      return `Appended ${updates.updatedRows || values.length} rows to ${updates.updatedRange || range}`;
    } else {
      const url = `https://sheets.googleapis.com/v4/spreadsheets/${spreadsheetId}/values/${encoded}?valueInputOption=USER_ENTERED`;
      const data = await googlePut(url, { values }, token);
      return `Updated ${data.updatedRows || values.length} rows in ${data.updatedRange || range}`;
    }
  },

  async SheetsCreate(input, token) {
    const { title, sheetNames } = input;
    const body = { properties: { title } };

    if (sheetNames && sheetNames.length > 0) {
      body.sheets = sheetNames.map((name) => ({ properties: { title: name } }));
    }

    const url = "https://sheets.googleapis.com/v4/spreadsheets";
    const data = await googlePost(url, body, token);
    return `Created spreadsheet: ${data.spreadsheetId}\nURL: ${data.spreadsheetUrl}`;
  },

  async CalendarList(input, token) {
    const calendarId = input.calendarId || "primary";
    const maxResults = input.maxResults || 10;
    const tz = getTimezone();

    // timeMin/timeMax must be full RFC3339 (with offset) for the API
    const timeMin = input.timeMin || new Date().toISOString();
    let url = `https://www.googleapis.com/calendar/v3/calendars/${encodeURIComponent(calendarId)}/events?timeMin=${encodeURIComponent(timeMin)}&maxResults=${maxResults}&singleEvents=true&orderBy=startTime&timeZone=${encodeURIComponent(tz)}`;

    if (input.timeMax) url += `&timeMax=${encodeURIComponent(input.timeMax)}`;
    if (input.query) url += `&q=${encodeURIComponent(input.query)}`;

    const data = await googleGet(url, token);
    const items = data.items || [];

    if (items.length === 0) return `No upcoming events found. (timezone: ${tz})`;

    const lines = items
      .map((e) => {
        const start = e.start?.dateTime || e.start?.date || "?";
        const end = e.end?.dateTime || e.end?.date || "?";
        let line = `[${e.id}] ${e.summary || "(no title)"}\n  ${start} → ${end}`;
        if (e.location) line += `\n  Location: ${e.location}`;
        if (e.attendees?.length) {
          line += `\n  Attendees: ${e.attendees.map((a) => a.email).join(", ")}`;
        }
        return line;
      })
      .join("\n\n");

    return `${lines}\n\n(timezone: ${tz})`;
  },

  async CalendarCreate(input, token) {
    const calendarId = input.calendarId || "primary";
    const tz = getTimezone();
    const body = {
      summary: input.summary,
      start: { dateTime: stripTzSuffix(input.start), timeZone: tz },
      end: { dateTime: stripTzSuffix(input.end), timeZone: tz },
    };

    if (input.description) body.description = input.description;
    if (input.location) body.location = input.location;
    if (input.attendees?.length) {
      body.attendees = input.attendees.map((email) => ({ email }));
    }

    const url = `https://www.googleapis.com/calendar/v3/calendars/${encodeURIComponent(calendarId)}/events`;
    const data = await googlePost(url, body, token);

    const start = data.start?.dateTime || data.start?.date || "?";
    return `Created event: ${data.summary}\nID: ${data.id}\nStart: ${start}\nLink: ${data.htmlLink}`;
  },

  async CalendarUpdate(input, token) {
    const calendarId = input.calendarId || "primary";
    const eventId = input.eventId;
    const encodedCal = encodeURIComponent(calendarId);
    const url = `https://www.googleapis.com/calendar/v3/calendars/${encodedCal}/events/${eventId}`;

    if (input.delete) {
      await googleDelete(url, token);
      return `Deleted event ${eventId}`;
    }

    const body = {};
    if (input.summary) body.summary = input.summary;
    if (input.description) body.description = input.description;
    if (input.location) body.location = input.location;
    const tz = getTimezone();
    if (input.start) body.start = { dateTime: stripTzSuffix(input.start), timeZone: tz };
    if (input.end) body.end = { dateTime: stripTzSuffix(input.end), timeZone: tz };
    if (input.attendees?.length) {
      body.attendees = input.attendees.map((email) => ({ email }));
    }

    const data = await googlePatch(url, body, token);
    const start = data.start?.dateTime || data.start?.date || "?";
    return `Updated event: ${data.summary || data.id}\nStart: ${start}`;
  },

  async DocsRead(input, token) {
    const { documentId } = input;
    const url = `https://docs.googleapis.com/v1/documents/${documentId}`;
    const data = await googleGet(url, token);

    const title = data.title || "(untitled)";
    let text = "";

    const content = data.body?.content || [];
    for (const el of content) {
      if (el.paragraph?.elements) {
        for (const te of el.paragraph.elements) {
          if (te.textRun?.content) text += te.textRun.content;
        }
      }
    }

    const body = text.trim() || "(empty document)";
    return `Title: ${title}\n\n${body}`;
  },

  async DocsCreate(input, token) {
    const { title, content } = input;

    // Create the document
    const createUrl = "https://docs.googleapis.com/v1/documents";
    const doc = await googlePost(createUrl, { title }, token);
    const docId = doc.documentId;

    // Insert content if provided
    if (content) {
      const batchUrl = `https://docs.googleapis.com/v1/documents/${docId}:batchUpdate`;
      const requests = buildFormattedInsertRequests(content, 1);
      try {
        await googlePost(batchUrl, { requests }, token);
      } catch {
        // Content insertion failed, but doc was created
      }
    }

    return `Created document: ${title}\nID: ${docId}\nURL: https://docs.google.com/document/d/${docId}/edit`;
  },

  async DocsUpdate(input, token) {
    const { documentId } = input;
    const batchUrl = `https://docs.googleapis.com/v1/documents/${documentId}:batchUpdate`;
    const actions = [];

    // Find & replace
    if (input.replaceText && input.replacementText !== undefined) {
      await googlePost(batchUrl, {
        requests: [
          {
            replaceAllText: {
              containsText: { text: input.replaceText, matchCase: true },
              replaceText: input.replacementText,
            },
          },
        ],
      }, token);
      actions.push(`Replaced "${input.replaceText}" with "${input.replacementText}"`);
    }

    // Insert at index
    if (input.insertText && input.insertIndex) {
      const requests = buildFormattedInsertRequests(input.insertText, input.insertIndex);
      await googlePost(batchUrl, { requests }, token);
      actions.push(`Inserted text at index ${input.insertIndex}`);
    }

    // Append text
    if (input.appendText) {
      // Get document to find end index
      const docUrl = `https://docs.googleapis.com/v1/documents/${documentId}`;
      const doc = await googleGet(docUrl, token);
      const content = doc.body?.content || [];
      let endIndex = 1;
      if (content.length > 0) {
        const last = content[content.length - 1];
        endIndex = (last.endIndex || 2) - 1;
      }
      const requests = buildFormattedInsertRequests(input.appendText, endIndex);
      await googlePost(batchUrl, { requests }, token);
      actions.push("Appended text");
    }

    if (actions.length === 0) return "No update operations specified.";
    return `Updated document ${documentId}:\n${actions.join("\n")}\nURL: https://docs.google.com/document/d/${documentId}/edit`;
  },

  async SlidesRead(input, token) {
    const { presentationId } = input;
    const url = `https://slides.googleapis.com/v1/presentations/${presentationId}`;
    const data = await googleGet(url, token);

    const title = data.title || "(untitled)";
    const slides = data.slides || [];

    if (slides.length === 0) return `Title: ${title}\n\n(empty presentation)`;

    const lines = [`Title: ${title}`, `Slides: ${slides.length}`, ""];

    for (let i = 0; i < slides.length; i++) {
      const slide = slides[i];
      lines.push(`--- Slide ${i + 1} ---`);

      // Background
      const bg = slide.slideProperties?.pageBackgroundFill;
      if (bg?.solidFill?.color?.rgbColor) {
        const c = bg.solidFill.color.rgbColor;
        const toHex = (v) => Math.round((v || 0) * 255).toString(16).padStart(2, "0");
        lines.push(`Background: #${toHex(c.red)}${toHex(c.green)}${toHex(c.blue)}`);
      } else if (bg?.stretchedPictureFill?.contentUrl) {
        lines.push(`Background image: ${bg.stretchedPictureFill.contentUrl}`);
      }

      // Page elements
      for (const el of slide.pageElements || []) {
        // Text from shapes
        if (el.shape?.text?.textElements) {
          let text = "";
          for (const te of el.shape.text.textElements) {
            if (te.textRun?.content) text += te.textRun.content;
          }
          const trimmed = text.trim();
          if (trimmed) lines.push(trimmed);
        }

        // Images
        if (el.image?.sourceUrl) {
          lines.push(`[Image: ${el.image.sourceUrl}]`);
        } else if (el.image?.contentUrl) {
          lines.push(`[Image: ${el.image.contentUrl}]`);
        }

        // Tables
        if (el.table) {
          const rows = el.table.rows || 0;
          const cols = el.table.columns || 0;
          lines.push(`[Table: ${rows}x${cols}]`);
          for (const row of el.table.tableRows || []) {
            const cells = (row.tableCells || []).map((cell) => {
              let cellText = "";
              for (const te of cell.text?.textElements || []) {
                if (te.textRun?.content) cellText += te.textRun.content;
              }
              return cellText.trim();
            });
            lines.push(`  | ${cells.join(" | ")} |`);
          }
        }
      }

      // Speaker notes
      const notesPage = slide.slideProperties?.notesPage;
      if (notesPage?.pageElements) {
        for (const el of notesPage.pageElements) {
          if (el.shape?.shapeType === "TEXT_BOX" && el.shape?.text?.textElements) {
            let notesText = "";
            for (const te of el.shape.text.textElements) {
              if (te.textRun?.content) notesText += te.textRun.content;
            }
            const trimmed = notesText.trim();
            if (trimmed) lines.push(`Notes: ${trimmed}`);
          }
        }
      }

      lines.push("");
    }

    return lines.join("\n").trim();
  },

  async SlidesCreate(input, token) {
    const { title, slides } = input;
    const baseUrl = "https://slides.googleapis.com/v1/presentations";

    // Create the presentation
    const pres = await googlePost(baseUrl, { title }, token);
    const presId = pres.presentationId;

    if (slides && slides.length > 0) {
      const batchUrl = `${baseUrl}/${presId}:batchUpdate`;

      // Phase 1: Create all slides with content, backgrounds, images
      for (let i = 0; i < slides.length; i++) {
        const slideData = slides[i];
        const ts = `${Date.now()}_${i}`;
        const slideId = `slide_${ts}`;

        const requests = buildSlideRequests(slideData, slideId, ts);
        requests[0].createSlide.insertionIndex = i + 1;
        await googlePost(batchUrl, { requests }, token);
      }

      // Delete the default blank first slide
      const defaultSlideId = pres.slides?.[0]?.objectId;
      if (defaultSlideId) {
        try {
          await googlePost(batchUrl, {
            requests: [{ deleteObject: { objectId: defaultSlideId } }],
          }, token);
        } catch {
          // Default slide may already be gone
        }
      }

      // Phase 2: Add speaker notes (requires fetching created slide objectIds for notesPage)
      const slidesWithNotes = slides
        .map((s, i) => (s.notes ? { index: i, notes: s.notes } : null))
        .filter(Boolean);

      if (slidesWithNotes.length > 0) {
        const updated = await googleGet(`${baseUrl}/${presId}`, token);
        for (const { index, notes } of slidesWithNotes) {
          const slide = updated.slides?.[index];
          if (!slide) continue;
          const notesPage = slide.slideProperties?.notesPage;
          if (!notesPage?.pageElements) continue;
          const notesBox = notesPage.pageElements.find(
            (el) => el.shape?.shapeType === "TEXT_BOX" && el.shape?.placeholder?.type === "BODY"
          );
          if (!notesBox) continue;
          await googlePost(batchUrl, {
            requests: [
              { insertText: { objectId: notesBox.objectId, text: notes, insertionIndex: 0 } },
            ],
          }, token);
        }
      }
    }

    return `Created presentation: ${title}\nID: ${presId}\nSlides: ${slides?.length || 0}\nURL: https://docs.google.com/presentation/d/${presId}/edit`;
  },

  async SlidesUpdate(input, token) {
    const { presentationId } = input;
    const baseUrl = `https://slides.googleapis.com/v1/presentations/${presentationId}`;
    const batchUrl = `${baseUrl}:batchUpdate`;
    const actions = [];

    // Helper to get slide objectId by index
    const getSlideId = async (idx) => {
      const pres = await googleGet(baseUrl, token);
      const slides = pres.slides || [];
      if (idx < 0 || idx >= slides.length) {
        throw new Error(`Slide index ${idx} out of range (0-${slides.length - 1})`);
      }
      return slides[idx].objectId;
    };

    // Replace text across all slides
    if (input.replaceText && input.replacementText !== undefined) {
      await googlePost(batchUrl, {
        requests: [
          {
            replaceAllText: {
              containsText: { text: input.replaceText, matchCase: true },
              replaceText: input.replacementText,
            },
          },
        ],
      }, token);
      actions.push(`Replaced "${input.replaceText}" with "${input.replacementText}"`);
    }

    // Add a new slide (uses buildSlideRequests for full feature support)
    if (input.addSlide) {
      const pres = await googleGet(baseUrl, token);
      const insertionIndex = (pres.slides || []).length;
      const ts = `add_${Date.now()}`;
      const slideId = `slide_${ts}`;

      const requests = buildSlideRequests(input.addSlide, slideId, ts);
      requests[0].createSlide.insertionIndex = insertionIndex;
      await googlePost(batchUrl, { requests }, token);

      // Speaker notes for added slide
      if (input.addSlide.notes) {
        const updated = await googleGet(baseUrl, token);
        const slide = updated.slides?.[insertionIndex];
        const notesBox = slide?.slideProperties?.notesPage?.pageElements?.find(
          (el) => el.shape?.shapeType === "TEXT_BOX" && el.shape?.placeholder?.type === "BODY"
        );
        if (notesBox) {
          await googlePost(batchUrl, {
            requests: [
              { insertText: { objectId: notesBox.objectId, text: input.addSlide.notes, insertionIndex: 0 } },
            ],
          }, token);
        }
      }

      actions.push(`Added slide "${input.addSlide.title || "(untitled)"}"`);
    }

    // Delete a slide by index
    if (input.deleteSlideIndex !== undefined) {
      const slideObjectId = await getSlideId(input.deleteSlideIndex);
      await googlePost(batchUrl, {
        requests: [{ deleteObject: { objectId: slideObjectId } }],
      }, token);
      actions.push(`Deleted slide at index ${input.deleteSlideIndex}`);
    }

    // Insert image on existing slide
    if (input.insertImage) {
      const { slideIndex, imageUrl, x, y, width, height } = input.insertImage;
      const pageId = await getSlideId(slideIndex);
      const imgX = x ?? 10;
      const imgY = y ?? 10;
      const imgW = width ?? 80;
      const imgH = height ?? 80;
      await googlePost(batchUrl, {
        requests: [
          {
            createImage: {
              url: imageUrl,
              elementProperties: {
                pageObjectId: pageId,
                ...buildTransform(imgX, imgY, imgW, imgH),
              },
            },
          },
        ],
      }, token);
      actions.push(`Inserted image on slide ${slideIndex}`);
    }

    // Set slide background
    if (input.setBackground) {
      const { slideIndex, color, imageUrl } = input.setBackground;
      const pageId = await getSlideId(slideIndex);
      const props = {};
      let fields;
      if (imageUrl) {
        props.pageBackgroundFill = {
          stretchedPictureFill: { contentUrl: imageUrl },
        };
        fields = "pageBackgroundFill.stretchedPictureFill";
      } else if (color) {
        props.pageBackgroundFill = {
          solidFill: { color: { rgbColor: hexToRgb(color) } },
        };
        fields = "pageBackgroundFill.solidFill.color";
      }
      if (fields) {
        await googlePost(batchUrl, {
          requests: [{ updatePageProperties: { objectId: pageId, pageProperties: props, fields } }],
        }, token);
        actions.push(`Set background on slide ${slideIndex}`);
      }
    }

    // Add shape to a slide
    if (input.addShape) {
      const { slideIndex, shapeType, x, y, width, height, fillColor, borderColor, text } = input.addShape;
      const pageId = await getSlideId(slideIndex);
      const shapeId = `shape_${Date.now()}`;
      const requests = [
        {
          createShape: {
            objectId: shapeId,
            shapeType: shapeType || "RECTANGLE",
            elementProperties: {
              pageObjectId: pageId,
              ...buildTransform(x, y, width, height),
            },
          },
        },
      ];

      if (fillColor) {
        requests.push({
          updateShapeProperties: {
            objectId: shapeId,
            shapeProperties: {
              shapeBackgroundFill: {
                solidFill: { color: { rgbColor: hexToRgb(fillColor) } },
              },
            },
            fields: "shapeBackgroundFill.solidFill.color",
          },
        });
      }
      if (borderColor) {
        requests.push({
          updateShapeProperties: {
            objectId: shapeId,
            shapeProperties: {
              outline: {
                outlineFill: {
                  solidFill: { color: { rgbColor: hexToRgb(borderColor) } },
                },
              },
            },
            fields: "outline.outlineFill.solidFill.color",
          },
        });
      }
      if (text) {
        requests.push({
          insertText: { objectId: shapeId, text, insertionIndex: 0 },
        });
      }

      await googlePost(batchUrl, { requests }, token);
      actions.push(`Added ${shapeType || "RECTANGLE"} shape on slide ${slideIndex}`);
    }

    // Add table to a slide
    if (input.addTable) {
      const { slideIndex, rows, columns, data, x, y, width, height } = input.addTable;
      const pageId = await getSlideId(slideIndex);
      const tableId = `table_${Date.now()}`;
      const tableReq = {
        createTable: {
          objectId: tableId,
          rows,
          columns,
          elementProperties: { pageObjectId: pageId },
        },
      };

      // Position if specified
      if (x !== undefined && y !== undefined && width !== undefined && height !== undefined) {
        tableReq.createTable.elementProperties = {
          pageObjectId: pageId,
          ...buildTransform(x, y, width, height),
        };
      }

      const requests = [tableReq];

      // Fill cell data (2D array of strings)
      if (data && Array.isArray(data)) {
        for (let r = 0; r < data.length && r < rows; r++) {
          for (let c = 0; c < (data[r]?.length || 0) && c < columns; c++) {
            const cellText = String(data[r][c] ?? "");
            if (cellText) {
              requests.push({
                insertText: {
                  objectId: tableId,
                  cellLocation: { rowIndex: r, columnIndex: c },
                  text: cellText,
                  insertionIndex: 0,
                },
              });
            }
          }
        }
      }

      await googlePost(batchUrl, { requests }, token);
      actions.push(`Added ${rows}x${columns} table on slide ${slideIndex}`);
    }

    // Update text style on a slide
    if (input.updateTextStyle) {
      const { slideIndex, bold, italic, underline, fontFamily, fontSize, color } = input.updateTextStyle;
      const pres = await googleGet(baseUrl, token);
      const slides = pres.slides || [];
      const idx = slideIndex;
      if (idx < 0 || idx >= slides.length) {
        throw new Error(`Slide index ${idx} out of range (0-${slides.length - 1})`);
      }
      const slide = slides[idx];
      const requests = [];

      for (const el of slide.pageElements || []) {
        if (!el.shape?.text?.textElements) continue;
        // Calculate total text length
        let textLen = 0;
        for (const te of el.shape.text.textElements) {
          if (te.textRun?.content) textLen += te.textRun.content.length;
        }
        if (textLen === 0) continue;

        const style = {};
        const fields = [];
        if (bold !== undefined) { style.bold = bold; fields.push("bold"); }
        if (italic !== undefined) { style.italic = italic; fields.push("italic"); }
        if (underline !== undefined) { style.underline = underline; fields.push("underline"); }
        if (fontFamily) { style.fontFamily = fontFamily; fields.push("fontFamily"); }
        if (fontSize) {
          style.fontSize = { magnitude: fontSize, unit: "PT" };
          fields.push("fontSize");
        }
        if (color) {
          style.foregroundColor = { opaqueColor: { rgbColor: hexToRgb(color) } };
          fields.push("foregroundColor");
        }

        if (fields.length > 0) {
          requests.push({
            updateTextStyle: {
              objectId: el.objectId,
              textRange: { type: "ALL" },
              style,
              fields: fields.join(","),
            },
          });
        }
      }

      if (requests.length > 0) {
        await googlePost(batchUrl, { requests }, token);
        actions.push(`Updated text style on slide ${slideIndex}`);
      }
    }

    if (actions.length === 0) return "No update operations specified.";
    return `Updated presentation ${presentationId}:\n${actions.join("\n")}\nURL: https://docs.google.com/presentation/d/${presentationId}/edit`;
  },

  async DriveList(input, token) {
    const maxResults = input.maxResults || 20;
    const parts = ["trashed = false"];

    if (input.query) parts.push(`(${input.query})`);
    if (input.folderId) parts.push(`'${input.folderId}' in parents`);
    if (input.mimeType) parts.push(`mimeType = '${input.mimeType}'`);

    const q = encodeURIComponent(parts.join(" and "));
    const fields = encodeURIComponent("files(id,name,mimeType,modifiedTime,size,webViewLink)");
    const url = `https://www.googleapis.com/drive/v3/files?q=${q}&pageSize=${maxResults}&fields=${fields}&orderBy=modifiedTime desc`;

    const data = await googleGet(url, token);
    const files = data.files || [];

    if (files.length === 0) return "No files found.";

    return files
      .map((f) => {
        let line = `[${f.id}] ${f.name} (${f.mimeType})`;
        if (f.modifiedTime) line += `\n  Modified: ${f.modifiedTime}`;
        if (f.size) line += `  Size: ${formatBytes(f.size)}`;
        if (f.webViewLink) line += `\n  Link: ${f.webViewLink}`;
        return line;
      })
      .join("\n\n");
  },

  async DriveDownload(input, token) {
    const { fileId, outputPath, exportMimeType } = input;

    let url;
    if (exportMimeType) {
      url = `https://www.googleapis.com/drive/v3/files/${fileId}/export?mimeType=${encodeURIComponent(exportMimeType)}`;
    } else {
      url = `https://www.googleapis.com/drive/v3/files/${fileId}?alt=media`;
    }

    const bytes = await googleGetBytes(url, token);
    fs.writeFileSync(outputPath, bytes);
    return `Downloaded ${formatBytes(bytes.length)} to ${outputPath}`;
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
