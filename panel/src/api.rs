//! JSON API used by the web page. Everything under /api except /api/login
//! requires a logged-in session (see `require_login`).

use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use common::Snapshot;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::RwLock;

use crate::agents::{self, AgentError};
use crate::auth::{self, Sessions};
use crate::settings::{self, ServerEntry, ServerList};

pub struct AppState {
    pub password_hash: String,
    pub sessions: Sessions,
    pub servers: RwLock<ServerList>,
    pub servers_path: std::path::PathBuf,
    pub http: reqwest::Client,
}

type St = State<Arc<AppState>>;

// ---------- errors ----------

pub struct ApiError(StatusCode, String);

impl ApiError {
    fn new(code: u16, msg: impl Into<String>) -> Self {
        ApiError(
            StatusCode::from_u16(code).unwrap_or(StatusCode::BAD_GATEWAY),
            msg.into(),
        )
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

impl From<AgentError> for ApiError {
    fn from(e: AgentError) -> Self {
        ApiError::new(e.status, e.message)
    }
}

type ApiResult<T> = Result<T, ApiError>;

// ---------- middleware ----------

/// Blocks /api/* unless logged in, and requires `X-Panel: 1` on anything that
/// changes state (CSRF protection; see auth.rs).
pub async fn require_login(State(st): St, req: Request, next: Next) -> Response {
    let path = req.uri().path();
    if !path.starts_with("/api/") {
        return next.run(req).await;
    }
    if req.method() != Method::GET
        && req.headers().get("x-panel").map(|v| v.as_bytes()) != Some(b"1")
    {
        return ApiError::new(403, "missing X-Panel header").into_response();
    }
    if path == "/api/login" {
        return next.run(req).await;
    }
    let ok = auth::session_from_headers(req.headers()).is_some_and(|id| st.sessions.is_valid(&id));
    if !ok {
        return ApiError::new(401, "not logged in").into_response();
    }
    next.run(req).await
}

// ---------- login ----------

#[derive(Deserialize)]
pub struct LoginBody {
    password: String,
}

pub async fn login(State(st): St, Json(body): Json<LoginBody>) -> ApiResult<Response> {
    if st.sessions.locked_out() {
        return Err(ApiError::new(
            429,
            "too many wrong passwords, try again in a few minutes",
        ));
    }

    // Argon2 is deliberately slow, so run it off the async threads.
    let hash = st.password_hash.clone();
    let ok = tokio::task::spawn_blocking(move || auth::verify_password(&body.password, &hash))
        .await
        .unwrap_or(false);

    if !ok {
        st.sessions.record_failure();
        tokio::time::sleep(Duration::from_secs(1)).await; // slow down guessing
        return Err(ApiError::new(401, "wrong password"));
    }

    st.sessions.clear_failures();
    let id = st.sessions.create();
    let mut resp = Json(json!({ "ok": true })).into_response();
    resp.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&auth::session_cookie(&id)).unwrap(),
    );
    Ok(resp)
}

pub async fn logout(State(st): St, headers: HeaderMap) -> Response {
    if let Some(id) = auth::session_from_headers(&headers) {
        st.sessions.remove(&id);
    }
    let mut resp = Json(json!({ "ok": true })).into_response();
    resp.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&auth::clear_cookie()).unwrap(),
    );
    resp
}

pub async fn me() -> Json<serde_json::Value> {
    Json(json!({ "ok": true }))
}

// ---------- servers ----------

#[derive(Serialize)]
pub struct ServerView {
    name: String,
    url: String,
    has_token: bool,
    ok: bool,
    error: Option<String>,
    stats: Option<Snapshot>,
}

/// Every server with its live stats, fetched in parallel.
pub async fn list_servers(State(st): St) -> Json<Vec<ServerView>> {
    let servers = st.servers.read().await.servers.clone();

    let mut tasks = tokio::task::JoinSet::new();
    for (name, entry) in servers {
        let http = st.http.clone();
        tasks.spawn(async move {
            let result = agents::stats(&http, &entry).await;
            let (ok, error, stats) = match result {
                Ok(s) => (true, None, Some(s)),
                Err(e) => (false, Some(e.message), None),
            };
            ServerView {
                has_token: entry.token.is_some(),
                url: entry.url,
                name,
                ok,
                error,
                stats,
            }
        });
    }

    let mut out: Vec<ServerView> = tasks.join_all().await;
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Json(out)
}

#[derive(Deserialize)]
pub struct ServerBody {
    url: String,
    /// Empty or missing = keep the existing token.
    token: Option<String>,
    /// Set true to remove the stored token.
    #[serde(default)]
    clear_token: bool,
}

/// Add or update a server, then test the connection.
pub async fn put_server(
    State(st): St,
    Path(name): Path<String>,
    Json(body): Json<ServerBody>,
) -> ApiResult<Json<serde_json::Value>> {
    if !settings::valid_server_name(&name) {
        return Err(ApiError::new(
            400,
            "names can use letters, numbers, - and _ (max 32)",
        ));
    }
    let url = settings::normalize_agent_url(&body.url).map_err(|e| ApiError::new(400, e))?;

    let entry = {
        let mut list = st.servers.write().await;
        let existing = list.servers.get(&name).and_then(|s| s.token.clone());
        let token = match body.token.map(|t| t.trim().to_string()) {
            Some(t) if !t.is_empty() => Some(t),
            _ if body.clear_token => None,
            _ => existing,
        };
        let entry = ServerEntry { url, token };
        list.servers.insert(name.clone(), entry.clone());
        settings::save_servers(&st.servers_path, &list).map_err(|e| ApiError::new(500, e))?;
        entry
    };

    // Saved either way; report whether it can actually be reached.
    let test = agents::stats(&st.http, &entry).await;
    Ok(Json(json!({
        "saved": true,
        "test_ok": test.is_ok(),
        "test_error": test.err().map(|e| e.message),
    })))
}

pub async fn delete_server(
    State(st): St,
    Path(name): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let mut list = st.servers.write().await;
    if list.servers.remove(&name).is_none() {
        return Err(ApiError::new(404, format!("no server called '{name}'")));
    }
    settings::save_servers(&st.servers_path, &list).map_err(|e| ApiError::new(500, e))?;
    Ok(Json(json!({ "ok": true })))
}

async fn find(st: &AppState, name: &str) -> ApiResult<ServerEntry> {
    st.servers
        .read()
        .await
        .servers
        .get(name)
        .cloned()
        .ok_or_else(|| ApiError::new(404, format!("no server called '{name}'")))
}

// ---------- actions ----------

pub async fn container_action(
    State(st): St,
    Path((name, container, action)): Path<(String, String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    if !matches!(action.as_str(), "start" | "stop" | "restart") {
        return Err(ApiError::new(404, "unknown action"));
    }
    let server = find(&st, &name).await?;
    agents::container_action(&st.http, &server, &container, &action).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct LogsQuery {
    lines: Option<u32>,
}

pub async fn container_logs(
    State(st): St,
    Path((name, container)): Path<(String, String)>,
    Query(q): Query<LogsQuery>,
) -> ApiResult<Response> {
    let server = find(&st, &name).await?;
    let text =
        agents::container_logs(&st.http, &server, &container, q.lines.unwrap_or(200)).await?;
    Ok(([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], text).into_response())
}

#[derive(Deserialize)]
pub struct RebootBody {
    /// Must equal the server name, typed by the user as confirmation.
    confirm: String,
}

pub async fn reboot(
    State(st): St,
    Path(name): Path<String>,
    Json(body): Json<RebootBody>,
) -> ApiResult<Json<serde_json::Value>> {
    if body.confirm != name {
        return Err(ApiError::new(400, "type the server's name to confirm"));
    }
    let server = find(&st, &name).await?;
    agents::reboot(&st.http, &server).await?;
    eprintln!("reboot of '{name}' requested from the panel");
    Ok(Json(json!({ "ok": true })))
}
