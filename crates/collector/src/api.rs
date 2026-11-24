//! Versioned HTTP + WebSocket API.
//!
//! Every request needs the pairing token, as `Authorization: Bearer <token>`,
//! `X-Fleet-Token: <token>`, or `?token=<token>` (for WebSockets, which can't set headers
//! from a browser). Requests from outside the LAN are refused before the token is checked:
//! only loopback, private (RFC 1918), link-local and Tailscale (100.64.0.0/10) addresses pass.

use crate::state::{self, SharedState};
use crate::types::StreamEvent;
use crate::files;
use crate::types::ReceivedFile;
use axum::body::Body;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, DefaultBodyLimit, Path, Query, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use futures_util::StreamExt;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use std::net::{IpAddr, SocketAddr};
use subtle::ConstantTimeEq;
use tokio::io::AsyncWriteExt;

pub fn router(st: SharedState) -> Router {
    let v1 = Router::new()
        .route("/info", get(info))
        .route("/snapshot", get(snapshot))
        .route("/stream", get(stream))
        .route("/metrics/history", get(metrics_history))
        .route("/claude/prompts", get(prompts))
        .route("/claude/daily", get(daily))
        .route("/claude/tokens", get(tokens))
        .route("/processes/{pid}/impact", get(kill_impact))
        .route("/processes/{pid}/kill", post(kill_process))
        .route("/kills", get(kill_log))
        // Streamed straight to disk, so no size limit on the body.
        .route("/files", put(receive_file).layer(DefaultBodyLimit::disable()))
        .route("/files/received", get(received_files))
        .route("/shell", get(shell))
        .route("/shell/status", get(shell_status))
        .route("/shell/sessions", get(shell_sessions))
        .route("/screen", get(screenshot))
        .route("/agent", get(agent_status))
        .route("/agent/tasks", get(agent_tasks).post(agent_submit))
        .route("/agent/tasks/{id}", get(agent_task))
        .route("/agent/tasks/{id}/cancel", post(agent_cancel));
    Router::new()
        .nest("/v1", v1)
        .layer(middleware::from_fn_with_state(st.clone(), guard))
        // The token is the credential (no cookies), so any origin may call us with it.
        .layer(tower_http::cors::CorsLayer::permissive())
        .with_state(st)
}

pub async fn serve(st: SharedState) -> anyhow::Result<()> {
    let addr = SocketAddr::from(([0, 0, 0, 0], st.cfg.port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| anyhow::anyhow!("binding port {}: {e}", st.cfg.port))?;
    tracing::info!("API listening on {addr} (LAN only, token required)");
    axum::serve(listener, router(st).into_make_service_with_connect_info::<SocketAddr>()).await?;
    Ok(())
}

pub fn is_lan(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || (o[0] == 100 && (o[1] & 0xC0) == 64) // 100.64.0.0/10 (Tailscale / CGNAT)
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_lan(IpAddr::V4(v4));
            }
            let s = v6.segments();
            v6.is_loopback()
                || (s[0] & 0xfe00) == 0xfc00 // unique local (incl. Tailscale fd7a:115c:a1e0::/48)
                || (s[0] & 0xffc0) == 0xfe80 // link-local
        }
    }
}

fn token_from(headers: &HeaderMap, query: Option<&str>) -> Option<String> {
    if let Some(v) = headers.get("authorization").and_then(|v| v.to_str().ok()) {
        if let Some(t) = v.strip_prefix("Bearer ") {
            return Some(t.trim().to_string());
        }
    }
    if let Some(v) = headers.get("x-fleet-token").and_then(|v| v.to_str().ok()) {
        return Some(v.trim().to_string());
    }
    query?
        .split('&')
        .find_map(|kv| kv.strip_prefix("token=").map(str::to_string))
}

async fn guard(
    State(st): State<SharedState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Response {
    if !is_lan(peer.ip()) {
        tracing::warn!("refused non-LAN request from {}", peer.ip());
        return (StatusCode::FORBIDDEN, Json(json!({"error": "LAN only"}))).into_response();
    }
    let given = token_from(req.headers(), req.uri().query()).unwrap_or_default();
    let ok: bool = given.as_bytes().ct_eq(st.cfg.token.as_bytes()).into();
    if !ok {
        return (StatusCode::UNAUTHORIZED, Json(json!({"error": "missing or wrong pairing token"})))
            .into_response();
    }
    next.run(req).await
}

type ApiResult<T> = Result<Json<T>, (StatusCode, Json<serde_json::Value>)>;

fn internal(e: impl std::fmt::Display) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()})))
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> ApiResult<T> {
    match tokio::task::spawn_blocking(f).await {
        Ok(Ok(v)) => Ok(Json(v)),
        Ok(Err(e)) => Err(internal(e)),
        Err(e) => Err(internal(e)),
    }
}

async fn info(State(st): State<SharedState>) -> impl IntoResponse {
    Json(st.machine.clone())
}

async fn snapshot(State(st): State<SharedState>) -> impl IntoResponse {
    Json(st.snapshot())
}

#[derive(Deserialize)]
struct HistoryQ {
    minutes: Option<i64>,
}

async fn metrics_history(State(st): State<SharedState>, Query(q): Query<HistoryQ>) -> ApiResult<Vec<crate::types::MetricsPoint>> {
    blocking(move || state::metrics_history(&st, q.minutes.unwrap_or(60))).await
}

#[derive(Deserialize)]
struct PromptsQ {
    limit: Option<usize>,
    before: Option<i64>,
}

async fn prompts(State(st): State<SharedState>, Query(q): Query<PromptsQ>) -> ApiResult<Vec<crate::types::PromptEntry>> {
    blocking(move || state::prompts(&st, q.limit.unwrap_or(50), q.before)).await
}

#[derive(Deserialize)]
struct DailyQ {
    days: Option<i64>,
}

async fn daily(State(st): State<SharedState>, Query(q): Query<DailyQ>) -> ApiResult<Vec<crate::types::DailyTokens>> {
    blocking(move || state::daily(&st, q.days.unwrap_or(14))).await
}

#[derive(Deserialize)]
struct TokensQ {
    from: i64,
    to: Option<i64>,
}

async fn tokens(State(st): State<SharedState>, Query(q): Query<TokensQ>) -> ApiResult<crate::types::TokenTotals> {
    blocking(move || state::tokens(&st, q.from, q.to.unwrap_or(i64::MAX))).await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImpactQ {
    start_time: Option<u64>,
}

async fn kill_impact(State(st): State<SharedState>, Path(pid): Path<u32>, Query(q): Query<ImpactQ>) -> Response {
    match st.kill_impact(pid, q.start_time) {
        Ok(i) => Json(i).into_response(),
        Err(e) => (StatusCode::NOT_FOUND, Json(json!({ "error": e }))).into_response(),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct KillBody {
    /// End the whole process tree (children first).
    #[serde(default)]
    tree: bool,
    /// Required: guards against a pid that was reused by another program.
    start_time: u64,
    /// Who asked, e.g. the dashboard machine's name. Logged with the caller's IP.
    #[serde(default)]
    requested_by: Option<String>,
}

async fn kill_process(
    State(st): State<SharedState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(pid): Path<u32>,
    Json(body): Json<KillBody>,
) -> Response {
    let who = format!(
        "{} ({})",
        body.requested_by.as_deref().filter(|s| !s.is_empty()).unwrap_or("unknown dashboard"),
        peer.ip()
    );
    let st2 = st.clone();
    let res = tokio::task::spawn_blocking(move || st2.kill(pid, Some(body.start_time), body.tree, &who)).await;
    match res {
        Ok(Ok(r)) => Json(r).into_response(),
        Ok(Err(e)) => (StatusCode::CONFLICT, Json(json!({ "error": e }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response(),
    }
}

#[derive(Deserialize)]
struct KillsQ {
    limit: Option<usize>,
}

async fn kill_log(State(st): State<SharedState>, Query(q): Query<KillsQ>) -> ApiResult<Vec<crate::types::KillLogEntry>> {
    blocking(move || state::kills(&st, q.limit.unwrap_or(50))).await
}

#[derive(Deserialize)]
struct FileQ {
    name: String,
    /// Sending dashboard's machine name (shown to the receiver).
    from: Option<String>,
}

fn fail(code: StatusCode, msg: impl Into<String>) -> Response {
    (code, Json(json!({ "error": msg.into() }))).into_response()
}

fn gib(b: u64) -> String {
    format!("{:.1} GB", b as f64 / 1_073_741_824.0)
}

/// Receive one file into the inbox. See `files` for the safety rules.
async fn receive_file(
    State(st): State<SharedState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Query(q): Query<FileQ>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let dir = st.cfg.inbox_dir.clone();
    let sent_name = q.name.chars().take(400).collect::<String>();
    let name = files::sanitize_name(&sent_name);
    let from: String = q
        .from
        .as_deref()
        .map(|s| s.chars().filter(|c| !c.is_control()).take(64).collect::<String>())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "another PC".into());

    if let Err(e) = tokio::fs::create_dir_all(&dir).await {
        return fail(StatusCode::INTERNAL_SERVER_ERROR, format!("couldn't create {}: {e}", dir.display()));
    }
    let declared = headers.get(header::CONTENT_LENGTH).and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok());
    let d2 = dir.clone();
    let free = tokio::task::spawn_blocking(move || files::free_space(&d2)).await.ok().flatten().unwrap_or(u64::MAX);
    let room = free.saturating_sub(files::FREE_SPACE_MARGIN);
    if let Some(size) = declared {
        if size > room {
            return fail(
                StatusCode::INSUFFICIENT_STORAGE,
                format!("not enough disk space on {}: {} needed, {} free", st.cfg.machine_name, gib(size), gib(free)),
            );
        }
    }

    let part = files::part_path(&dir);
    let mut out = match tokio::fs::File::create(&part).await {
        Ok(f) => f,
        Err(e) => return fail(StatusCode::INTERNAL_SERVER_ERROR, format!("couldn't write to {}: {e}", dir.display())),
    };
    let mut written = 0u64;
    let mut stream = body.into_data_stream();
    let copied: Result<(), Response> = async {
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| fail(StatusCode::BAD_REQUEST, format!("transfer interrupted: {e}")))?;
            written += chunk.len() as u64;
            if written > room {
                return Err(fail(StatusCode::INSUFFICIENT_STORAGE, format!("ran out of disk space on {}", st.cfg.machine_name)));
            }
            out.write_all(&chunk)
                .await
                .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, format!("write failed: {e}")))?;
        }
        out.flush().await.map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, format!("write failed: {e}")))?;
        if declared.is_some_and(|d| d != written) {
            return Err(fail(StatusCode::BAD_REQUEST, "transfer incomplete"));
        }
        Ok(())
    }
    .await;
    drop(out);
    if let Err(resp) = copied {
        let _ = tokio::fs::remove_file(&part).await;
        tracing::info!("incoming {name} from {from} ({}) not saved", peer.ip());
        return resp;
    }

    // Pick the final name and move the finished file into place (one at a time, so two
    // simultaneous transfers of the same name don't pick the same target).
    static FINALIZE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let (d3, n3, p3, from3) = (dir.clone(), name.clone(), part.clone(), from.clone());
    let placed = tokio::task::spawn_blocking(move || {
        let _g = FINALIZE.lock().unwrap();
        let dest = files::unique_path(&d3, &n3);
        std::fs::rename(&p3, &dest)?;
        files::mark_of_the_web(&dest, &from3);
        Ok::<_, std::io::Error>(dest)
    })
    .await;
    let dest = match placed {
        Ok(Ok(d)) => d,
        Ok(Err(e)) => {
            let _ = tokio::fs::remove_file(&part).await;
            return fail(StatusCode::INTERNAL_SERVER_ERROR, format!("couldn't save the file: {e}"));
        }
        Err(e) => {
            let _ = tokio::fs::remove_file(&part).await;
            return fail(StatusCode::INTERNAL_SERVER_ERROR, format!("couldn't save the file: {e}"));
        }
    };

    let rec = st.record_received(ReceivedFile {
        id: 0,
        ts: chrono::Utc::now().timestamp_millis(),
        name: dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or(name),
        sent_name,
        size: written,
        from,
        ip: peer.ip().to_string(),
        path: dest.display().to_string(),
    });
    Json(rec).into_response()
}

async fn received_files(State(st): State<SharedState>, Query(q): Query<KillsQ>) -> ApiResult<Vec<ReceivedFile>> {
    blocking(move || state::received(&st, q.limit.unwrap_or(50))).await
}

#[derive(Deserialize)]
struct ShellQ {
    cols: Option<u16>,
    rows: Option<u16>,
    from: Option<String>,
}

async fn shell_status(State(st): State<SharedState>) -> impl IntoResponse {
    Json(json!({
        "enabled": st.cfg.allow_shell(),
        "shell": crate::shell::shell_name(),
        "active": crate::shell::active(),
        "machine": st.cfg.machine_name,
    }))
}

async fn shell(
    ws: WebSocketUpgrade,
    State(st): State<SharedState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Query(q): Query<ShellQ>,
) -> Response {
    if !st.cfg.allow_shell() {
        return fail(
            StatusCode::FORBIDDEN,
            format!("Remote shell is turned off on {}. Turn it on in that PC's Settings → System.", st.cfg.machine_name),
        );
    }
    let Some(slot) = crate::shell::try_reserve() else {
        return fail(StatusCode::TOO_MANY_REQUESTS, format!("{} remote shells are already open on {}.", crate::shell::MAX_SESSIONS, st.cfg.machine_name));
    };
    let from: String = q
        .from
        .as_deref()
        .map(|s| s.chars().filter(|c| !c.is_control()).take(64).collect::<String>())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "another PC".into());
    let ip = peer.ip().to_string();
    let (cols, rows) = (q.cols.unwrap_or(120), q.rows.unwrap_or(30));
    ws.on_upgrade(move |socket| crate::shell::session(socket, st, slot, from, ip, cols, rows))
}

#[derive(Deserialize)]
struct ScreenQ {
    from: Option<String>,
}

async fn screenshot(State(st): State<SharedState>, ConnectInfo(peer): ConnectInfo<SocketAddr>, Query(q): Query<ScreenQ>) -> Response {
    if !st.cfg.allow_screen() {
        return fail(
            StatusCode::FORBIDDEN,
            format!("Screenshots are turned off on {}. Turn them on in that PC's Settings → System.", st.cfg.machine_name),
        );
    }
    let from: String = q
        .from
        .as_deref()
        .map(|s| s.chars().filter(|c| !c.is_control()).take(64).collect::<String>())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "another device".into());
    match tokio::task::spawn_blocking(crate::screen::capture).await {
        Ok(Ok(jpeg)) => {
            st.emit(&StreamEvent::ScreenViewed { ts: chrono::Utc::now().timestamp_millis(), from, ip: peer.ip().to_string() });
            ([(header::CONTENT_TYPE, "image/jpeg"), (header::CACHE_CONTROL, "no-store")], jpeg).into_response()
        }
        Ok(Err(e)) => fail(StatusCode::INTERNAL_SERVER_ERROR, e),
        Err(e) => fail(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[derive(Deserialize)]
struct AgentSubmit {
    prompt: String,
    cwd: String,
    mode: String,
    model: Option<String>,
    effort: Option<String>,
    #[serde(rename = "followUpOf")]
    follow_up_of: Option<i64>,
    from: Option<String>,
}

async fn agent_status(State(st): State<SharedState>) -> Response {
    match tokio::task::spawn_blocking(move || crate::agent::status(&st)).await {
        Ok(s) => Json(s).into_response(),
        Err(e) => fail(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn agent_tasks(State(st): State<SharedState>, Query(q): Query<KillsQ>) -> Response {
    let limit = q.limit.unwrap_or(50).min(500);
    match tokio::task::spawn_blocking(move || crate::agent::recent(&st, limit)).await {
        Ok(list) => Json(list).into_response(),
        Err(e) => fail(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn agent_task(State(st): State<SharedState>, Path(id): Path<i64>) -> Response {
    match tokio::task::spawn_blocking(move || crate::agent::get(&st, id)).await {
        Ok(Some(t)) => Json(t).into_response(),
        Ok(None) => fail(StatusCode::NOT_FOUND, "no such task"),
        Err(e) => fail(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn agent_submit(
    State(st): State<SharedState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(body): Json<AgentSubmit>,
) -> Response {
    let from: String = body
        .from
        .as_deref()
        .map(|s| s.chars().filter(|c| !c.is_control()).take(64).collect::<String>())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "another device".into());
    let n = crate::agent::NewTask {
        prompt: body.prompt,
        cwd: body.cwd,
        mode: body.mode,
        model: body.model.filter(|m| !m.is_empty()),
        effort: body.effort.filter(|e| !e.is_empty()),
        follow_up_of: body.follow_up_of,
        from,
        ip: peer.ip().to_string(),
    };
    match tokio::task::spawn_blocking(move || crate::agent::submit(&st, n)).await {
        Ok(Ok(t)) => (StatusCode::CREATED, Json(t)).into_response(),
        Ok(Err((code, msg))) => fail(StatusCode::from_u16(code).unwrap_or(StatusCode::BAD_REQUEST), msg),
        Err(e) => fail(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn agent_cancel(State(st): State<SharedState>, Path(id): Path<i64>) -> Response {
    match tokio::task::spawn_blocking(move || crate::agent::cancel(&st, id)).await {
        Ok(Ok(())) => Json(json!({"ok": true})).into_response(),
        Ok(Err(e)) => fail(StatusCode::CONFLICT, e),
        Err(e) => fail(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn shell_sessions(State(st): State<SharedState>, Query(q): Query<KillsQ>) -> ApiResult<Vec<crate::types::ShellSession>> {
    blocking(move || state::shell_sessions(&st, q.limit.unwrap_or(50))).await
}

async fn stream(ws: WebSocketUpgrade, State(st): State<SharedState>) -> Response {
    ws.on_upgrade(move |socket| stream_loop(socket, st))
}

async fn stream_loop(mut socket: WebSocket, st: SharedState) {
    let mut rx = st.events.subscribe();
    let first = serde_json::to_string(&StreamEvent::Snapshot(Box::new(st.snapshot()))).unwrap_or_default();
    if socket.send(Message::Text(first.into())).await.is_err() {
        return;
    }
    loop {
        tokio::select! {
            ev = rx.recv() => match ev {
                Ok(text) => {
                    if socket.send(Message::Text(text.to_string().into())).await.is_err() {
                        return;
                    }
                }
                // Fell behind: resync with a full snapshot.
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    let snap = serde_json::to_string(&StreamEvent::Snapshot(Box::new(st.snapshot()))).unwrap_or_default();
                    if socket.send(Message::Text(snap.into())).await.is_err() {
                        return;
                    }
                }
                Err(_) => return,
            },
            msg = socket.recv() => match msg {
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return,
                _ => {}
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lan_ranges() {
        for ok in ["127.0.0.1", "192.168.1.20", "10.0.0.5", "172.20.1.1", "100.101.102.103", "169.254.1.1", "::1", "fe80::1", "fd7a:115c:a1e0::1"] {
            assert!(is_lan(ok.parse().unwrap()), "{ok}");
        }
        for bad in ["8.8.8.8", "100.128.0.1", "172.32.0.1", "2001:4860::8888"] {
            assert!(!is_lan(bad.parse().unwrap()), "{bad}");
        }
    }
}
