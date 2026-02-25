//! Browser tools — 9 CDP-backed tools for browser automation.

mod click;
mod evaluate;
mod navigate;
mod new_tab;
mod save_pdf;
mod screenshot;
mod snapshot;
mod switch_tab;
mod tabs;
mod type_text;

use lukan_browser::BrowserManager;
use lukan_core::models::tools::ToolResult;

use crate::ToolRegistry;

/// Register all browser tools into the given registry.
pub fn register_browser_tools(registry: &mut ToolRegistry) {
    registry.register(Box::new(navigate::BrowserNavigate));
    registry.register(Box::new(snapshot::BrowserSnapshot));
    registry.register(Box::new(screenshot::BrowserScreenshot));
    registry.register(Box::new(click::BrowserClick));
    registry.register(Box::new(type_text::BrowserType));
    registry.register(Box::new(evaluate::BrowserEvaluate));
    registry.register(Box::new(save_pdf::BrowserSavePDF));
    registry.register(Box::new(tabs::BrowserTabs));
    registry.register(Box::new(new_tab::BrowserNewTab));
    registry.register(Box::new(switch_tab::BrowserSwitchTab));
}

/// Get the BrowserManager or return an error ToolResult.
fn get_manager() -> Result<std::sync::Arc<BrowserManager>, Box<ToolResult>> {
    BrowserManager::get().ok_or_else(|| {
        Box::new(ToolResult::error(
            "Browser not initialized. Start with --browser or --browser-cdp flag.",
        ))
    })
}

/// Wrap browser content in untrusted_content tags so the LLM treats it as
/// data, not instructions. This is the primary prompt injection defense.
fn wrap_untrusted(content: &str) -> String {
    format!("<untrusted_content source=\"browser\">\n{content}\n</untrusted_content>")
}
