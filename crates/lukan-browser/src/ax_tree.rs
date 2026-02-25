//! Accessibility tree parser — builds a numbered text snapshot from Chrome's AX tree.

use std::collections::HashMap;
use std::fmt::Write;
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::Result;
use serde_json::{Value, json};
use tracing::debug;

use crate::cdp_client::CdpClient;

// ── RefMap — maps numbered refs to DOM nodes ────────────────────────────

/// A reference entry linking a snapshot [ref] number to a DOM node.
#[derive(Debug, Clone)]
pub struct RefEntry {
    pub backend_dom_node_id: i64,
    pub role: String,
    pub name: String,
}

/// Global ref map, reset on each snapshot.
pub type RefMap = Arc<Mutex<HashMap<u32, RefEntry>>>;

fn global_ref_map() -> &'static RefMap {
    static MAP: OnceLock<RefMap> = OnceLock::new();
    MAP.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
}

/// Get the global ref map.
pub fn ref_map() -> RefMap {
    global_ref_map().clone()
}

/// Look up a ref number in the global map.
pub fn resolve_ref(ref_num: u32) -> Option<RefEntry> {
    global_ref_map().lock().ok()?.get(&ref_num).cloned()
}

// ── Interactive / structural roles ─────────────────────────────────────

const INTERACTIVE_ROLES: &[&str] = &[
    "button",
    "link",
    "textbox",
    "searchbox",
    "combobox",
    "checkbox",
    "radio",
    "switch",
    "slider",
    "spinbutton",
    "tab",
    "menuitem",
    "heading",
    "img",
    "progressbar",
];

const STRUCTURAL_ROLES: &[&str] = &[
    "navigation",
    "main",
    "banner",
    "contentinfo",
    "complementary",
    "form",
    "region",
    "dialog",
    "alert",
    "toolbar",
    "tablist",
    "menu",
    "list",
    "tree",
    "group",
];

// ── Text sanitization ─────────────────────────────────────────────────

/// Max chars per individual node name. Legitimate UI elements rarely exceed this.
/// Longer text is likely page body content or injection attempts.
const MAX_NODE_NAME_LEN: usize = 200;

/// Sanitize a node's text: strip zero-width/control chars and truncate.
fn sanitize_name(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| {
            // Keep normal printable chars, newlines, tabs
            if c.is_control() && *c != '\n' && *c != '\t' {
                return false;
            }
            // Strip zero-width and invisible formatting chars
            !matches!(
                *c,
                '\u{200B}' // zero-width space
                | '\u{200C}' // zero-width non-joiner
                | '\u{200D}' // zero-width joiner
                | '\u{200E}' // left-to-right mark
                | '\u{200F}' // right-to-left mark
                | '\u{FEFF}' // BOM / zero-width no-break space
                | '\u{2060}' // word joiner
                | '\u{2061}' // function application
                | '\u{2062}' // invisible times
                | '\u{2063}' // invisible separator
                | '\u{2064}' // invisible plus
                | '\u{00AD}' // soft hyphen
            )
        })
        .collect();

    let trimmed = cleaned.trim();
    if trimmed.len() > MAX_NODE_NAME_LEN {
        format!("{}...", &trimmed[..MAX_NODE_NAME_LEN])
    } else {
        trimmed.to_string()
    }
}

/// Check if a node is explicitly hidden via AX properties.
fn is_hidden_node(node: &Value) -> bool {
    let Some(props) = node.get("properties").and_then(|p| p.as_array()) else {
        return false;
    };
    for prop in props {
        let name = prop.get("name").and_then(|n| n.as_str()).unwrap_or("");
        let val = prop.get("value").and_then(|v| v.get("value"));
        // aria-hidden="true" or hidden attribute
        if name == "hidden"
            && (val.and_then(|v| v.as_bool()) == Some(true)
                || val.and_then(|v| v.as_str()) == Some("true"))
        {
            return true;
        }
    }
    false
}

// ── Snapshot builder ───────────────────────────────────────────────────

const MAX_SNAPSHOT_LEN: usize = 10_000;

/// Fetch the accessibility tree from Chrome and build a text snapshot.
///
/// Interactive elements are numbered `[1], [2], ...` and stored in the RefMap
/// so Click/Type tools can resolve them.
///
/// When `compact` is true, only interactive elements are included (no static
/// text or structural markers), reducing token usage by ~50-70%.
pub async fn get_accessibility_snapshot(cdp: &CdpClient, compact: bool) -> Result<String> {
    // Clear previous refs
    if let Ok(mut map) = global_ref_map().lock() {
        map.clear();
    }

    // Get frame tree (needed for frame context) and full AX tree concurrently
    let (frame_result, ax_result) = tokio::join!(
        cdp.send("Page.getFrameTree", json!({})),
        cdp.send("Accessibility.getFullAXTree", json!({})),
    );

    // Frame tree is best-effort
    let _frame_tree = frame_result.ok();

    let ax_response = ax_result?;
    let nodes = ax_response
        .get("nodes")
        .and_then(|n| n.as_array())
        .cloned()
        .unwrap_or_default();

    debug!(node_count = nodes.len(), "Got AX tree");

    // Parse nodes
    let mut output = String::new();
    let mut ref_counter: u32 = 0;

    for node in &nodes {
        let role = get_ax_property(node, "role")
            .or_else(|| {
                node.get("role")
                    .and_then(|r| r.get("value"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .unwrap_or_default();
        let name = get_ax_name(node)
            .map(|n| sanitize_name(&n))
            .unwrap_or_default();

        // Skip ignored/empty/hidden nodes
        if node
            .get("ignored")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            continue;
        }
        if role == "none" || role == "generic" || role == "InlineTextBox" {
            continue;
        }
        if is_hidden_node(node) {
            continue;
        }

        let role_lower = role.to_lowercase();

        // Interactive element — assign a ref number
        if INTERACTIVE_ROLES.contains(&role_lower.as_str()) {
            ref_counter += 1;
            let ref_num = ref_counter;

            // Store in ref map
            let backend_id = node
                .get("backendDOMNodeId")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);

            if let Ok(mut map) = global_ref_map().lock() {
                map.insert(
                    ref_num,
                    RefEntry {
                        backend_dom_node_id: backend_id,
                        role: role_lower.clone(),
                        name: name.clone(),
                    },
                );
            }

            // Build state description
            let state = build_state_string(node);

            let line = if name.is_empty() {
                format!("[{ref_num}] {role_lower}{state}")
            } else {
                format!("[{ref_num}] {role_lower} \"{name}\"{state}")
            };

            writeln!(output, "{line}").ok();
        }
        // Structural element — context marker (skip in compact mode)
        else if !compact && STRUCTURAL_ROLES.contains(&role_lower.as_str()) {
            if name.is_empty() {
                writeln!(output, "--- {role_lower} ---").ok();
            } else {
                writeln!(output, "--- {role_lower}: {name} ---").ok();
            }
        }
        // Static text (skip in compact mode)
        else if !compact && role_lower == "statictext" && !name.is_empty() {
            writeln!(output, "{name}").ok();
        }

        // Truncate if too long
        if output.len() > MAX_SNAPSHOT_LEN {
            output.truncate(MAX_SNAPSHOT_LEN);
            output.push_str("\n... (snapshot truncated)");
            break;
        }
    }

    if output.is_empty() {
        output = "(empty page — no accessibility content)".to_string();
    }

    let snapshot = format!(
        "<<BROWSER_SNAPSHOT>>\n{}\n<</BROWSER_SNAPSHOT>>",
        output.trim()
    );

    debug!(
        refs = ref_counter,
        len = snapshot.len(),
        "Built accessibility snapshot"
    );

    Ok(snapshot)
}

/// Extract the name from an AX node.
fn get_ax_name(node: &Value) -> Option<String> {
    // Try name.value first
    if let Some(name_val) = node
        .get("name")
        .and_then(|n| n.get("value"))
        .and_then(|v| v.as_str())
    {
        let trimmed = name_val.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    // Try properties
    get_ax_property(node, "name")
}

/// Extract a property value from an AX node's properties array.
fn get_ax_property(node: &Value, prop_name: &str) -> Option<String> {
    let props = node.get("properties")?.as_array()?;
    for prop in props {
        if prop.get("name").and_then(|n| n.as_str()) == Some(prop_name)
            && let Some(val) = prop.get("value").and_then(|v| v.get("value"))
        {
            return match val {
                Value::String(s) => Some(s.clone()),
                Value::Bool(b) => Some(b.to_string()),
                Value::Number(n) => Some(n.to_string()),
                _ => None,
            };
        }
    }
    None
}

/// Build a string describing the state of an AX node (checked, disabled, etc.).
fn build_state_string(node: &Value) -> String {
    let mut parts = Vec::new();

    let props = match node.get("properties").and_then(|p| p.as_array()) {
        Some(p) => p,
        None => return String::new(),
    };

    for prop in props {
        let name = match prop.get("name").and_then(|n| n.as_str()) {
            Some(n) => n,
            None => continue,
        };
        let val = prop.get("value").and_then(|v| v.get("value"));

        match name {
            "checked" => {
                if val.and_then(|v| v.as_str()) == Some("true")
                    || val.and_then(|v| v.as_bool()) == Some(true)
                {
                    parts.push("checked");
                }
            }
            "selected" => {
                if val.and_then(|v| v.as_bool()) == Some(true) {
                    parts.push("selected");
                }
            }
            "expanded" => {
                if val.and_then(|v| v.as_bool()) == Some(true) {
                    parts.push("expanded");
                } else if val.and_then(|v| v.as_bool()) == Some(false) {
                    parts.push("collapsed");
                }
            }
            "disabled" => {
                if val.and_then(|v| v.as_bool()) == Some(true) {
                    parts.push("disabled");
                }
            }
            "required" => {
                if val.and_then(|v| v.as_bool()) == Some(true) {
                    parts.push("required");
                }
            }
            "value" => {
                if let Some(Value::String(s)) = val
                    && !s.is_empty()
                {
                    parts.push("has-value");
                }
            }
            _ => {}
        }
    }

    if parts.is_empty() {
        String::new()
    } else {
        format!(" ({})", parts.join(", "))
    }
}
