use std::net::IpAddr;

use anyhow::{Context, Result};
use axum::extract::{ConnectInfo, Query};
use axum::http::header;
use axum::response::{IntoResponse, Redirect, Response};
use chrono::Utc;
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;

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
    let mut validation = Validation::default();
    validation.validate_aud = false;
    let data = decode::<RelayClaims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .context("Invalid or expired JWT")?;
    Ok(data.claims)
}

/// Constant-time comparison of two secret strings (prevents timing attacks).
fn secrets_equal(a: &str, b: &str) -> bool {
    a.as_bytes().ct_eq(b.as_bytes()).into()
}

// ── Rate limit helper ──────────────────────────────────────────────

fn rate_limit_response() -> Response {
    (
        axum::http::StatusCode::TOO_MANY_REQUESTS,
        "Rate limit exceeded — try again later",
    )
        .into_response()
}

// ── Cookie helpers ──────────────────────────────────────────────────

/// Build a `Set-Cookie` header value for the auth token.
/// Uses HttpOnly (no JS access), SameSite=None (allows cross-origin requests
/// from authorized dashboards), and Secure (required by SameSite=None).
/// CSRF is mitigated by restrictive CORS (only whitelisted origins via RELAY_CORS_ORIGINS).
pub fn build_auth_cookie(token: &str) -> String {
    format!(
        "lukan_token={}; HttpOnly; SameSite=None; Path=/; Max-Age={}; Secure",
        token,
        30 * 24 * 60 * 60 // 30 days
    )
}

/// Build a `Set-Cookie` header that clears the auth cookie.
fn build_clear_cookie() -> String {
    "lukan_token=; HttpOnly; SameSite=None; Path=/; Max-Age=0; Secure".to_string()
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

// ── Client IP helper ────────────────────────────────────────────────

/// Extract the client IP from X-Forwarded-For header or ConnectInfo.
/// Converts IPv4-mapped IPv6 addresses (::ffff:x.x.x.x) to plain IPv4.
pub fn client_ip(
    headers: &axum::http::HeaderMap,
    connect_info: Option<&axum::extract::ConnectInfo<std::net::SocketAddr>>,
) -> IpAddr {
    let raw = if let Some(forwarded) = headers.get("x-forwarded-for")
        && let Ok(val) = forwarded.to_str()
        && let Some(first) = val.split(',').next()
        && let Ok(ip) = first.trim().parse::<IpAddr>()
    {
        ip
    } else if let Some(info) = connect_info {
        info.0.ip()
    } else {
        return IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED);
    };

    // Convert IPv4-mapped IPv6 (::ffff:x.x.x.x) to plain IPv4
    match raw {
        IpAddr::V6(v6) => v6
            .to_ipv4_mapped()
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V6(v6)),
        ip => ip,
    }
}

// ── Auth status & logout ────────────────────────────────────────────

/// GET /auth/status — check if the browser has a valid auth cookie.
/// Requires valid browser cookie auth; returns 401 if not authenticated.
pub async fn auth_status(
    axum::extract::State(state): axum::extract::State<SharedState>,
    headers: axum::http::HeaderMap,
) -> Response {
    match extract_token_from_cookie(&headers) {
        Some(token) => match verify_jwt(&state.browser_jwt_secret(), &token) {
            Ok(claims) => {
                let devices = state.list_device_names(&claims.sub);
                axum::Json(serde_json::json!({
                    "authenticated": true,
                    "daemonConnected": !devices.is_empty(),
                    "devices": devices,
                }))
                .into_response()
            }
            Err(_) => (
                axum::http::StatusCode::UNAUTHORIZED,
                axum::Json(serde_json::json!({
                    "authenticated": false,
                    "daemonConnected": false,
                })),
            )
                .into_response(),
        },
        None => (
            axum::http::StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({
                "authenticated": false,
                "daemonConnected": false,
            })),
        )
            .into_response(),
    }
}

/// POST /auth/logout — clear the auth cookie.
pub async fn auth_logout() -> Response {
    let mut resp = axum::Json(serde_json::json!({ "ok": true })).into_response();
    resp.headers_mut()
        .insert(header::SET_COOKIE, build_clear_cookie().parse().unwrap());
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
    /// Optional path to redirect to after login (e.g. "/device").
    pub redirect: Option<String>,
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

    // Encode flow info in OAuth state parameter with CSRF nonce
    let flow_info = if let Some(port) = params.cli_port {
        format!("cli_port={port}")
    } else if let Some(redirect) = &params.redirect {
        format!("redirect={redirect}")
    } else {
        String::new()
    };
    let oauth_state = state.create_oauth_state(&flow_info);

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
    // Verify CSRF state parameter (single-use nonce)
    let flow_info = params
        .state
        .as_deref()
        .and_then(|s| state.verify_oauth_state(s))
        .context("Invalid or expired OAuth state parameter (CSRF check failed)")?;

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

    // Parse flow info from verified OAuth state
    let cli_port = flow_info
        .strip_prefix("cli_port=")
        .and_then(|p| p.parse::<u16>().ok());
    let redirect_path = flow_info.strip_prefix("redirect=");

    if let Some(port) = cli_port {
        // CLI/daemon login — create a short-lived auth code instead of passing token in URL
        let jwt = create_jwt(&state.jwt_secret, &payload.sub, &payload.email)?;
        let auth_code = state.create_auth_code(jwt, payload.sub.clone(), payload.email.clone());
        let redirect = format!(
            "http://localhost:{port}/callback?code={}&user_id={}&email={}",
            urlencoding::encode(&auth_code),
            urlencoding::encode(&payload.sub),
            urlencoding::encode(&payload.email),
        );
        Ok(Redirect::temporary(&redirect).into_response())
    } else {
        // Browser login — use browser_jwt_secret (invalidated on restart)
        // Set HttpOnly cookie instead of passing token in URL
        let jwt = create_jwt(&state.browser_jwt_secret(), &payload.sub, &payload.email)?;
        let cookie = build_auth_cookie(&jwt);
        let target = if let Some(path) = redirect_path {
            format!("{}{path}", state.public_url)
        } else {
            state.public_url.clone()
        };
        let mut resp = Redirect::temporary(&target).into_response();
        resp.headers_mut()
            .insert(header::SET_COOKIE, cookie.parse().unwrap());
        Ok(resp)
    }
}

// ── Auth Code Exchange ──────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct AuthCodeExchangeBody {
    pub code: String,
}

/// POST /auth/exchange — Exchange a short-lived auth code for a JWT token.
/// Used by CLI after OAuth callback redirects with ?code= instead of ?token=.
pub async fn auth_code_exchange(
    axum::extract::State(state): axum::extract::State<SharedState>,
    axum::Json(body): axum::Json<AuthCodeExchangeBody>,
) -> Response {
    match state.exchange_auth_code(&body.code) {
        Some(entry) => axum::Json(serde_json::json!({
            "token": entry.token,
            "userId": entry.user_id,
            "email": entry.email,
        }))
        .into_response(),
        None => (
            axum::http::StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({ "error": "Invalid or expired auth code" })),
        )
            .into_response(),
    }
}

// ── Dev mode ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct DevLoginBody {
    pub email: Option<String>,
    pub secret: Option<String>,
}

/// GET /auth/dev — Returns whether dev login is available (for frontend to show the form).
pub async fn dev_login(axum::extract::State(state): axum::extract::State<SharedState>) -> Response {
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
            Some(provided) if secrets_equal(provided, expected) => {}
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
            let cookie = build_auth_cookie(&jwt);
            let mut resp = axum::Json(serde_json::json!({
                "ok": true,
                "email": email,
            }))
            .into_response();
            resp.headers_mut()
                .insert(header::SET_COOKIE, cookie.parse().unwrap());
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
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    headers: axum::http::HeaderMap,
    axum::Json(body): axum::Json<DevLoginBody>,
) -> Response {
    if !state.dev_mode {
        return (axum::http::StatusCode::NOT_FOUND, "Dev mode not enabled").into_response();
    }

    // Rate limit
    let ip = client_ip(&headers, Some(&ConnectInfo(addr)));
    if !state.rate_dev_token.check(ip) {
        return rate_limit_response();
    }

    if let Some(expected) = &state.dev_secret {
        match &body.secret {
            Some(provided) if secrets_equal(provided, expected) => {}
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

// ── Device code flow ─────────────────────────────────────────────────

/// POST /auth/device — Start a device code login flow.
/// Returns a device_code (for polling) and user_code (for the user to enter in a browser).
pub async fn device_code_start(
    axum::extract::State(state): axum::extract::State<SharedState>,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    headers: axum::http::HeaderMap,
) -> Response {
    let ip = client_ip(&headers, Some(&ConnectInfo(addr)));
    if !state.rate_device_start.check(ip) {
        return rate_limit_response();
    }

    let (device_code, user_code) = state.create_device_code();
    let verification_url = format!("{}/device", state.public_url);

    axum::Json(serde_json::json!({
        "deviceCode": device_code,
        "userCode": user_code,
        "verificationUrl": verification_url,
        "expiresIn": 900,
        "interval": 5,
    }))
    .into_response()
}

#[derive(Debug, Deserialize)]
pub struct DeviceCodePollBody {
    #[serde(rename = "deviceCode")]
    pub device_code: String,
}

/// POST /auth/device/poll — CLI polls this to check if the user has authorized.
pub async fn device_code_poll(
    axum::extract::State(state): axum::extract::State<SharedState>,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    headers: axum::http::HeaderMap,
    axum::Json(body): axum::Json<DeviceCodePollBody>,
) -> Response {
    use crate::state::DeviceCodePollResult;

    let ip = client_ip(&headers, Some(&ConnectInfo(addr)));
    if !state.rate_device_poll.check(ip) {
        return rate_limit_response();
    }

    match state.poll_device_code(&body.device_code) {
        Some(DeviceCodePollResult::Pending) => {
            axum::Json(serde_json::json!({ "status": "pending" })).into_response()
        }
        Some(DeviceCodePollResult::Authorized {
            token,
            user_id,
            email,
        }) => axum::Json(serde_json::json!({
            "status": "authorized",
            "token": token,
            "userId": user_id,
            "email": email,
        }))
        .into_response(),
        Some(DeviceCodePollResult::Expired) | None => axum::Json(serde_json::json!({
            "status": "expired",
        }))
        .into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct DeviceCodeVerifyBody {
    #[serde(rename = "userCode")]
    pub user_code: String,
    /// For dev mode: email
    pub email: Option<String>,
    /// For dev mode: secret
    pub secret: Option<String>,
}

/// POST /auth/device/verify — Browser sends the user_code + credentials to authorize.
/// In dev mode: accepts email + optional secret (same as /auth/dev/token).
/// In production: this endpoint is called after Google OAuth completes (the browser
/// is already authenticated via cookie), so we verify the cookie instead.
pub async fn device_code_verify(
    axum::extract::State(state): axum::extract::State<SharedState>,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    headers: axum::http::HeaderMap,
    axum::Json(body): axum::Json<DeviceCodeVerifyBody>,
) -> Response {
    let ip = client_ip(&headers, Some(&ConnectInfo(addr)));
    if !state.rate_device_verify.check(ip) {
        return rate_limit_response();
    }

    // Find the device code entry by user_code
    let device_code = match state.find_by_user_code(&body.user_code) {
        Some(dc) => dc,
        None => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({ "error": "Invalid or expired code" })),
            )
                .into_response();
        }
    };

    // Determine user identity: try browser cookie first, then dev mode
    let (user_id, email) = if let Some(token) = extract_token_from_cookie(&headers) {
        // Browser is authenticated — use their identity
        match verify_jwt(&state.browser_jwt_secret(), &token) {
            Ok(claims) => (claims.sub, claims.email),
            Err(_) => {
                return (
                    axum::http::StatusCode::UNAUTHORIZED,
                    axum::Json(serde_json::json!({ "error": "Invalid browser session" })),
                )
                    .into_response();
            }
        }
    } else if state.dev_mode {
        // Dev mode: use email from body
        if let Some(expected) = &state.dev_secret {
            match &body.secret {
                Some(provided) if secrets_equal(provided, expected) => {}
                _ => {
                    return (
                        axum::http::StatusCode::UNAUTHORIZED,
                        axum::Json(serde_json::json!({ "error": "Invalid or missing secret" })),
                    )
                        .into_response();
                }
            }
        }
        let email = body.email.unwrap_or_else(|| "dev@localhost".to_string());
        let user_id = format!("dev-{}", email.replace(['@', '.'], "-"));
        (user_id, email)
    } else {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({ "error": "Not authenticated" })),
        )
            .into_response();
    };

    // Create a daemon JWT (uses base jwt_secret, survives relay restarts)
    let jwt = match create_jwt(&state.jwt_secret, &user_id, &email) {
        Ok(t) => t,
        Err(e) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(serde_json::json!({ "error": format!("Failed to create token: {e}") })),
            )
                .into_response();
        }
    };

    // Mark the device code as authorized
    state.complete_device_code(&device_code, jwt, user_id, email);

    axum::Json(serde_json::json!({ "ok": true })).into_response()
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
