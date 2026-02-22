//! Google OAuth2 PKCE browser flow for Google Workspace API authentication.

use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::Rng;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tracing::info;

const REDIRECT_PORT: u16 = 1456;
const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

const SCOPES: &str = "https://www.googleapis.com/auth/spreadsheets \
                       https://www.googleapis.com/auth/calendar \
                       https://www.googleapis.com/auth/documents \
                       https://www.googleapis.com/auth/drive";

/// Tokens returned from Google OAuth2
pub struct GoogleTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: u64,
}

// ── PKCE helpers ────────────────────────────────────────────────────────

fn generate_random_base64url(len: usize) -> String {
    let bytes: Vec<u8> = (0..len).map(|_| rand::rng().random::<u8>()).collect();
    URL_SAFE_NO_PAD.encode(&bytes)
}

fn generate_pkce() -> (String, String) {
    let verifier = generate_random_base64url(32);
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let challenge = URL_SAFE_NO_PAD.encode(hasher.finalize());
    (verifier, challenge)
}

fn generate_state() -> String {
    generate_random_base64url(32)
}

// ── Token exchange ──────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
}

async fn exchange_code_for_tokens(
    code: &str,
    verifier: &str,
    client_id: &str,
    client_secret: &str,
) -> Result<GoogleTokens> {
    let redirect_uri = format!("http://localhost:{REDIRECT_PORT}/auth/callback");
    let params = [
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", &redirect_uri),
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("code_verifier", verifier),
    ];

    let client = reqwest::Client::new();
    let resp = client
        .post(TOKEN_URL)
        .form(&params)
        .send()
        .await
        .context("Failed to exchange authorization code")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        bail!("Token exchange failed: {status}\n{text}");
    }

    let data: TokenResponse = resp.json().await.context("Failed to parse token response")?;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    Ok(GoogleTokens {
        access_token: data.access_token,
        refresh_token: data.refresh_token.unwrap_or_default(),
        expires_at: now_ms + (data.expires_in.unwrap_or(3600)) * 1000,
    })
}

// ── Token refresh ───────────────────────────────────────────────────────

pub async fn refresh_google_token(
    refresh_token: &str,
    client_id: &str,
    client_secret: &str,
) -> Result<GoogleTokens> {
    let params = [
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", client_id),
        ("client_secret", client_secret),
    ];

    let client = reqwest::Client::new();
    let resp = client
        .post(TOKEN_URL)
        .form(&params)
        .send()
        .await
        .context("Failed to refresh token")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        bail!("Token refresh failed: {status}\n{text}");
    }

    let data: TokenResponse = resp.json().await.context("Failed to parse refresh response")?;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    Ok(GoogleTokens {
        access_token: data.access_token,
        refresh_token: data
            .refresh_token
            .unwrap_or_else(|| refresh_token.to_string()),
        expires_at: now_ms + (data.expires_in.unwrap_or(3600)) * 1000,
    })
}

// ── Browser OAuth flow ──────────────────────────────────────────────────

/// Run the full OAuth2 PKCE flow: starts a local server, opens browser, waits for callback.
pub async fn authenticate_google(
    client_id: &str,
    client_secret: &str,
) -> Result<GoogleTokens> {
    let (verifier, challenge) = generate_pkce();
    let state = generate_state();
    let redirect_uri = format!("http://localhost:{REDIRECT_PORT}/auth/callback");

    let auth_url = format!(
        "{AUTH_URL}?response_type=code&client_id={}&redirect_uri={}&scope={}&code_challenge={}&code_challenge_method=S256&state={}&access_type=offline&prompt=consent",
        urlencoding::encode(client_id),
        urlencoding::encode(&redirect_uri),
        urlencoding::encode(SCOPES),
        urlencoding::encode(&challenge),
        urlencoding::encode(&state),
    );

    println!("\x1b[1m\x1b[33m━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\x1b[0m");
    println!("\x1b[1mPlease sign in with your Google account:\x1b[0m\n");
    println!("  Opening browser...\n");
    println!("\x1b[90mIf browser doesn't open, visit:\x1b[0m");
    println!("  \x1b[36m{auth_url}\x1b[0m\n");
    println!("\x1b[90mWaiting for authorization...\x1b[0m");
    println!("\x1b[1m\x1b[33m━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\x1b[0m\n");

    // Try to open browser
    let _ = open::that(&auth_url);

    // Start callback server
    let code = wait_for_callback(&state).await?;

    info!("Exchanging authorization code for tokens...");
    println!("Exchanging authorization code for tokens...");

    exchange_code_for_tokens(&code, &verifier, client_id, client_secret).await
}

/// Start a local TCP server and wait for the OAuth callback.
async fn wait_for_callback(expected_state: &str) -> Result<String> {
    let listener = TcpListener::bind(format!("127.0.0.1:{REDIRECT_PORT}"))
        .await
        .context(format!(
            "Failed to bind to port {REDIRECT_PORT}. Is another process using it?"
        ))?;

    let timeout = tokio::time::timeout(std::time::Duration::from_secs(300), async {
        loop {
            let (mut stream, _) = listener.accept().await?;

            // Read the HTTP request
            let mut buf = vec![0u8; 4096];
            let n = tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await?;
            let request = String::from_utf8_lossy(&buf[..n]);

            // Parse the request line to get the path
            let first_line = request.lines().next().unwrap_or("");
            let path = first_line.split_whitespace().nth(1).unwrap_or("/");

            if !path.starts_with("/auth/callback") {
                let response = "HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\n\r\nNot found";
                stream.write_all(response.as_bytes()).await?;
                continue;
            }

            // Parse query parameters
            let query = path.split('?').nth(1).unwrap_or("");
            let params: std::collections::HashMap<&str, &str> = query
                .split('&')
                .filter_map(|p| {
                    let mut kv = p.splitn(2, '=');
                    Some((kv.next()?, kv.next()?))
                })
                .collect();

            // Check for error
            if let Some(error) = params.get("error") {
                let desc = params.get("error_description").unwrap_or(&"");
                let body = format!("Authentication failed: {error} {desc}");
                let response = format!(
                    "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream.write_all(response.as_bytes()).await?;
                return Err(anyhow::anyhow!("Google OAuth error: {error} — {desc}"));
            }

            // Validate state
            let received_state = params.get("state").unwrap_or(&"");
            if *received_state != expected_state {
                let body = "State mismatch — possible CSRF attack";
                let response = format!(
                    "HTTP/1.1 400 Bad Request\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream.write_all(response.as_bytes()).await?;
                return Err(anyhow::anyhow!("OAuth state mismatch"));
            }

            // Extract code
            if let Some(code) = params.get("code") {
                let body = "<html><body><h2>Google authentication successful!</h2><p>You can close this tab and return to the terminal.</p></body></html>";
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream.write_all(response.as_bytes()).await?;
                return Ok(code.to_string());
            }

            let body = "No authorization code received";
            let response = format!(
                "HTTP/1.1 400 Bad Request\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).await?;
            return Err(anyhow::anyhow!("No code in OAuth callback"));
        }
    })
    .await;

    match timeout {
        Ok(result) => result,
        Err(_) => bail!("Google OAuth callback timeout (5 minutes). Please try again."),
    }
}
