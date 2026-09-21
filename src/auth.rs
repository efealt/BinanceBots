use axum::{
    Form, Router,
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::Next,
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use crate::storage::StorageReader;
use serde::Deserialize;
use std::{
    collections::HashMap,
    env,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use std::io::Read;
use thiserror::Error;

const SESSION_COOKIE: &str = "session";
const SESSION_TTL_SECONDS: u64 = 12 * 60 * 60;

pub struct AuthState {
    enabled: bool,
    username: String,
    password: String,
    secure_cookie: bool,
    sessions: Mutex<HashMap<String, Instant>>,
    storage_reader: Arc<StorageReader>,
}

impl AuthState {
    pub fn from_env(storage_reader: Arc<StorageReader>) -> Result<Self, AuthConfigError> {
        let render_runtime = env::var_os("PORT").is_some();
        let default_mode = if render_runtime { "enabled" } else { "disabled" };
        let mode = env::var("BINANCE_GRID_AUTH_MODE").unwrap_or_else(|_| default_mode.to_string());
        let enabled = match mode.trim().to_ascii_lowercase().as_str() {
            "enabled" => true,
            "disabled" => false,
            _ => return Err(AuthConfigError::InvalidMode),
        };

        let username = env::var("BINANCE_GRID_AUTH_USERNAME").unwrap_or_default();
        let password = env::var("BINANCE_GRID_AUTH_PASSWORD").unwrap_or_default();

        if enabled {
            if username.trim().is_empty() {
                return Err(AuthConfigError::MissingUsername);
            }
            if password.len() < 12 {
                return Err(AuthConfigError::WeakPassword);
            }
        }

        Ok(Self {
            enabled,
            username,
            password,
            secure_cookie: render_runtime,
            sessions: Mutex::new(HashMap::new()),
            storage_reader,
        })
    }

    fn valid_credentials(&self, username: &str, password: &str) -> bool {
        self.enabled
            && constant_time_eq(self.username.as_bytes(), username.as_bytes())
            && constant_time_eq(self.password.as_bytes(), password.as_bytes())
    }

    fn create_session(&self) -> Result<String, std::io::Error> {
        let mut random = [0_u8; 32];
        std::fs::File::open("/dev/urandom")?.read_exact(&mut random)?;
        let token = random.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
        let expiry = Instant::now() + Duration::from_secs(SESSION_TTL_SECONDS);
        self.sessions_lock().insert(token.clone(), expiry);
        Ok(token)
    }

    fn session_valid(&self, token: &str) -> bool {
        if !self.enabled {
            return true;
        }
        let now = Instant::now();
        let mut sessions = self.sessions_lock();
        sessions.retain(|_, expiry| *expiry > now);
        sessions.get(token).is_some_and(|expiry| *expiry > now)
    }

    fn invalidate_session(&self, token: &str) {
        self.sessions_lock().remove(token);
    }

    fn sessions_lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Instant>> {
        self.sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn session_cookie(&self, token: &str) -> String {
        let secure = if self.secure_cookie { "; Secure" } else { "" };
        format!(
            "{SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={SESSION_TTL_SECONDS}{secure}"
        )
    }

    fn clear_cookie(&self) -> String {
        let secure = if self.secure_cookie { "; Secure" } else { "" };
        format!(
            "{SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0{secure}"
        )
    }
}

#[derive(Debug, Error)]
pub enum AuthConfigError {
    #[error("BINANCE_GRID_AUTH_MODE must be enabled or disabled")]
    InvalidMode,
    #[error("BINANCE_GRID_AUTH_USERNAME is required when authentication is enabled")]
    MissingUsername,
    #[error("BINANCE_GRID_AUTH_PASSWORD must contain at least 12 characters when authentication is enabled")]
    WeakPassword,
}

pub fn public_router(state: Arc<AuthState>) -> Router {
    Router::new()
        .route("/login", get(login_page).post(login))
        .route("/logout", post(logout))
        .with_state(state)
}

pub async fn require_auth(
    State(state): State<Arc<AuthState>>,
    request: Request,
    next: Next,
) -> Response {
    if !state.enabled {
        return next.run(request).await;
    }

    let path = request.uri().path().to_string();
    let authenticated = session_token(request.headers())
        .as_deref()
        .is_some_and(|token| state.session_valid(token));

    if authenticated {
        let mut response = next.run(request).await;
        if should_disable_cache(&path) {
            apply_no_store_headers(&mut response);
        }
        return response;
    }

    let mut response = if path.starts_with("/api/") {
        (StatusCode::UNAUTHORIZED, "authentication required").into_response()
    } else {
        Redirect::to("/login").into_response()
    };
    apply_no_store_headers(&mut response);
    response
}

async fn login_page(State(state): State<Arc<AuthState>>, headers: HeaderMap) -> Response {
    if !state.enabled {
        return Redirect::to("/").into_response();
    }
    if session_token(&headers)
        .as_deref()
        .is_some_and(|token| state.session_valid(token))
    {
        return Redirect::to("/").into_response();
    }
    login_response(StatusCode::OK, false)
}

#[derive(Deserialize)]
struct LoginForm {
    username: String,
    password: String,
}

async fn login(
    State(state): State<Arc<AuthState>>,
    headers: HeaderMap,
    Form(form): Form<LoginForm>,
) -> Response {
    if !state.enabled {
        return Redirect::to("/").into_response();
    }

    if !state.valid_credentials(&form.username, &form.password) {
        record_auth_event(&state, &headers, "login_failed").await;
        tokio::time::sleep(Duration::from_millis(600)).await;
        return login_response(StatusCode::UNAUTHORIZED, true);
    }

    let token = match state.create_session() {
        Ok(token) => token,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "could not create session").into_response(),
    };
    record_auth_event(&state, &headers, "login_success").await;
    let mut response = Redirect::to("/").into_response();
    if let Ok(cookie) = HeaderValue::from_str(&state.session_cookie(&token)) {
        response.headers_mut().insert(header::SET_COOKIE, cookie);
    }
    apply_no_store_headers(&mut response);
    response
}

async fn logout(State(state): State<Arc<AuthState>>, headers: HeaderMap) -> Response {
    if let Some(token) = session_token(&headers) {
        state.invalidate_session(&token);
    }
    record_auth_event(&state, &headers, "logout").await;
    let mut response = Redirect::to("/login").into_response();
    if let Ok(cookie) = HeaderValue::from_str(&state.clear_cookie()) {
        response.headers_mut().insert(header::SET_COOKIE, cookie);
    }
    apply_no_store_headers(&mut response);
    response.headers_mut().insert(
        "clear-site-data",
        HeaderValue::from_static("\"cache\""),
    );
    response
}

async fn record_auth_event(state: &Arc<AuthState>, headers: &HeaderMap, event_type: &'static str) {
    let source_ip = request_source_ip(headers);
    let user_agent = header_value(headers, header::USER_AGENT, 512);
    let storage_reader = Arc::clone(&state.storage_reader);
    let result = tokio::task::spawn_blocking(move || {
        storage_reader.record_auth_event(event_type, source_ip.as_deref(), user_agent.as_deref())
    })
    .await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(error)) => eprintln!("auth audit write failed: {error}"),
        Err(error) => eprintln!("auth audit task failed: {error}"),
    }
}

fn request_source_ip(headers: &HeaderMap) -> Option<String> {
    ["x-forwarded-for", "x-real-ip", "cf-connecting-ip"]
        .iter()
        .find_map(|name| {
            headers
                .get(*name)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.split(',').next())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| truncate(value, 128))
        })
}

fn header_value(headers: &HeaderMap, name: header::HeaderName, max_len: usize) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| truncate(value, max_len))
}

fn truncate(value: &str, max_len: usize) -> String {
    value.chars().take(max_len).collect()
}

fn session_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .filter_map(|part| part.trim().split_once('='))
        .find_map(|(name, value)| (name == SESSION_COOKIE).then(|| value.to_string()))
}

fn constant_time_eq(expected: &[u8], supplied: &[u8]) -> bool {
    if expected.len() != supplied.len() {
        return false;
    }
    let mut diff = 0_u8;
    for (left, right) in expected.iter().zip(supplied.iter()) {
        diff |= left ^ right;
    }
    diff == 0
}

fn should_disable_cache(path: &str) -> bool {
    path == "/" || path.ends_with(".html") || path.starts_with("/api/")
}

fn apply_no_store_headers(response: &mut Response) {
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store, max-age=0"),
    );
    response
        .headers_mut()
        .insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
    response
        .headers_mut()
        .insert(header::EXPIRES, HeaderValue::from_static("0"));
}

fn login_response(status: StatusCode, invalid: bool) -> Response {
    let error = if invalid {
        r#"<p class="error" role="alert">Invalid username or password.</p>"#
    } else {
        ""
    };
    let html = format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="color-scheme" content="dark light">
<title>Secure Access</title>
<style>
:root {{ font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; color-scheme: dark; background:#0b0f14; color:#eef3f8; }}
* {{ box-sizing:border-box; }}
body {{ margin:0; min-height:100vh; display:grid; place-items:center; padding:24px; background:radial-gradient(circle at top,#162231 0,#0b0f14 45%); }}
main {{ width:min(420px,100%); border:1px solid #263443; border-radius:18px; background:#101720; padding:30px; box-shadow:0 24px 70px rgba(0,0,0,.35); }}
.brand {{ display:flex; align-items:center; gap:12px; margin-bottom:26px; }}
.mark {{ width:42px; height:42px; border-radius:12px; display:grid; place-items:center; background:#e6b94a; color:#111; font-weight:800; }}
h1 {{ margin:0; font-size:1.35rem; }}
small {{ color:#8f9eae; }}
label {{ display:grid; gap:8px; margin-top:16px; color:#b9c4cf; font-size:.9rem; }}
input {{ width:100%; border:1px solid #304253; border-radius:10px; background:#0b1118; color:#fff; padding:12px 13px; font:inherit; }}
input:focus {{ outline:2px solid #e6b94a; outline-offset:1px; }}
button {{ width:100%; margin-top:22px; border:0; border-radius:10px; padding:12px 14px; background:#e6b94a; color:#111; font-weight:800; cursor:pointer; }}
.error {{ margin:16px 0 0; color:#ff9a9a; font-size:.9rem; }}
.note {{ margin:22px 0 0; color:#758596; font-size:.82rem; line-height:1.5; }}
</style>
</head>
<body>
<main>
<div class="brand"><span class="mark">SA</span><div><h1>Secure Access</h1><small>Private workspace</small></div></div>
<form method="post" action="/login">
<label>Username<input name="username" autocomplete="username" required autofocus></label>
<label>Password<input name="password" type="password" autocomplete="current-password" required></label>
<button type="submit">Sign in</button>
</form>
{error}
<p class="note">This private session expires after 12 hours or when the service restarts.</p>
</main>
</body>
</html>"#
    );
    let mut response = (status, Html(html)).into_response();
    apply_no_store_headers(&mut response);
    response
}

#[cfg(test)]
mod tests {
    use super::{SESSION_COOKIE, constant_time_eq, session_token};
    use axum::http::{HeaderMap, HeaderValue, header};

    #[test]
    fn constant_time_compare_requires_exact_match() {
        assert!(constant_time_eq(b"correct horse", b"correct horse"));
        assert!(!constant_time_eq(b"correct horse", b"correct house"));
        assert!(!constant_time_eq(b"short", b"longer"));
    }

    #[test]
    fn session_cookie_parser_finds_named_cookie() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("theme=dark; session=abc-123; x=1"),
        );
        assert_eq!(session_token(&headers).as_deref(), Some("abc-123"));
        assert_eq!(SESSION_COOKIE, "session");
    }
}
