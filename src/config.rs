use clap::Parser;
use codex_login::CLIENT_ID;
use sha2::Digest;
use sha2::Sha256;
use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;
use url::Host;
use url::Url;

pub const DEFAULT_CHATGPT_BASE_URL: &str = "https://chatgpt.com/backend-api/codex";
pub const DEFAULT_AUTH_ISSUER: &str = "https://auth.openai.com";
pub const DEFAULT_AUTH_CALLBACK_URL: &str = "http://localhost:1455/auth/callback";
pub const DEFAULT_DATA_DIR: &str = ".codex-proxy";
const APP_DATA_DIR_NAME: &str = "codex-proxy";

#[derive(Debug, Clone, Parser)]
#[command(
    name = "codex-proxy",
    about = "Load-balanced proxy for multiple Codex credentials"
)]
pub struct Args {
    #[arg(long, env = "CODEX_PROXY_BIND", default_value = "127.0.0.1:8787")]
    pub bind: SocketAddr,

    #[arg(long, env = "CODEX_PROXY_DATA_DIR", default_value = DEFAULT_DATA_DIR)]
    pub data_dir: PathBuf,

    #[arg(long, env = "CODEX_PROXY_DATABASE_URL")]
    pub database_url: Option<String>,

    #[arg(long, env = "CODEX_PROXY_ADMIN_PASSWORD")]
    pub admin_password: String,

    #[arg(
        long,
        env = "CODEX_PROXY_CHATGPT_BASE_URL",
        default_value = DEFAULT_CHATGPT_BASE_URL
    )]
    pub chatgpt_base_url: String,

    #[arg(
        long,
        env = "CODEX_PROXY_AUTH_ISSUER",
        default_value = DEFAULT_AUTH_ISSUER
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
        default_value = DEFAULT_AUTH_CALLBACK_URL
    )]
    pub auth_callback_url: String,

    #[arg(long, env = "CODEX_PROXY_PUBLIC_BASE_URL")]
    pub public_base_url: Option<String>,

    #[arg(long, env = "CODEX_PROXY_FORCED_CHATGPT_WORKSPACE_ID")]
    pub forced_chatgpt_workspace_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub bind: SocketAddr,
    pub data_dir: PathBuf,
    pub database_url: String,
    pub admin_password_hash: String,
    pub chatgpt_base_url: String,
    pub auth_issuer: String,
    pub auth_client_id: String,
    pub auth_callback_url: String,
    pub public_base_url: Option<String>,
    pub forced_chatgpt_workspace_id: Option<String>,
}

impl Args {
    pub fn into_config(self) -> std::io::Result<AppConfig> {
        let data_dir = resolve_data_dir(self.data_dir)?;
        let database_url = normalize_optional_string(self.database_url).unwrap_or_else(|| {
            let db_path = data_dir.join("codex-proxy.sqlite");
            format!("sqlite://{}?mode=rwc", db_path.display())
        });
        let auth_callback_url =
            normalize_string_or_default(self.auth_callback_url, DEFAULT_AUTH_CALLBACK_URL);
        validate_auth_callback_url(
            &auth_callback_url,
            "CODEX_PROXY_AUTH_CALLBACK_URL / --auth-callback-url",
        )?;
        Ok(AppConfig {
            bind: self.bind,
            data_dir,
            database_url,
            admin_password_hash: hash_secret(&self.admin_password),
            chatgpt_base_url: normalize_string_or_default(
                self.chatgpt_base_url,
                DEFAULT_CHATGPT_BASE_URL,
            ),
            auth_issuer: normalize_string_or_default(self.auth_issuer, DEFAULT_AUTH_ISSUER),
            auth_client_id: normalize_string_or_default(self.auth_client_id, CLIENT_ID),
            auth_callback_url,
            public_base_url: normalize_optional_string(self.public_base_url),
            forced_chatgpt_workspace_id: normalize_optional_string(
                self.forced_chatgpt_workspace_id,
            ),
        })
    }
}

fn resolve_data_dir(data_dir: PathBuf) -> io::Result<PathBuf> {
    if data_dir.is_absolute() {
        return Ok(data_dir);
    }

    let cwd_data_dir = std::env::current_dir()?.join(&data_dir);
    if data_dir == PathBuf::from(DEFAULT_DATA_DIR) {
        if cwd_data_dir.exists() {
            return Ok(cwd_data_dir);
        }

        if let Some(default_data_dir) = preferred_default_data_dir() {
            return Ok(default_data_dir);
        }
    }

    Ok(cwd_data_dir)
}

fn preferred_default_data_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .or_else(|| std::env::var_os("APPDATA"))
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .map(|root| root.join(APP_DATA_DIR_NAME))
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .map(|root| {
                root.join("Library")
                    .join("Application Support")
                    .join(APP_DATA_DIR_NAME)
            })
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .map(|root| root.join(APP_DATA_DIR_NAME))
            .or_else(|| {
                std::env::var_os("HOME")
                    .filter(|value| !value.is_empty())
                    .map(PathBuf::from)
                    .map(|root| root.join(".local").join("share").join(APP_DATA_DIR_NAME))
            })
    }
}

fn normalize_string_or_default(value: String, default_value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        default_value.to_string()
    } else {
        trimmed.to_string()
    }
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn validate_auth_callback_url(value: &str, field_name: &str) -> io::Result<()> {
    let parsed = Url::parse(value).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid {field_name}: {err}"),
        )
    })?;

    if parsed.scheme() != "http" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "invalid {field_name}: browser auth callback URL must use http and point to a loopback host like {DEFAULT_AUTH_CALLBACK_URL}"
            ),
        ));
    }

    let is_loopback = match parsed.host() {
        Some(Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(addr)) => addr.is_loopback(),
        Some(Host::Ipv6(addr)) => addr.is_loopback(),
        None => false,
    };
    if !is_loopback {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "invalid {field_name}: browser auth callback URL must point to a loopback host like {DEFAULT_AUTH_CALLBACK_URL} so it matches the official Codex browser login flow"
            ),
        ));
    }

    if parsed.port().is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "invalid {field_name}: browser auth callback URL must include an explicit localhost port like {DEFAULT_AUTH_CALLBACK_URL}"
            ),
        ));
    }

    Ok(())
}

fn hash_secret(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn into_config_trims_auth_strings_and_drops_blank_optionals() {
        let args = Args {
            bind: "127.0.0.1:8787".parse().expect("bind should parse"),
            data_dir: PathBuf::from("/tmp/codex-proxy"),
            database_url: Some("   ".to_string()),
            admin_password: "secret".to_string(),
            chatgpt_base_url: "  https://chatgpt.com/backend-api/codex  ".to_string(),
            auth_issuer: "  https://auth.openai.com/  ".to_string(),
            auth_client_id: format!("  {CLIENT_ID}  "),
            auth_callback_url: "  http://localhost:1455/auth/callback  ".to_string(),
            public_base_url: Some("   ".to_string()),
            forced_chatgpt_workspace_id: Some("   ".to_string()),
        };

        let config = args.into_config().expect("config should build");

        assert!(
            config
                .database_url
                .starts_with("sqlite:///tmp/codex-proxy/codex-proxy.sqlite"),
            "expected default sqlite path, got {}",
            config.database_url
        );
        assert_eq!(config.chatgpt_base_url, DEFAULT_CHATGPT_BASE_URL);
        assert_eq!(config.auth_issuer, "https://auth.openai.com/");
        assert_eq!(config.auth_client_id, CLIENT_ID);
        assert_eq!(config.auth_callback_url, DEFAULT_AUTH_CALLBACK_URL);
        assert_eq!(config.public_base_url, None);
        assert_eq!(config.forced_chatgpt_workspace_id, None);
    }

    #[test]
    fn into_config_rejects_invalid_auth_callback_url() {
        let args = Args {
            bind: "127.0.0.1:8787".parse().expect("bind should parse"),
            data_dir: PathBuf::from("/tmp/codex-proxy"),
            database_url: None,
            admin_password: "secret".to_string(),
            chatgpt_base_url: DEFAULT_CHATGPT_BASE_URL.to_string(),
            auth_issuer: DEFAULT_AUTH_ISSUER.to_string(),
            auth_client_id: CLIENT_ID.to_string(),
            auth_callback_url: "not a url".to_string(),
            public_base_url: None,
            forced_chatgpt_workspace_id: None,
        };

        let err = args
            .into_config()
            .expect_err("invalid callback URL should be rejected");

        assert!(
            err.to_string()
                .contains("CODEX_PROXY_AUTH_CALLBACK_URL / --auth-callback-url"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn into_config_resolves_custom_relative_data_dir_from_cwd() {
        let args = Args {
            bind: "127.0.0.1:8787".parse().expect("bind should parse"),
            data_dir: PathBuf::from("test-data"),
            database_url: None,
            admin_password: "secret".to_string(),
            chatgpt_base_url: DEFAULT_CHATGPT_BASE_URL.to_string(),
            auth_issuer: DEFAULT_AUTH_ISSUER.to_string(),
            auth_client_id: CLIENT_ID.to_string(),
            auth_callback_url: DEFAULT_AUTH_CALLBACK_URL.to_string(),
            public_base_url: None,
            forced_chatgpt_workspace_id: None,
        };

        let config = args.into_config().expect("config should build");

        assert_eq!(
            config.data_dir,
            std::env::current_dir()
                .expect("cwd should resolve")
                .join("test-data")
        );
    }

    #[test]
    fn into_config_rejects_non_loopback_auth_callback_url() {
        let args = Args {
            bind: "127.0.0.1:8787".parse().expect("bind should parse"),
            data_dir: PathBuf::from("/tmp/codex-proxy"),
            database_url: None,
            admin_password: "secret".to_string(),
            chatgpt_base_url: DEFAULT_CHATGPT_BASE_URL.to_string(),
            auth_issuer: DEFAULT_AUTH_ISSUER.to_string(),
            auth_client_id: CLIENT_ID.to_string(),
            auth_callback_url: "https://proxy.example.com/auth/callback".to_string(),
            public_base_url: None,
            forced_chatgpt_workspace_id: None,
        };

        let err = args
            .into_config()
            .expect_err("non-loopback callback URL should be rejected");

        assert!(
            err.to_string().contains("loopback host"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn into_config_accepts_loopback_ip_auth_callback_url() {
        let args = Args {
            bind: "127.0.0.1:8787".parse().expect("bind should parse"),
            data_dir: PathBuf::from("/tmp/codex-proxy"),
            database_url: None,
            admin_password: "secret".to_string(),
            chatgpt_base_url: DEFAULT_CHATGPT_BASE_URL.to_string(),
            auth_issuer: DEFAULT_AUTH_ISSUER.to_string(),
            auth_client_id: CLIENT_ID.to_string(),
            auth_callback_url: "http://127.0.0.1:1455/auth/callback".to_string(),
            public_base_url: None,
            forced_chatgpt_workspace_id: None,
        };

        let config = args
            .into_config()
            .expect("loopback callback URL should be accepted");

        assert_eq!(
            config.auth_callback_url,
            "http://127.0.0.1:1455/auth/callback"
        );
    }
}
