//! Google Workspace tools: Sheets, Calendar, Docs, Drive
//! Uses direct REST API calls (no Google SDK dependency).

use async_trait::async_trait;
use lukan_core::config::CredentialsManager;
use lukan_core::models::tools::ToolResult;
use serde_json::{Value, json};
use tracing::debug;

use crate::google_auth::refresh_google_token;
use crate::{Tool, ToolContext};

// ── Auth helper ─────────────────────────────────────────────────────────

/// Get a valid Google access token, auto-refreshing if needed.
async fn get_access_token() -> Result<String, String> {
    let creds = CredentialsManager::load()
        .await
        .map_err(|e| format!("Failed to load credentials: {e}"))?;

    let client_id = creds
        .google_client_id
        .as_deref()
        .ok_or("Google not configured. Run: lukan google-auth")?;
    let client_secret = creds
        .google_client_secret
        .as_deref()
        .ok_or("Google not configured. Run: lukan google-auth")?;

    let access_token = creds
        .google_access_token
        .as_deref()
        .ok_or("Google not authenticated. Run: lukan google-auth")?;

    // Check if token needs refresh (within 5 minutes of expiry)
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    if let (Some(refresh_token), Some(expiry)) =
        (&creds.google_refresh_token, creds.google_token_expiry)
        && expiry < now_ms + 5 * 60 * 1000
        && !refresh_token.is_empty()
    {
        debug!("Google token expired or expiring soon, refreshing");
        let tokens = refresh_google_token(refresh_token, client_id, client_secret)
            .await
            .map_err(|e| format!("Token refresh failed: {e}"))?;

        // Save refreshed tokens
        let mut updated_creds = creds;
        updated_creds.google_access_token = Some(tokens.access_token.clone());
        updated_creds.google_refresh_token = Some(tokens.refresh_token);
        updated_creds.google_token_expiry = Some(tokens.expires_at);
        let _ = CredentialsManager::save(&updated_creds).await;

        return Ok(tokens.access_token);
    }

    Ok(access_token.to_string())
}

/// Make an authenticated GET request to a Google API endpoint.
async fn google_get(url: &str) -> Result<Value, String> {
    let token = get_access_token().await?;
    let client = reqwest::Client::new();
    let resp = client
        .get(url)
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("API error {status}: {text}"));
    }

    resp.json().await.map_err(|e| format!("JSON parse error: {e}"))
}

/// Make an authenticated POST request with JSON body.
async fn google_post(url: &str, body: &Value) -> Result<Value, String> {
    let token = get_access_token().await?;
    let client = reqwest::Client::new();
    let resp = client
        .post(url)
        .bearer_auth(&token)
        .json(body)
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("API error {status}: {text}"));
    }

    resp.json().await.map_err(|e| format!("JSON parse error: {e}"))
}

/// Make an authenticated PUT request with JSON body.
async fn google_put(url: &str, body: &Value) -> Result<Value, String> {
    let token = get_access_token().await?;
    let client = reqwest::Client::new();
    let resp = client
        .put(url)
        .bearer_auth(&token)
        .json(body)
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("API error {status}: {text}"));
    }

    resp.json().await.map_err(|e| format!("JSON parse error: {e}"))
}

/// Make an authenticated PATCH request with JSON body.
async fn google_patch(url: &str, body: &Value) -> Result<Value, String> {
    let token = get_access_token().await?;
    let client = reqwest::Client::new();
    let resp = client
        .patch(url)
        .bearer_auth(&token)
        .json(body)
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("API error {status}: {text}"));
    }

    resp.json().await.map_err(|e| format!("JSON parse error: {e}"))
}

/// Make an authenticated DELETE request.
async fn google_delete(url: &str) -> Result<(), String> {
    let token = get_access_token().await?;
    let client = reqwest::Client::new();
    let resp = client
        .delete(url)
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("API error {status}: {text}"));
    }

    Ok(())
}

/// Download bytes from a Google API endpoint.
async fn google_get_bytes(url: &str) -> Result<Vec<u8>, String> {
    let token = get_access_token().await?;
    let client = reqwest::Client::new();
    let resp = client
        .get(url)
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("API error {status}: {text}"));
    }

    resp.bytes()
        .await
        .map(|b| b.to_vec())
        .map_err(|e| format!("Download error: {e}"))
}

// ============================================================================
// SHEETS TOOLS
// ============================================================================

pub struct SheetsReadTool;

#[async_trait]
impl Tool for SheetsReadTool {
    fn name(&self) -> &str {
        "SheetsRead"
    }
    fn description(&self) -> &str {
        "Read values from a Google Sheets spreadsheet by range (e.g. 'Sheet1!A1:C10')"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "spreadsheetId": { "type": "string", "description": "The spreadsheet ID (from the URL)" },
                "range": { "type": "string", "description": "A1 notation range, e.g. 'Sheet1!A1:C10'" }
            },
            "required": ["spreadsheetId", "range"]
        })
    }
    async fn execute(&self, input: Value, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let id = input["spreadsheetId"].as_str().unwrap_or("");
        let range = input["range"].as_str().unwrap_or("");
        let encoded_range = urlencoding::encode(range);
        let url = format!(
            "https://sheets.googleapis.com/v4/spreadsheets/{id}/values/{encoded_range}"
        );

        match google_get(&url).await {
            Ok(data) => {
                let values = data["values"].as_array();
                match values {
                    Some(rows) if !rows.is_empty() => {
                        let formatted: Vec<String> = rows
                            .iter()
                            .map(|row| {
                                row.as_array()
                                    .map(|cells| {
                                        cells
                                            .iter()
                                            .map(|c| c.as_str().unwrap_or("").to_string())
                                            .collect::<Vec<_>>()
                                            .join("\t")
                                    })
                                    .unwrap_or_default()
                            })
                            .collect();
                        Ok(ToolResult::success(format!(
                            "{} row(s) returned:\n\n{}",
                            rows.len(),
                            formatted.join("\n")
                        )))
                    }
                    _ => Ok(ToolResult::success("No data found in the specified range.")),
                }
            }
            Err(e) => Ok(ToolResult::error(format!("SheetsRead error: {e}"))),
        }
    }
}

pub struct SheetsWriteTool;

#[async_trait]
impl Tool for SheetsWriteTool {
    fn name(&self) -> &str {
        "SheetsWrite"
    }
    fn description(&self) -> &str {
        "Write or append values to a Google Sheets spreadsheet"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "spreadsheetId": { "type": "string", "description": "The spreadsheet ID" },
                "range": { "type": "string", "description": "A1 notation range for where to write" },
                "values": {
                    "type": "array",
                    "items": { "type": "array", "items": {} },
                    "description": "2D array of values to write"
                },
                "append": { "type": "boolean", "description": "If true, append rows after existing data", "default": false }
            },
            "required": ["spreadsheetId", "range", "values"]
        })
    }
    async fn execute(&self, input: Value, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let id = input["spreadsheetId"].as_str().unwrap_or("");
        let range = input["range"].as_str().unwrap_or("");
        let values = &input["values"];
        let append = input["append"].as_bool().unwrap_or(false);
        let encoded_range = urlencoding::encode(range);

        let body = json!({ "values": values });

        if append {
            let url = format!(
                "https://sheets.googleapis.com/v4/spreadsheets/{id}/values/{encoded_range}:append?valueInputOption=USER_ENTERED"
            );
            match google_post(&url, &body).await {
                Ok(data) => {
                    let updated_rows = data["updates"]["updatedRows"]
                        .as_u64()
                        .unwrap_or(0);
                    let updated_range = data["updates"]["updatedRange"]
                        .as_str()
                        .unwrap_or(range);
                    Ok(ToolResult::success(format!(
                        "Appended {updated_rows} row(s) to {updated_range}."
                    )))
                }
                Err(e) => Ok(ToolResult::error(format!("SheetsWrite error: {e}"))),
            }
        } else {
            let url = format!(
                "https://sheets.googleapis.com/v4/spreadsheets/{id}/values/{encoded_range}?valueInputOption=USER_ENTERED"
            );
            match google_put(&url, &body).await {
                Ok(data) => {
                    let updated_rows = data["updatedRows"].as_u64().unwrap_or(0);
                    let updated_range = data["updatedRange"].as_str().unwrap_or(range);
                    Ok(ToolResult::success(format!(
                        "Updated {updated_rows} row(s) in {updated_range}."
                    )))
                }
                Err(e) => Ok(ToolResult::error(format!("SheetsWrite error: {e}"))),
            }
        }
    }
}

pub struct SheetsCreateTool;

#[async_trait]
impl Tool for SheetsCreateTool {
    fn name(&self) -> &str {
        "SheetsCreate"
    }
    fn description(&self) -> &str {
        "Create a new Google Sheets spreadsheet"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "title": { "type": "string", "description": "Title for the new spreadsheet" },
                "sheetNames": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional list of sheet/tab names to create"
                }
            },
            "required": ["title"]
        })
    }
    async fn execute(&self, input: Value, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let title = input["title"].as_str().unwrap_or("Untitled");
        let sheet_names = input["sheetNames"].as_array();

        let mut body = json!({
            "properties": { "title": title }
        });

        if let Some(names) = sheet_names {
            let sheets: Vec<Value> = names
                .iter()
                .map(|n| json!({ "properties": { "title": n.as_str().unwrap_or("Sheet") } }))
                .collect();
            body["sheets"] = Value::Array(sheets);
        }

        let url = "https://sheets.googleapis.com/v4/spreadsheets";
        match google_post(url, &body).await {
            Ok(data) => {
                let ss_id = data["spreadsheetId"].as_str().unwrap_or("");
                let ss_url = data["spreadsheetUrl"].as_str().unwrap_or("");
                Ok(ToolResult::success(format!(
                    "Spreadsheet created.\nID: {ss_id}\nURL: {ss_url}"
                )))
            }
            Err(e) => Ok(ToolResult::error(format!("SheetsCreate error: {e}"))),
        }
    }
}

// ============================================================================
// CALENDAR TOOLS
// ============================================================================

pub struct CalendarListTool;

#[async_trait]
impl Tool for CalendarListTool {
    fn name(&self) -> &str {
        "CalendarList"
    }
    fn description(&self) -> &str {
        "List upcoming events from Google Calendar"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "calendarId": { "type": "string", "description": "Calendar ID (default: 'primary')", "default": "primary" },
                "query": { "type": "string", "description": "Free-text search query to filter events" },
                "timeMin": { "type": "string", "description": "Start of time range (ISO 8601). Defaults to now." },
                "timeMax": { "type": "string", "description": "End of time range (ISO 8601)" },
                "maxResults": { "type": "number", "description": "Max events to return (default: 10)", "default": 10 }
            }
        })
    }
    async fn execute(&self, input: Value, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let calendar_id = input["calendarId"]
            .as_str()
            .unwrap_or("primary");
        let max_results = input["maxResults"].as_u64().unwrap_or(10);
        let time_min = input["timeMin"]
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

        let encoded_cal = urlencoding::encode(calendar_id);
        let encoded_time_min = urlencoding::encode(&time_min);
        let mut url = format!(
            "https://www.googleapis.com/calendar/v3/calendars/{encoded_cal}/events?timeMin={encoded_time_min}&maxResults={max_results}&singleEvents=true&orderBy=startTime"
        );

        if let Some(time_max) = input["timeMax"].as_str() {
            url.push_str(&format!("&timeMax={}", urlencoding::encode(time_max)));
        }
        if let Some(query) = input["query"].as_str() {
            url.push_str(&format!("&q={}", urlencoding::encode(query)));
        }

        match google_get(&url).await {
            Ok(data) => {
                let items = data["items"].as_array();
                match items {
                    Some(events) if !events.is_empty() => {
                        let lines: Vec<String> = events
                            .iter()
                            .map(|e| {
                                let id = e["id"].as_str().unwrap_or("?");
                                let summary = e["summary"].as_str().unwrap_or("(no title)");
                                let start = e["start"]["dateTime"]
                                    .as_str()
                                    .or_else(|| e["start"]["date"].as_str())
                                    .unwrap_or("?");
                                let end = e["end"]["dateTime"]
                                    .as_str()
                                    .or_else(|| e["end"]["date"].as_str())
                                    .unwrap_or("");
                                let location = e["location"]
                                    .as_str()
                                    .map(|l| format!("\n  Location: {l}"))
                                    .unwrap_or_default();
                                let attendees = e["attendees"]
                                    .as_array()
                                    .map(|a| {
                                        let emails: Vec<&str> = a
                                            .iter()
                                            .filter_map(|att| att["email"].as_str())
                                            .collect();
                                        if emails.is_empty() {
                                            String::new()
                                        } else {
                                            format!("\n  Attendees: {}", emails.join(", "))
                                        }
                                    })
                                    .unwrap_or_default();
                                format!("[{id}] {summary}\n  {start} → {end}{location}{attendees}")
                            })
                            .collect();
                        Ok(ToolResult::success(format!(
                            "{} event(s):\n\n{}",
                            events.len(),
                            lines.join("\n\n")
                        )))
                    }
                    _ => Ok(ToolResult::success("No upcoming events found.")),
                }
            }
            Err(e) => Ok(ToolResult::error(format!("CalendarList error: {e}"))),
        }
    }
}

pub struct CalendarCreateTool;

#[async_trait]
impl Tool for CalendarCreateTool {
    fn name(&self) -> &str {
        "CalendarCreate"
    }
    fn description(&self) -> &str {
        "Create a new Google Calendar event"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "summary": { "type": "string", "description": "Event title" },
                "start": { "type": "string", "description": "Start time (ISO 8601)" },
                "end": { "type": "string", "description": "End time (ISO 8601)" },
                "description": { "type": "string", "description": "Event description" },
                "location": { "type": "string", "description": "Event location" },
                "attendees": { "type": "array", "items": { "type": "string" }, "description": "Attendee email addresses" },
                "calendarId": { "type": "string", "description": "Calendar ID (default: 'primary')", "default": "primary" }
            },
            "required": ["summary", "start", "end"]
        })
    }
    async fn execute(&self, input: Value, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let calendar_id = input["calendarId"]
            .as_str()
            .unwrap_or("primary");
        let encoded_cal = urlencoding::encode(calendar_id);
        let url = format!(
            "https://www.googleapis.com/calendar/v3/calendars/{encoded_cal}/events"
        );

        let mut body = json!({
            "summary": input["summary"],
            "start": { "dateTime": input["start"] },
            "end": { "dateTime": input["end"] }
        });

        if let Some(desc) = input["description"].as_str() {
            body["description"] = json!(desc);
        }
        if let Some(loc) = input["location"].as_str() {
            body["location"] = json!(loc);
        }
        if let Some(attendees) = input["attendees"].as_array() {
            let att: Vec<Value> = attendees
                .iter()
                .filter_map(|a| a.as_str())
                .map(|email| json!({ "email": email }))
                .collect();
            body["attendees"] = Value::Array(att);
        }

        match google_post(&url, &body).await {
            Ok(data) => {
                let event_id = data["id"].as_str().unwrap_or("?");
                let summary = data["summary"].as_str().unwrap_or("");
                let start = data["start"]["dateTime"]
                    .as_str()
                    .or_else(|| data["start"]["date"].as_str())
                    .unwrap_or("?");
                let link = data["htmlLink"].as_str().unwrap_or("");
                Ok(ToolResult::success(format!(
                    "Event created.\nID: {event_id}\nSummary: {summary}\nStart: {start}\nLink: {link}"
                )))
            }
            Err(e) => Ok(ToolResult::error(format!("CalendarCreate error: {e}"))),
        }
    }
}

pub struct CalendarUpdateTool;

#[async_trait]
impl Tool for CalendarUpdateTool {
    fn name(&self) -> &str {
        "CalendarUpdate"
    }
    fn description(&self) -> &str {
        "Update or delete a Google Calendar event"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "eventId": { "type": "string", "description": "The event ID to update or delete" },
                "calendarId": { "type": "string", "description": "Calendar ID (default: 'primary')", "default": "primary" },
                "delete": { "type": "boolean", "description": "Set true to delete the event", "default": false },
                "summary": { "type": "string", "description": "New event title" },
                "start": { "type": "string", "description": "New start time (ISO 8601)" },
                "end": { "type": "string", "description": "New end time (ISO 8601)" },
                "description": { "type": "string", "description": "New event description" },
                "location": { "type": "string", "description": "New event location" },
                "attendees": { "type": "array", "items": { "type": "string" }, "description": "New attendee emails (replaces existing)" }
            },
            "required": ["eventId"]
        })
    }
    async fn execute(&self, input: Value, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let event_id = input["eventId"].as_str().unwrap_or("");
        let calendar_id = input["calendarId"]
            .as_str()
            .unwrap_or("primary");
        let delete = input["delete"].as_bool().unwrap_or(false);
        let encoded_cal = urlencoding::encode(calendar_id);

        if delete {
            let url = format!(
                "https://www.googleapis.com/calendar/v3/calendars/{encoded_cal}/events/{event_id}"
            );
            match google_delete(&url).await {
                Ok(()) => Ok(ToolResult::success(format!("Event {event_id} deleted."))),
                Err(e) => Ok(ToolResult::error(format!("CalendarUpdate error: {e}"))),
            }
        } else {
            let mut patch = json!({});
            if let Some(s) = input["summary"].as_str() {
                patch["summary"] = json!(s);
            }
            if let Some(s) = input["description"].as_str() {
                patch["description"] = json!(s);
            }
            if let Some(s) = input["location"].as_str() {
                patch["location"] = json!(s);
            }
            if let Some(s) = input["start"].as_str() {
                patch["start"] = json!({ "dateTime": s });
            }
            if let Some(s) = input["end"].as_str() {
                patch["end"] = json!({ "dateTime": s });
            }
            if let Some(attendees) = input["attendees"].as_array() {
                let att: Vec<Value> = attendees
                    .iter()
                    .filter_map(|a| a.as_str())
                    .map(|email| json!({ "email": email }))
                    .collect();
                patch["attendees"] = Value::Array(att);
            }

            let url = format!(
                "https://www.googleapis.com/calendar/v3/calendars/{encoded_cal}/events/{event_id}"
            );
            match google_patch(&url, &patch).await {
                Ok(data) => {
                    let eid = data["id"].as_str().unwrap_or(event_id);
                    let summary = data["summary"].as_str().unwrap_or("");
                    let start = data["start"]["dateTime"]
                        .as_str()
                        .or_else(|| data["start"]["date"].as_str())
                        .unwrap_or("?");
                    Ok(ToolResult::success(format!(
                        "Event updated.\nID: {eid}\nSummary: {summary}\nStart: {start}"
                    )))
                }
                Err(e) => Ok(ToolResult::error(format!("CalendarUpdate error: {e}"))),
            }
        }
    }
}

// ============================================================================
// DOCS TOOLS
// ============================================================================

pub struct DocsReadTool;

#[async_trait]
impl Tool for DocsReadTool {
    fn name(&self) -> &str {
        "DocsRead"
    }
    fn description(&self) -> &str {
        "Read a Google Doc's content as plain text"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "documentId": { "type": "string", "description": "The Google Doc ID (from the URL)" }
            },
            "required": ["documentId"]
        })
    }
    async fn execute(&self, input: Value, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let doc_id = input["documentId"].as_str().unwrap_or("");
        let url = format!("https://docs.googleapis.com/v1/documents/{doc_id}");

        match google_get(&url).await {
            Ok(data) => {
                let title = data["title"].as_str().unwrap_or("Untitled");
                let mut text = String::new();
                if let Some(content) = data["body"]["content"].as_array() {
                    for element in content {
                        if let Some(paragraph) = element.get("paragraph")
                            && let Some(elements) = paragraph["elements"].as_array()
                        {
                            for el in elements {
                                if let Some(content) =
                                    el["textRun"]["content"].as_str()
                                {
                                    text.push_str(content);
                                }
                            }
                        }
                    }
                }
                let trimmed = text.trim();
                let body = if trimmed.is_empty() {
                    "(empty document)"
                } else {
                    trimmed
                };
                Ok(ToolResult::success(format!("Title: {title}\n\n{body}")))
            }
            Err(e) => Ok(ToolResult::error(format!("DocsRead error: {e}"))),
        }
    }
}

pub struct DocsCreateTool;

#[async_trait]
impl Tool for DocsCreateTool {
    fn name(&self) -> &str {
        "DocsCreate"
    }
    fn description(&self) -> &str {
        "Create a new Google Doc with optional initial content"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "title": { "type": "string", "description": "Title for the new document" },
                "content": { "type": "string", "description": "Optional initial text content (supports **bold** and *italic*)" }
            },
            "required": ["title"]
        })
    }
    async fn execute(&self, input: Value, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let title = input["title"].as_str().unwrap_or("Untitled");

        let body = json!({ "title": title });
        let url = "https://docs.googleapis.com/v1/documents";

        match google_post(url, &body).await {
            Ok(data) => {
                let doc_id = data["documentId"].as_str().unwrap_or("");

                // Insert content if provided
                if let Some(content) = input["content"].as_str()
                    && !content.is_empty()
                {
                    let requests = build_formatted_insert_requests(content, 1);
                    let batch_url = format!(
                        "https://docs.googleapis.com/v1/documents/{doc_id}:batchUpdate"
                    );
                    let batch_body = json!({ "requests": requests });
                    let _ = google_post(&batch_url, &batch_body).await;
                }

                Ok(ToolResult::success(format!(
                    "Document created.\nID: {doc_id}\nTitle: {title}\nURL: https://docs.google.com/document/d/{doc_id}/edit"
                )))
            }
            Err(e) => Ok(ToolResult::error(format!("DocsCreate error: {e}"))),
        }
    }
}

pub struct DocsUpdateTool;

#[async_trait]
impl Tool for DocsUpdateTool {
    fn name(&self) -> &str {
        "DocsUpdate"
    }
    fn description(&self) -> &str {
        "Update an existing Google Doc (append, find-and-replace, or insert at position)"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "documentId": { "type": "string", "description": "The Google Doc ID to update" },
                "appendText": { "type": "string", "description": "Text to append at end of document (supports **bold** and *italic*)" },
                "replaceText": { "type": "string", "description": "Text to find in the document" },
                "replacementText": { "type": "string", "description": "Text to replace it with" },
                "insertText": { "type": "string", "description": "Text to insert at a specific position (supports **bold** and *italic*)" },
                "insertIndex": { "type": "number", "description": "Character index where to insert (1-based)" }
            },
            "required": ["documentId"]
        })
    }
    async fn execute(&self, input: Value, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let doc_id = input["documentId"].as_str().unwrap_or("");
        let mut requests: Vec<Value> = Vec::new();
        let mut actions: Vec<&str> = Vec::new();

        // Find and replace
        if let (Some(find), Some(replace)) = (
            input["replaceText"].as_str(),
            input["replacementText"].as_str(),
        ) {
            requests.push(json!({
                "replaceAllText": {
                    "containsText": { "text": find, "matchCase": true },
                    "replaceText": replace
                }
            }));
            actions.push("replaced text");
        }

        // Insert at index
        if let (Some(text), Some(index)) = (
            input["insertText"].as_str(),
            input["insertIndex"].as_u64(),
        ) {
            requests.extend(build_formatted_insert_requests(text, index as usize));
            actions.push("inserted text");
        }

        // Append text
        if let Some(append_text) = input["appendText"].as_str() {
            // Get doc to find end index
            let doc_url = format!("https://docs.googleapis.com/v1/documents/{doc_id}");
            match google_get(&doc_url).await {
                Ok(doc) => {
                    let content = doc["body"]["content"].as_array();
                    let end_index = content
                        .and_then(|c| c.last())
                        .and_then(|e| e["endIndex"].as_u64())
                        .map(|i| (i - 1) as usize)
                        .unwrap_or(1);
                    requests.extend(build_formatted_insert_requests(append_text, end_index));
                    actions.push("appended text");
                }
                Err(e) => return Ok(ToolResult::error(format!("DocsUpdate error: {e}"))),
            }
        }

        if requests.is_empty() {
            return Ok(ToolResult::error(
                "No update action specified. Use appendText, replaceText+replacementText, or insertText+insertIndex.",
            ));
        }

        let batch_url = format!(
            "https://docs.googleapis.com/v1/documents/{doc_id}:batchUpdate"
        );
        let batch_body = json!({ "requests": requests });

        match google_post(&batch_url, &batch_body).await {
            Ok(_) => Ok(ToolResult::success(format!(
                "Document updated ({}).\nID: {doc_id}\nURL: https://docs.google.com/document/d/{doc_id}/edit",
                actions.join(", ")
            ))),
            Err(e) => Ok(ToolResult::error(format!("DocsUpdate error: {e}"))),
        }
    }
}

// ============================================================================
// DRIVE TOOLS
// ============================================================================

pub struct DriveListTool;

#[async_trait]
impl Tool for DriveListTool {
    fn name(&self) -> &str {
        "DriveList"
    }
    fn description(&self) -> &str {
        "List files in Google Drive with optional filtering"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Drive search query (e.g. \"name contains 'report'\")" },
                "folderId": { "type": "string", "description": "Folder ID to list contents of" },
                "mimeType": { "type": "string", "description": "Filter by MIME type" },
                "maxResults": { "type": "number", "description": "Max files to return (default: 20)", "default": 20 }
            }
        })
    }
    async fn execute(&self, input: Value, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let max_results = input["maxResults"].as_u64().unwrap_or(20);
        let mut query_parts = vec!["trashed = false".to_string()];

        if let Some(q) = input["query"].as_str() {
            query_parts.push(format!("({q})"));
        }
        if let Some(folder) = input["folderId"].as_str() {
            query_parts.push(format!("'{folder}' in parents"));
        }
        if let Some(mime) = input["mimeType"].as_str() {
            query_parts.push(format!("mimeType = '{mime}'"));
        }

        let q = query_parts.join(" and ");
        let encoded_q = urlencoding::encode(&q);
        let url = format!(
            "https://www.googleapis.com/drive/v3/files?q={encoded_q}&pageSize={max_results}&fields=files(id,name,mimeType,modifiedTime,size,webViewLink)&orderBy=modifiedTime desc"
        );

        match google_get(&url).await {
            Ok(data) => {
                let files = data["files"].as_array();
                match files {
                    Some(files) if !files.is_empty() => {
                        let lines: Vec<String> = files
                            .iter()
                            .map(|f| {
                                let id = f["id"].as_str().unwrap_or("?");
                                let name = f["name"].as_str().unwrap_or("?");
                                let mime = f["mimeType"].as_str().unwrap_or("?");
                                let modified = f["modifiedTime"].as_str().unwrap_or("?");
                                let size = f["size"]
                                    .as_str()
                                    .and_then(|s| s.parse::<u64>().ok())
                                    .map(|b| format!(" ({})", format_bytes(b)))
                                    .unwrap_or_default();
                                format!("[{id}] {name}{size}\n  Type: {mime}\n  Modified: {modified}")
                            })
                            .collect();
                        Ok(ToolResult::success(format!(
                            "{} file(s):\n\n{}",
                            files.len(),
                            lines.join("\n\n")
                        )))
                    }
                    _ => Ok(ToolResult::success("No files found.")),
                }
            }
            Err(e) => Ok(ToolResult::error(format!("DriveList error: {e}"))),
        }
    }
}

pub struct DriveDownloadTool;

#[async_trait]
impl Tool for DriveDownloadTool {
    fn name(&self) -> &str {
        "DriveDownload"
    }
    fn description(&self) -> &str {
        "Download or export a file from Google Drive"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "fileId": { "type": "string", "description": "The file ID to download" },
                "outputPath": { "type": "string", "description": "Local path to save the file" },
                "exportMimeType": { "type": "string", "description": "MIME type to export Google Docs/Sheets/Slides to (e.g. 'text/plain', 'application/pdf', 'text/csv')" }
            },
            "required": ["fileId", "outputPath"]
        })
    }
    async fn execute(&self, input: Value, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let file_id = input["fileId"].as_str().unwrap_or("");
        let output_path = input["outputPath"].as_str().unwrap_or("");

        let url = if let Some(mime) = input["exportMimeType"].as_str() {
            let encoded_mime = urlencoding::encode(mime);
            format!(
                "https://www.googleapis.com/drive/v3/files/{file_id}/export?mimeType={encoded_mime}"
            )
        } else {
            format!("https://www.googleapis.com/drive/v3/files/{file_id}?alt=media")
        };

        match google_get_bytes(&url).await {
            Ok(bytes) => {
                let size = bytes.len();
                tokio::fs::write(output_path, &bytes)
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to write file: {e}"))?;
                Ok(ToolResult::success(format!(
                    "File downloaded to {output_path} ({}).",
                    format_bytes(size as u64)
                )))
            }
            Err(e) => Ok(ToolResult::error(format!("DriveDownload error: {e}"))),
        }
    }
}

// ── Formatting helpers ──────────────────────────────────────────────────

struct TextSpan {
    start: usize,
    end: usize,
    bold: bool,
    italic: bool,
}

/// Parse **bold** and *italic* from text, returning plain text and spans.
fn parse_formatting(raw: &str) -> (String, Vec<TextSpan>) {
    let mut spans = Vec::new();
    let mut text = String::new();
    let chars: Vec<char> = raw.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        // **bold**
        if i + 1 < chars.len()
            && chars[i] == '*'
            && chars[i + 1] == '*'
            && let Some(close) = raw[i + 2..].find("**")
        {
            let inner = &raw[i + 2..i + 2 + close];
            let start = text.len();
            text.push_str(inner);
            spans.push(TextSpan {
                start,
                end: text.len(),
                bold: true,
                italic: false,
            });
            i += 2 + close + 2;
            continue;
        }
        // *italic* (single asterisk, not followed by another)
        if chars[i] == '*'
            && (i + 1 >= chars.len() || chars[i + 1] != '*')
            && let Some(close) = raw[i + 1..].find('*')
        {
            // Make sure close is not **
            if i + 1 + close + 1 >= chars.len() || chars[i + 1 + close + 1] != '*' {
                let inner = &raw[i + 1..i + 1 + close];
                let start = text.len();
                text.push_str(inner);
                spans.push(TextSpan {
                    start,
                    end: text.len(),
                    bold: false,
                    italic: true,
                });
                i += 1 + close + 1;
                continue;
            }
        }
        text.push(chars[i]);
        i += 1;
    }

    (text, spans)
}

/// Build Google Docs API requests for inserting formatted text at a given index.
fn build_formatted_insert_requests(raw: &str, insert_index: usize) -> Vec<Value> {
    let (text, spans) = parse_formatting(raw);
    let mut requests = Vec::new();

    requests.push(json!({
        "insertText": {
            "location": { "index": insert_index },
            "text": text
        }
    }));

    for span in &spans {
        let mut fields = Vec::new();
        let mut text_style = json!({});
        if span.bold {
            text_style["bold"] = json!(true);
            fields.push("bold");
        }
        if span.italic {
            text_style["italic"] = json!(true);
            fields.push("italic");
        }
        requests.push(json!({
            "updateTextStyle": {
                "range": {
                    "startIndex": insert_index + span.start,
                    "endIndex": insert_index + span.end
                },
                "textStyle": text_style,
                "fields": fields.join(",")
            }
        }));
    }

    requests
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    if bytes < 1024 * 1024 {
        return format!("{:.1} KB", bytes as f64 / 1024.0);
    }
    format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
}
