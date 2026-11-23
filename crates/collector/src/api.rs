//! Versioned HTTP + WebSocket API.
//!
//! Every request needs the pairing token, as `Authorization: Bearer <token>`,
//! `X-Fleet-Token: <token>`, or `?token=<token>` (for WebSockets, which can't set headers
//! from a browser). Requests from outside the LAN are refused before the token is checked:
//! only loopback, private (RFC 1918), link-local and Tailscale (100.64.0.0/10) addresses pass.

use crate::state::{self, SharedState};
use crate::types::StreamEvent;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, Path, Query, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use std::net::{IpAddr, SocketAddr};
use subtle::ConstantTimeEq;

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
        .route("/kills", get(kill_log));
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
