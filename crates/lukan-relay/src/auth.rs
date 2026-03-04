use anyhow::{Context, Result};
use axum::extract::Query;
use axum::http::header;
use axum::response::{IntoResponse, Redirect, Response};
use chrono::Utc;
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};

use crate::state::SharedState;

/// JWT claims for relay authentication.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RelayClaims {
    /// Subject (user ID — Google account ID)
    pub sub: String,
    /// User email
    pub email: String,
    /// Expiration (unix timestamp)
    pub exp: usize,
    /// Issued at
    pub iat: usize,
}

/// Create a signed JWT for a user.
pub fn create_jwt(secret: &str, user_id: &str, email: &str) -> Result<String> {
    let now = Utc::now().timestamp() as usize;
    let claims = RelayClaims {
        sub: user_id.to_string(),
        email: email.to_string(),
        exp: now + 30 * 24 * 60 * 60, // 30 days
        iat: now,
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?;
    Ok(token)
}

/// Verify and decode a JWT, returning the claims.
pub fn verify_jwt(secret: &str, token: &str) -> Result<RelayClaims> {
    let data = decode::<RelayClaims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .context("Invalid or expired JWT")?;
    Ok(data.claims)
}

// ── Cookie helpers ──────────────────────────────────────────────────

/// Build a `Set-Cookie` header value for the auth token.
/// Uses HttpOnly (no JS access), SameSite=Lax (allows OAuth redirects),
/// and Secure when the relay is behind HTTPS.
pub fn build_auth_cookie(token: &str, public_url: &str) -> String {
    let secure = public_url.starts_with("https://");
    let mut cookie = format!(
        "lukan_token={}; HttpOnly; SameSite=Lax; Path=/; Max-Age={}",
        token,
        30 * 24 * 60 * 60 // 30 days
    );
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

/// Build a `Set-Cookie` header that clears the auth cookie.
fn build_clear_cookie() -> String {
    "lukan_token=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0".to_string()
}

/// Extract the `lukan_token` value from the `Cookie` request header.
pub fn extract_token_from_cookie(headers: &axum::http::HeaderMap) -> Option<String> {
    let cookie_header = headers.get(header::COOKIE)?.to_str().ok()?;
    for part in cookie_header.split(';') {
        let part = part.trim();
        if let Some(value) = part.strip_prefix("lukan_token=")
            && !value.is_empty()
        {
            return Some(value.to_string());
        }
    }
    None
}

// ── Auth status & logout ────────────────────────────────────────────

/// GET /auth/status — check if the browser has a valid auth cookie.
pub async fn auth_status(
    axum::extract::State(state): axum::extract::State<SharedState>,
    headers: axum::http::HeaderMap,
) -> Response {
    let (authenticated, daemon_connected) = match extract_token_from_cookie(&headers) {
        Some(token) => match verify_jwt(&state.browser_jwt_secret(), &token) {
            Ok(claims) => (true, state.has_daemon(&claims.sub)),
            Err(_) => (false, false),
        },
        None => (false, false),
    };

    axum::Json(serde_json::json!({
        "authenticated": authenticated,
        "daemonConnected": daemon_connected,
    }))
    .into_response()
}

/// POST /auth/logout — clear the auth cookie.
pub async fn auth_logout() -> Response {
    let mut resp = axum::Json(serde_json::json!({ "ok": true })).into_response();
    resp.headers_mut().insert(
        header::SET_COOKIE,
        build_clear_cookie().parse().unwrap(),
    );
    resp
}

// ── Google OAuth ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct OAuthCallbackParams {
    pub code: String,
    #[allow(dead_code)]
    pub state: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GoogleLoginParams {
    /// Optional localhost redirect port for CLI login flow.
    pub cli_port: Option<u16>,
}

/// Google OAuth token exchange response.
#[derive(Debug, Deserialize)]
struct GoogleTokenResponse {
    // access_token: String,
    id_token: Option<String>,
}

/// Google ID token payload (subset of fields we care about).
#[derive(Debug, Deserialize)]
struct GoogleIdTokenPayload {
    sub: String,
    email: String,
}

/// GET /auth/google — redirect to Google OAuth consent screen.
pub async fn google_login(
    axum::extract::State(state): axum::extract::State<SharedState>,
    Query(params): Query<GoogleLoginParams>,
) -> Response {
    let redirect_uri = format!("{}/auth/callback", state.public_url);

    // Include cli_port in state if present (for local callback redirect)
    let oauth_state = params
        .cli_port
        .map(|p| format!("cli_port={p}"))
        .unwrap_or_default();

    let url = format!(
        "https://accounts.google.com/o/oauth2/v2/auth?\
         client_id={}&\
         redirect_uri={}&\
         response_type=code&\
         scope=openid%20email%20profile&\
         access_type=offline&\
         state={}",
        urlencoding::encode(&state.google_client_id),
        urlencoding::encode(&redirect_uri),
        urlencoding::encode(&oauth_state),
    );

    Redirect::temporary(&url).into_response()
}

/// GET /auth/callback — Google OAuth callback, exchange code for ID token.
pub async fn google_callback(
    axum::extract::State(state): axum::extract::State<SharedState>,
    Query(params): Query<OAuthCallbackParams>,
) -> Response {
    match handle_google_callback(&state, &params).await {
        Ok(response) => response,
        Err(e) => {
            tracing::error!(error = %e, "Google OAuth callback failed");
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Authentication failed: {e}"),
            )
                .into_response()
        }
    }
}

async fn handle_google_callback(
    state: &SharedState,
    params: &OAuthCallbackParams,
) -> Result<Response> {
    let redirect_uri = format!("{}/auth/callback", state.public_url);

    // Exchange authorization code for tokens
    let client = reqwest::Client::new();
    let token_resp = client
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("code", params.code.as_str()),
            ("client_id", &state.google_client_id),
            ("client_secret", &state.google_client_secret),
            ("redirect_uri", &redirect_uri),
            ("grant_type", "authorization_code"),
        ])
        .send()
        .await
        .context("Failed to exchange auth code")?;

    if !token_resp.status().is_success() {
        let text = token_resp.text().await.unwrap_or_default();
        anyhow::bail!("Google token exchange failed: {text}");
    }

    let token_data: GoogleTokenResponse = token_resp.json().await?;
    let id_token = token_data
        .id_token
        .context("No id_token in Google response")?;

    // Decode the ID token (we trust Google's signature since we just received it)
    let payload = decode_google_id_token(&id_token)?;

    // Check if this was a CLI login flow (has cli_port in state)
    let cli_port = params
        .state
        .as_deref()
        .and_then(|s| s.strip_prefix("cli_port="))
        .and_then(|p| p.parse::<u16>().ok());

    if let Some(port) = cli_port {
        // CLI/daemon login — use base jwt_secret (survives relay restarts)
        let jwt = create_jwt(&state.jwt_secret, &payload.sub, &payload.email)?;
        let redirect = format!(
            "http://localhost:{port}/callback?token={}&user_id={}&email={}",
            urlencoding::encode(&jwt),
            urlencoding::encode(&payload.sub),
            urlencoding::encode(&payload.email),
        );
        Ok(Redirect::temporary(&redirect).into_response())
    } else {
        // Browser login — use browser_jwt_secret (invalidated on restart)
        // Set HttpOnly cookie instead of passing token in URL
        let jwt = create_jwt(&state.browser_jwt_secret(), &payload.sub, &payload.email)?;
        let cookie = build_auth_cookie(&jwt, &state.public_url);
        let mut resp = Redirect::temporary(&state.public_url).into_response();
        resp.headers_mut().insert(
            header::SET_COOKIE,
            cookie.parse().unwrap(),
        );
        Ok(resp)
    }
}

// ── Dev mode ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct DevLoginBody {
    pub email: Option<String>,
    pub secret: Option<String>,
}

/// GET /auth/dev — Returns whether dev login is available (for frontend to show the form).
pub async fn dev_login(
    axum::extract::State(state): axum::extract::State<SharedState>,
) -> Response {
    if !state.dev_mode {
        return (axum::http::StatusCode::NOT_FOUND, "Dev login not enabled").into_response();
    }

    axum::Json(serde_json::json!({
        "devMode": true,
        "requiresSecret": state.dev_secret.is_some(),
    }))
    .into_response()
}

/// POST /auth/dev — Issue a browser JWT via dev mode, set as HttpOnly cookie.
/// Accepts JSON body: { "email": "...", "secret": "..." }
pub async fn dev_login_post(
    axum::extract::State(state): axum::extract::State<SharedState>,
    axum::Json(body): axum::Json<DevLoginBody>,
) -> Response {
    if !state.dev_mode {
        return (axum::http::StatusCode::NOT_FOUND, "Dev login not enabled").into_response();
    }

    if let Some(expected) = &state.dev_secret {
        match &body.secret {
            Some(provided) if provided == expected => {}
            _ => {
                return (
                    axum::http::StatusCode::UNAUTHORIZED,
                    "Invalid or missing secret",
                )
                    .into_response();
            }
        }
    }

    let email = body.email.unwrap_or_else(|| "dev@localhost".to_string());
    let user_id = format!("dev-{}", email.replace(['@', '.'], "-"));

    // Browser token — uses browser_jwt_secret (invalidated on relay restart)
    match create_jwt(&state.browser_jwt_secret(), &user_id, &email) {
        Ok(jwt) => {
            let cookie = build_auth_cookie(&jwt, &state.public_url);
            let mut resp = axum::Json(serde_json::json!({
                "ok": true,
                "email": email,
            }))
            .into_response();
            resp.headers_mut().insert(
                header::SET_COOKIE,
                cookie.parse().unwrap(),
            );
            resp
        }
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to create token: {e}"),
        )
            .into_response(),
    }
}

/// POST /auth/dev/token — Return a raw JWT for CLI/daemon testing (dev mode only).
/// Uses base jwt_secret (survives relay restarts) since daemon needs persistent auth.
pub async fn dev_token(
    axum::extract::State(state): axum::extract::State<SharedState>,
    axum::Json(body): axum::Json<DevLoginBody>,
) -> Response {
    if !state.dev_mode {
        return (axum::http::StatusCode::NOT_FOUND, "Dev mode not enabled").into_response();
    }

    if let Some(expected) = &state.dev_secret {
        match &body.secret {
            Some(provided) if provided == expected => {}
            _ => {
                return (
                    axum::http::StatusCode::UNAUTHORIZED,
                    "Invalid or missing secret",
                )
                    .into_response();
            }
        }
    }

    let email = body.email.unwrap_or_else(|| "dev@localhost".to_string());
    let user_id = format!("dev-{}", email.replace(['@', '.'], "-"));

    // Daemon token — uses base jwt_secret (survives relay restarts)
    match create_jwt(&state.jwt_secret, &user_id, &email) {
        Ok(jwt) => {
            let resp = serde_json::json!({
                "token": jwt,
                "userId": user_id,
                "email": email,
            });
            axum::Json(resp).into_response()
        }
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to create token: {e}"),
        )
            .into_response(),
    }
}

/// Decode a Google ID token's payload without verifying the signature.
/// We trust it because we just received it directly from Google's token endpoint.
fn decode_google_id_token(id_token: &str) -> Result<GoogleIdTokenPayload> {
    let parts: Vec<&str> = id_token.split('.').collect();
    if parts.len() != 3 {
        anyhow::bail!("Invalid ID token format");
    }
    use base64::Engine;
    let payload_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(parts[1])
        .context("Failed to decode ID token payload")?;
    let payload: GoogleIdTokenPayload = serde_json::from_slice(&payload_bytes)?;
    Ok(payload)
}
