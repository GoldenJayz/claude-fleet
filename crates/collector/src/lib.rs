//! Claude Fleet collector: a plain background program (no AI, no cloud calls) that samples
//! system metrics, reads Claude Code's local logs, keeps history in SQLite and serves a
//! token-protected API on the LAN. Used standalone (`fleet-collector`) or embedded in the app.

pub mod api;
pub mod claude;
pub mod config;
pub mod db;
pub mod files;
pub mod gpu;
pub mod metrics;
pub mod procs;
pub mod shell;
pub mod state;
pub mod statusline;
pub mod types;
pub mod usage;

pub use config::{CollectorConfig, Overrides};
pub use state::{Collector, SharedState};

/// Start everything, including the HTTP server, and run until the server stops.
pub async fn run(cfg: CollectorConfig) -> anyhow::Result<()> {
    let c = Collector::start(cfg).await?;
    api::serve(c.state.clone()).await
}
