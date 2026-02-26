import { useState } from "react";
import { Camera, ExternalLink, Copy } from "lucide-react";
import { useBrowser } from "../../../hooks/useBrowser";

export function BrowserPanel() {
  const { status, tabs, screenshot, loading, launch, close, navigate, takeScreenshot } =
    useBrowser();
  const [urlInput, setUrlInput] = useState("");

  const handleNavigate = (e: React.FormEvent) => {
    e.preventDefault();
    if (!urlInput.trim()) return;
    let url = urlInput.trim();
    if (!url.startsWith("http://") && !url.startsWith("https://")) {
      url = "https://" + url;
    }
    navigate(url);
    setUrlInput("");
  };

  const handleCopy = () => {
    if (status.currentUrl) {
      navigator.clipboard.writeText(status.currentUrl);
    }
  };

  if (!status.running) {
    return (
      <div style={{ padding: 16, textAlign: "center" }}>
        <div style={{ color: "var(--text-muted)", fontSize: 12, marginBottom: 12 }}>
          Chrome is not running
        </div>
        <button
          onClick={launch}
          disabled={loading}
          style={{
            padding: "6px 16px",
            fontSize: 12,
            border: "1px solid var(--border)",
            borderRadius: 6,
            background: "var(--bg-secondary)",
            color: "var(--text-primary)",
            cursor: loading ? "wait" : "pointer",
          }}
        >
          {loading ? "Launching..." : "Launch Chrome"}
        </button>
      </div>
    );
  }

  return (
    <div>
      {/* Current URL */}
      {status.currentUrl && (
        <div className="browser-url-bar">
          <ExternalLink size={12} style={{ flexShrink: 0 }} />
          <span
            style={{
              flex: 1,
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}
          >
            {status.currentUrl}
          </span>
          <button
            onClick={handleCopy}
            style={{ border: "none", background: "transparent", color: "var(--text-muted)", cursor: "pointer", padding: 0 }}
            title="Copy URL"
          >
            <Copy size={11} />
          </button>
        </div>
      )}

      {/* Navigate input */}
      <form onSubmit={handleNavigate} className="browser-url-bar" style={{ margin: "4px 8px 0" }}>
        <input
          type="text"
          placeholder="Navigate to URL..."
          value={urlInput}
          onChange={(e) => setUrlInput(e.target.value)}
        />
      </form>

      {/* Quick actions */}
      <div style={{ display: "flex", gap: 4, padding: "8px 8px 0" }}>
        <button
          onClick={takeScreenshot}
          style={{
            flex: 1,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            gap: 4,
            padding: "5px 8px",
            fontSize: 11,
            border: "1px solid var(--border)",
            borderRadius: 4,
            background: "var(--bg-secondary)",
            color: "var(--text-secondary)",
            cursor: "pointer",
          }}
        >
          <Camera size={12} /> Screenshot
        </button>
        <button
          onClick={close}
          style={{
            padding: "5px 8px",
            fontSize: 11,
            border: "1px solid var(--border)",
            borderRadius: 4,
            background: "var(--bg-secondary)",
            color: "var(--danger)",
            cursor: "pointer",
          }}
        >
          Close
        </button>
      </div>

      {/* Screenshot preview */}
      {screenshot && (
        <div className="browser-screenshot-preview">
          <img src={screenshot} alt="Browser screenshot" />
        </div>
      )}

      {/* Tabs list */}
      {tabs.length > 0 && (
        <div style={{ padding: "8px 0" }}>
          <div
            style={{
              padding: "4px 12px",
              fontSize: 10,
              color: "var(--text-muted)",
              textTransform: "uppercase",
              letterSpacing: 0.5,
            }}
          >
            Tabs ({tabs.length})
          </div>
          {tabs.map((tab) => (
            <button
              key={tab.id}
              className="file-entry"
              onClick={() => navigate(tab.url)}
            >
              <span className="file-name" style={{ fontSize: 11 }}>
                {tab.title || tab.url}
              </span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
