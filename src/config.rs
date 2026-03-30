use clap::Parser;
use codex_login::CLIENT_ID;
use rand::Rng;
use rand::distr::Alphanumeric;
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Debug, Clone, Parser)]
#[command(
    name = "codex-proxy",
    about = "Load-balanced proxy for multiple Codex credentials"
)]
pub struct Args {
    #[arg(long, env = "CODEX_PROXY_BIND", default_value = "127.0.0.1:8787")]
    pub bind: SocketAddr,

    #[arg(long, env = "CODEX_PROXY_DATA_DIR", default_value = ".codex-proxy")]
    pub data_dir: PathBuf,

    #[arg(long, env = "CODEX_PROXY_DATABASE_URL")]
    pub database_url: Option<String>,

    #[arg(long, env = "CODEX_PROXY_ADMIN_TOKEN")]
    pub admin_token: Option<String>,

    #[arg(
        long,
        env = "CODEX_PROXY_CHATGPT_BASE_URL",
        default_value = "https://chatgpt.com/backend-api/codex"
    )]
    pub chatgpt_base_url: String,

    #[arg(
        long,
        env = "CODEX_PROXY_AUTH_ISSUER",
        default_value = "https://auth.openai.com"
    )]
    pub auth_issuer: String,

    #[arg(
        long,
        env = "CODEX_PROXY_AUTH_CLIENT_ID",
        default_value = CLIENT_ID
    )]
    pub auth_client_id: String,

    #[arg(
        long,
        env = "CODEX_PROXY_AUTH_CALLBACK_URL",
        default_value = "http://localhost:1455/auth/callback"
    )]
    pub auth_callback_url: String,

    #[arg(long, env = "CODEX_PROXY_FORCED_CHATGPT_WORKSPACE_ID")]
    pub forced_chatgpt_workspace_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub bind: SocketAddr,
    pub data_dir: PathBuf,
    pub database_url: String,
    pub admin_token: String,
    pub chatgpt_base_url: String,
    pub auth_issuer: String,
    pub auth_client_id: String,
    pub auth_callback_url: String,
    pub forced_chatgpt_workspace_id: Option<String>,
}

impl Args {
    pub fn into_config(self) -> std::io::Result<AppConfig> {
        let data_dir = if self.data_dir.is_absolute() {
            self.data_dir
        } else {
            std::env::current_dir()?.join(self.data_dir)
        };
        let database_url = self.database_url.unwrap_or_else(|| {
            let db_path = data_dir.join("codex-proxy.sqlite");
            format!("sqlite://{}?mode=rwc", db_path.display())
        });
        let admin_token = self.admin_token.unwrap_or_else(generate_admin_token);

        Ok(AppConfig {
            bind: self.bind,
            data_dir,
            database_url,
            admin_token,
            chatgpt_base_url: self.chatgpt_base_url,
            auth_issuer: self.auth_issuer,
            auth_client_id: self.auth_client_id,
            auth_callback_url: self.auth_callback_url,
            forced_chatgpt_workspace_id: self.forced_chatgpt_workspace_id,
        })
    }
}

fn generate_admin_token() -> String {
    let suffix = rand::rng()
        .sample_iter(Alphanumeric)
        .take(32)
        .map(char::from)
        .collect::<String>();
    format!("cpa_{suffix}")
}
