//! Remote shell: an interactive PowerShell on this PC, opened from another PC's dashboard.
//!
//! - Off by default. Only this PC can turn it on (`allow_shell` in `collector.json`, set from the
//!   app's Settings); there is no API to enable it remotely. The file is re-read for every new
//!   session, so turning it off takes effect immediately for new sessions.
//! - Runs as the signed-in user, never elevated, in a Windows pseudo-console (ConPTY), so
//!   interactive programs (including `claude`) work.
//! - Every session is logged (who, from which IP, start and end) and announced on the stream,
//!   so this PC's dashboard shows a notification while someone is connected.
//! - Closing the terminal ends the shell and anything still running in it.

use crate::state::SharedState;
use crate::types::ShellSession;
use axum::extract::ws::{Message, WebSocket};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::Deserialize;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

/// At most this many remote shells at once.
pub const MAX_SESSIONS: usize = 4;
static ACTIVE: AtomicUsize = AtomicUsize::new(0);

pub fn active() -> usize {
    ACTIVE.load(Ordering::SeqCst)
}

/// PowerShell 7 if installed, else Windows PowerShell.
pub fn shell_program() -> PathBuf {
    #[cfg(windows)]
    {
        for base in [std::env::var_os("ProgramFiles"), std::env::var_os("ProgramW6432")].into_iter().flatten() {
            let p = PathBuf::from(base).join("PowerShell").join("7").join("pwsh.exe");
            if p.exists() {
                return p;
            }
        }
        let sys = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        sys.join(r"System32\WindowsPowerShell\v1.0\powershell.exe")
    }
    #[cfg(not(windows))]
    {
        PathBuf::from(std::env::var_os("SHELL").unwrap_or_else(|| "/bin/sh".into()))
    }
}

pub fn shell_name() -> String {
    shell_program().file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Control {
    Resize { cols: u16, rows: u16 },
}

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize { cols: cols.clamp(20, 500), rows: rows.clamp(5, 200), pixel_width: 0, pixel_height: 0 }
}

async fn send_text(socket: &mut WebSocket, v: serde_json::Value) {
    let _ = socket.send(Message::Text(v.to_string().into())).await;
}

/// Releases the session slot however the session ends.
pub struct Slot;
impl Drop for Slot {
    fn drop(&mut self) {
        ACTIVE.fetch_sub(1, Ordering::SeqCst);
    }
}

pub fn try_reserve() -> Option<Slot> {
    let prev = ACTIVE.fetch_add(1, Ordering::SeqCst);
    if prev >= MAX_SESSIONS {
        ACTIVE.fetch_sub(1, Ordering::SeqCst);
        return None;
    }
    Some(Slot)
}

/// Run one session over the WebSocket: binary frames carry terminal bytes both ways, text frames
/// carry control messages (`{"type":"resize","cols":…,"rows":…}` in, `{"type":"exit"}` /
/// `{"type":"error","message":…}` out).
pub async fn session(mut socket: WebSocket, st: SharedState, slot: Slot, from: String, ip: String, cols: u16, rows: u16) {
    let _slot = slot;
    let pair = match native_pty_system().openpty(size(cols, rows)) {
        Ok(p) => p,
        Err(e) => return send_text(&mut socket, serde_json::json!({"type": "error", "message": format!("couldn't open a console: {e}")})).await,
    };
    let program = shell_program();
    let mut cmd = CommandBuilder::new(&program);
    cmd.arg("-NoLogo");
    if let Some(home) = directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf()) {
        cmd.cwd(home);
    }
    cmd.env("CLAUDE_FLEET_REMOTE_FROM", &from);
    let mut child = match pair.slave.spawn_command(cmd) {
        Ok(c) => c,
        Err(e) => return send_text(&mut socket, serde_json::json!({"type": "error", "message": format!("couldn't start {}: {e}", program.display())})).await,
    };
    drop(pair.slave);
    let mut killer = child.clone_killer();
    let (mut reader, mut writer) = match (pair.master.try_clone_reader(), pair.master.take_writer()) {
        (Ok(r), Ok(w)) => (r, w),
        _ => {
            let _ = killer.kill();
            return send_text(&mut socket, serde_json::json!({"type": "error", "message": "couldn't attach to the console"})).await;
        }
    };

    // Console output → WebSocket.
    let (out_tx, mut out_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);
    std::thread::spawn(move || {
        let mut buf = [0u8; 16 * 1024];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if out_tx.blocking_send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    // WebSocket → console input.
    let (in_tx, in_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        while let Ok(b) = in_rx.recv() {
            if writer.write_all(&b).and_then(|_| writer.flush()).is_err() {
                break;
            }
        }
    });
    // The console doesn't report EOF when the shell exits, so wait for the process itself.
    let (exit_tx, mut exit_rx) = tokio::sync::oneshot::channel::<Option<u32>>();
    std::thread::spawn(move || {
        let code = child.wait().ok().map(|s| s.exit_code());
        let _ = exit_tx.send(code);
    });

    let mut log = ShellSession {
        id: 0,
        started: chrono::Utc::now().timestamp_millis(),
        ended: None,
        from: from.clone(),
        ip: ip.clone(),
        shell: shell_name(),
    };
    log = st.record_shell(log);

    let exit_code = loop {
        tokio::select! {
            out = out_rx.recv() => match out {
                Some(bytes) => {
                    if socket.send(Message::Binary(bytes.into())).await.is_err() {
                        break None;
                    }
                }
                None => break None,
            },
            code = &mut exit_rx => {
                // Flush what the shell printed last.
                while let Ok(bytes) = out_rx.try_recv() {
                    let _ = socket.send(Message::Binary(bytes.into())).await;
                }
                break Some(code.ok().flatten());
            }
            msg = socket.recv() => match msg {
                Some(Ok(Message::Binary(b))) => {
                    let _ = in_tx.send(b.to_vec());
                }
                Some(Ok(Message::Text(t))) => {
                    if let Ok(Control::Resize { cols, rows }) = serde_json::from_str::<Control>(&t) {
                        let _ = pair.master.resize(size(cols, rows));
                    }
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break None,
                _ => {}
            },
        }
    };

    match exit_code {
        Some(code) => send_text(&mut socket, serde_json::json!({"type": "exit", "code": code})).await,
        None => {
            let _ = killer.kill();
        }
    }
    let _ = socket.send(Message::Close(None)).await;
    drop(in_tx);
    // Closing the pseudo-console also ends programs still attached to it.
    drop(pair.master);
    log.ended = Some(chrono::Utc::now().timestamp_millis());
    st.record_shell(log);
}
