use clap::{Parser, Subcommand};
use fleet_collector::{claude, config, db, statusline, CollectorConfig, Overrides};
use std::path::PathBuf;

/// Claude Fleet collector: system metrics + Claude Code usage, served on the LAN.
#[derive(Parser)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
    /// Data directory (default: %APPDATA%\ClaudeFleet). Use a different one to run a second collector.
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,
    /// Claude Code config directory (default: %USERPROFILE%\.claude).
    #[arg(long, global = true)]
    claude_dir: Option<PathBuf>,
    /// API port (saved for next time).
    #[arg(long, global = true)]
    port: Option<u16>,
    /// Machine name shown on the dashboard (saved for next time).
    #[arg(long, global = true)]
    name: Option<String>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the collector and API (default).
    Run,
    /// Print this machine's address details and pairing token.
    Token,
    /// Read the Claude logs once and print a summary.
    Scan,
    /// Merge the Claude Fleet status line into Claude Code's settings.json (backs it up first).
    InstallStatusline,
    /// Remove the Claude Fleet status line (restores the previous one, if any).
    UninstallStatusline,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,tower_http=warn".into()),
        )
        .init();

    let cli = Cli::parse();
    let cfg = CollectorConfig::load_or_create(&Overrides {
        data_dir: cli.data_dir,
        claude_dir: cli.claude_dir,
        port: cli.port,
        machine_name: cli.name,
    })?;

    match cli.cmd.unwrap_or(Cmd::Run) {
        Cmd::Run => {
            tracing::info!(
                "{} · data {} · claude {}",
                cfg.machine_name,
                cfg.data_dir.display(),
                cfg.claude_dir.display()
            );
            fleet_collector::run(cfg).await?;
        }
        Cmd::Token => {
            println!("machine: {}", cfg.machine_name);
            println!("port:    {}", cfg.port);
            println!("token:   {}", cfg.token);
        }
        Cmd::Scan => {
            let mut conn = db::open(&cfg.db_path())?;
            let t = std::time::Instant::now();
            let s = claude::scan(&mut conn, &cfg.claude_dir)?;
            println!(
                "scanned {} files ({} read, {:.1} MB) in {:.2?}: +{} prompts, +{} responses, {} bad lines",
                s.files_seen,
                s.files_read,
                s.bytes_read as f64 / 1_048_576.0,
                t.elapsed(),
                s.new_prompts,
                s.new_messages,
                s.bad_lines
            );
        }
        Cmd::InstallStatusline => {
            let r = statusline::install(&cfg)?;
            if r.already {
                println!("Already installed in {}", r.settings.display());
            } else {
                println!("Installed in {}", r.settings.display());
                if let Some(b) = r.backup {
                    println!("Backup: {}", b.display());
                }
                if let Some(c) = r.chained {
                    println!("Your previous status line still runs: {c}");
                }
            }
        }
        Cmd::UninstallStatusline => match statusline::uninstall(&cfg)? {
            Some(b) => println!("Removed. Backup: {}", b.display()),
            None => println!("Claude Fleet status line was not installed."),
        },
    }
    let _ = config::API_VERSION;
    Ok(())
}
