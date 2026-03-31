mod auth_flow;
mod config;
mod entities;
mod error;
mod models;
mod request_stats;
mod routes;
mod state;

use anyhow::Context;
use clap::Parser;
use config::Args;
use routes::build_router;
use state::AppState;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "codex_proxy=info".into()),
        )
        .init();

    let config = Args::parse().into_config()?;
    info!(
        bind = %config.bind,
        data_dir = %config.data_dir.display(),
        database_url = %config.database_url,
        "starting codex proxy",
    );
    info!(
        admin_password_auth = true,
        admin_key_auth = true,
        "using admin authentication (password sessions + user-managed admin keys)",
    );

    let state = AppState::new(config.clone()).await?;
    let listener = tokio::net::TcpListener::bind(config.bind)
        .await
        .with_context(|| format!("failed to bind {}", config.bind))?;

    axum::serve(listener, build_router(state))
        .await
        .context("axum server exited unexpectedly")?;

    Ok(())
}
