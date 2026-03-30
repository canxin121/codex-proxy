use crate::config::AppConfig;
use crate::entities::api_key;
use crate::entities::credential;
use crate::entities::credential_limit;
use crate::error::AppError;
use crate::models::CredentialKind;
use chrono::DateTime;
use chrono::Utc;
use codex_api::rate_limits::parse_all_rate_limits;
use codex_login::AuthCredentialsStoreMode;
use codex_login::AuthManager;
use codex_login::CodexAuth;
use codex_login::load_auth_dot_json;
use http::HeaderMap;
use sea_orm::ActiveModelTrait;
use sea_orm::ColumnTrait;
use sea_orm::ConnectionTrait;
use sea_orm::Database;
use sea_orm::DatabaseConnection;
use sea_orm::DbBackend;
use sea_orm::EntityTrait;
use sea_orm::QueryFilter;
use sea_orm::QueryOrder;
use sea_orm::Set;
use sea_orm::Statement;
use sha2::Digest;
use sha2::Sha256;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::RwLock;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct AppState {
    inner: Arc<AppStateInner>,
}

struct AppStateInner {
    config: AppConfig,
    db: DatabaseConnection,
    http_client: reqwest::Client,
    auth_managers: RwLock<HashMap<String, Arc<AuthManager>>>,
    active_requests: Mutex<HashMap<String, usize>>,
    auth_cancellations: Mutex<HashMap<String, CancellationToken>>,
}

#[derive(Clone)]
pub struct AuthenticatedPrincipal {
    pub principal_kind: AuthenticatedPrincipalKind,
    pub api_key_id: Option<String>,
    pub api_key_name: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthenticatedPrincipalKind {
    AdminToken,
    ApiKey,
}

impl AuthenticatedPrincipalKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AdminToken => "admin_token",
            Self::ApiKey => "api_key",
        }
    }
}

pub struct SelectedCredential {
    pub model: credential::Model,
}

pub struct RequestLease {
    state: AppState,
    credential_id: String,
}

impl Drop for RequestLease {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.state.inner.active_requests.lock()
            && let Some(count) = guard.get_mut(&self.credential_id)
        {
            if *count <= 1 {
                guard.remove(&self.credential_id);
            } else {
                *count -= 1;
            }
        }
    }
}

impl AppState {
    pub async fn new(config: AppConfig) -> Result<Self, AppError> {
        std::fs::create_dir_all(config.data_dir.join("credentials"))?;

        let db = Database::connect(config.database_url.as_str())
            .await
            .map_err(|err| AppError::internal(err.to_string()))?;
        initialize_database(&db).await?;
        recover_auth_sessions(&db).await?;

        let http_client = reqwest::Client::builder()
            .build()
            .map_err(|err| AppError::internal(err.to_string()))?;

        Ok(Self {
            inner: Arc::new(AppStateInner {
                config,
                db,
                http_client,
                auth_managers: RwLock::new(HashMap::new()),
                active_requests: Mutex::new(HashMap::new()),
                auth_cancellations: Mutex::new(HashMap::new()),
            }),
        })
    }

    pub fn config(&self) -> &AppConfig {
        &self.inner.config
    }

    pub fn db(&self) -> &DatabaseConnection {
        &self.inner.db
    }

    pub fn http_client(&self) -> &reqwest::Client {
        &self.inner.http_client
    }

    pub fn credential_home(&self, credential_id: &str) -> PathBuf {
        self.config()
            .data_dir
            .join("credentials")
            .join(credential_id)
    }

    pub fn provider_base_url(&self, model: &credential::Model) -> String {
        model.upstream_base_url.clone().unwrap_or_else(|| {
            match CredentialKind::from_str(&model.kind) {
                Some(CredentialKind::ChatgptAuth) | None => self.config().chatgpt_base_url.clone(),
            }
        })
    }

    pub fn active_requests_for(&self, credential_id: &str) -> usize {
        self.inner
            .active_requests
            .lock()
            .ok()
            .and_then(|guard| guard.get(credential_id).copied())
            .unwrap_or(0)
    }

    pub fn active_requests_total(&self) -> usize {
        self.inner
            .active_requests
            .lock()
            .ok()
            .map(|guard| guard.values().copied().sum())
            .unwrap_or(0)
    }

    pub fn acquire_request_lease(&self, credential_id: impl Into<String>) -> RequestLease {
        let credential_id = credential_id.into();
        if let Ok(mut guard) = self.inner.active_requests.lock() {
            let entry = guard.entry(credential_id.clone()).or_insert(0);
            *entry += 1;
        }
        RequestLease {
            state: self.clone(),
            credential_id,
        }
    }

    pub fn invalidate_auth_manager(&self, credential_id: &str) {
        if let Ok(mut guard) = self.inner.auth_managers.write() {
            guard.remove(credential_id);
        }
    }

    pub fn register_auth_cancellation(
        &self,
        auth_session_id: impl Into<String>,
        cancellation: CancellationToken,
    ) {
        if let Ok(mut guard) = self.inner.auth_cancellations.lock() {
            guard.insert(auth_session_id.into(), cancellation);
        }
    }

    pub fn take_auth_cancellation(&self, auth_session_id: &str) -> Option<CancellationToken> {
        self.inner
            .auth_cancellations
            .lock()
            .ok()
            .and_then(|mut guard| guard.remove(auth_session_id))
    }

    pub fn clear_auth_cancellation(&self, auth_session_id: &str) {
        if let Ok(mut guard) = self.inner.auth_cancellations.lock() {
            guard.remove(auth_session_id);
        }
    }

    pub async fn auth_manager(&self, credential_id: &str) -> Arc<AuthManager> {
        if let Ok(guard) = self.inner.auth_managers.read()
            && let Some(manager) = guard.get(credential_id)
        {
            return Arc::clone(manager);
        }

        let manager = AuthManager::shared(
            self.credential_home(credential_id),
            /* enable_codex_api_key_env */ false,
            AuthCredentialsStoreMode::File,
        );

        if let Ok(mut guard) = self.inner.auth_managers.write() {
            guard.insert(credential_id.to_string(), Arc::clone(&manager));
        }

        manager
    }

    pub async fn authenticate_api_key(
        &self,
        bearer: &str,
        require_admin: bool,
    ) -> Result<AuthenticatedPrincipal, AppError> {
        if bearer == self.config().admin_token {
            return Ok(AuthenticatedPrincipal {
                principal_kind: AuthenticatedPrincipalKind::AdminToken,
                api_key_id: None,
                api_key_name: None,
            });
        }

        let key_hash = hash_api_key(bearer);
        let record = api_key::Entity::find()
            .filter(api_key::Column::KeyHash.eq(key_hash))
            .one(self.db())
            .await
            .map_err(|err| AppError::internal(err.to_string()))?
            .ok_or_else(|| AppError::unauthorized("invalid api key"))?;

        if !record.enabled {
            return Err(AppError::forbidden("api key is disabled"));
        }
        if let Some(expires_at) = record.expires_at
            && expires_at < Utc::now()
        {
            return Err(AppError::forbidden("api key has expired"));
        }
        if require_admin && !record.is_admin {
            return Err(AppError::forbidden("admin privileges required"));
        }

        let api_key_id = record.id.clone();
        let api_key_name = record.name.clone();
        let mut active = api_key::ActiveModel::from(record);
        active.last_used_at = Set(Some(Utc::now()));
        active.updated_at = Set(Utc::now());
        active
            .update(self.db())
            .await
            .map_err(|err| AppError::internal(err.to_string()))?;

        Ok(AuthenticatedPrincipal {
            principal_kind: AuthenticatedPrincipalKind::ApiKey,
            api_key_id: Some(api_key_id),
            api_key_name: Some(api_key_name),
        })
    }

    pub async fn select_credential(
        &self,
        preferred_id: Option<&str>,
    ) -> Result<SelectedCredential, AppError> {
        if let Some(preferred_id) = preferred_id {
            let model = credential::Entity::find_by_id(preferred_id.to_string())
                .one(self.db())
                .await
                .map_err(|err| AppError::internal(err.to_string()))?
                .ok_or_else(|| AppError::not_found("credential not found"))?;
            if !model.enabled {
                return Err(AppError::forbidden("credential is disabled"));
            }
            if !self.credential_has_active_auth(&model.id).await {
                return Err(AppError::service_unavailable(
                    "credential auth is not configured",
                ));
            }
            return Ok(SelectedCredential { model });
        }

        let credentials = credential::Entity::find()
            .filter(credential::Column::Enabled.eq(true))
            .order_by_asc(credential::Column::CreatedAt)
            .all(self.db())
            .await
            .map_err(|err| AppError::internal(err.to_string()))?;

        if credentials.is_empty() {
            return Err(AppError::service_unavailable(
                "no enabled Codex credentials are available",
            ));
        }

        let limit_rows = credential_limit::Entity::find()
            .all(self.db())
            .await
            .map_err(|err| AppError::internal(err.to_string()))?;
        let mut limits_by_credential: HashMap<String, Vec<credential_limit::Model>> =
            HashMap::new();
        for row in limit_rows {
            limits_by_credential
                .entry(row.credential_id.clone())
                .or_default()
                .push(row);
        }

        let mut best: Option<(f64, credential::Model)> = None;
        for model in credentials {
            if !self.credential_has_active_auth(&model.id).await {
                continue;
            }
            let score = score_credential(
                &model,
                limits_by_credential
                    .get(&model.id)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]),
                self.active_requests_for(&model.id),
            );

            match best.as_ref() {
                Some((best_score, _)) if *best_score >= score => {}
                _ => best = Some((score, model)),
            }
        }

        let (_, model) = best.ok_or_else(|| {
            AppError::service_unavailable(
                "no enabled authenticated Codex credentials are available",
            )
        })?;
        Ok(SelectedCredential { model })
    }

    async fn credential_has_active_auth(&self, credential_id: &str) -> bool {
        self.auth_manager(credential_id)
            .await
            .auth()
            .await
            .is_some()
    }

    pub async fn sync_credential_from_auth(
        &self,
        credential_id: &str,
    ) -> Result<credential::Model, AppError> {
        let existing = credential::Entity::find_by_id(credential_id.to_string())
            .one(self.db())
            .await
            .map_err(|err| AppError::internal(err.to_string()))?
            .ok_or_else(|| AppError::not_found("credential not found"))?;

        let auth = self
            .auth_manager(credential_id)
            .await
            .auth()
            .await
            .ok_or_else(|| AppError::service_unavailable("credential auth is not available"))?;

        let auth_dot_json = load_auth_dot_json(
            &self.credential_home(credential_id),
            AuthCredentialsStoreMode::File,
        )
        .map_err(|err| AppError::internal(err.to_string()))?;

        let token_data = auth.get_token_data().ok();
        let account_id = auth.get_account_id();
        let account_email = auth.get_account_email();
        let plan_type = token_data
            .as_ref()
            .and_then(|tokens| tokens.id_token.get_chatgpt_plan_type_raw());
        let last_refresh_at = auth_dot_json.and_then(|payload| payload.last_refresh);

        let mut active = credential::ActiveModel::from(existing);
        active.account_id = Set(account_id);
        active.account_email = Set(account_email);
        active.plan_type = Set(plan_type);
        active.last_refresh_at = Set(last_refresh_at);
        active.updated_at = Set(Utc::now());

        active
            .update(self.db())
            .await
            .map_err(|err| AppError::internal(err.to_string()))
    }

    pub async fn record_credential_touch(&self, credential_id: &str) -> Result<(), AppError> {
        let existing = credential::Entity::find_by_id(credential_id.to_string())
            .one(self.db())
            .await
            .map_err(|err| AppError::internal(err.to_string()))?
            .ok_or_else(|| AppError::not_found("credential not found"))?;
        let mut active = credential::ActiveModel::from(existing);
        active.last_used_at = Set(Some(Utc::now()));
        active.updated_at = Set(Utc::now());
        active
            .update(self.db())
            .await
            .map_err(|err| AppError::internal(err.to_string()))?;
        Ok(())
    }

    pub async fn record_credential_error(
        &self,
        credential_id: &str,
        message: impl Into<String>,
    ) -> Result<(), AppError> {
        let existing = credential::Entity::find_by_id(credential_id.to_string())
            .one(self.db())
            .await
            .map_err(|err| AppError::internal(err.to_string()))?
            .ok_or_else(|| AppError::not_found("credential not found"))?;
        let mut active = credential::ActiveModel::from(existing);
        active.last_error = Set(Some(message.into()));
        active.failure_count = Set(active.failure_count.take().unwrap_or(0) + 1);
        active.updated_at = Set(Utc::now());
        active
            .update(self.db())
            .await
            .map_err(|err| AppError::internal(err.to_string()))?;
        Ok(())
    }

    pub async fn clear_credential_error(&self, credential_id: &str) -> Result<(), AppError> {
        let existing = credential::Entity::find_by_id(credential_id.to_string())
            .one(self.db())
            .await
            .map_err(|err| AppError::internal(err.to_string()))?
            .ok_or_else(|| AppError::not_found("credential not found"))?;
        let mut active = credential::ActiveModel::from(existing);
        active.last_error = Set(None);
        active.failure_count = Set(0);
        active.updated_at = Set(Utc::now());
        active
            .update(self.db())
            .await
            .map_err(|err| AppError::internal(err.to_string()))?;
        Ok(())
    }

    pub async fn update_rate_limits_from_headers(
        &self,
        credential_id: &str,
        headers: &HeaderMap,
    ) -> Result<(), AppError> {
        let snapshots = parse_all_rate_limits(headers);
        for snapshot in snapshots {
            self.persist_rate_limit_snapshot(credential_id, snapshot)
                .await?;
        }
        if !headers.is_empty() {
            let existing = credential::Entity::find_by_id(credential_id.to_string())
                .one(self.db())
                .await
                .map_err(|err| AppError::internal(err.to_string()))?
                .ok_or_else(|| AppError::not_found("credential not found"))?;
            let mut active = credential::ActiveModel::from(existing);
            active.last_limit_sync_at = Set(Some(Utc::now()));
            active.updated_at = Set(Utc::now());
            active
                .update(self.db())
                .await
                .map_err(|err| AppError::internal(err.to_string()))?;
        }
        Ok(())
    }

    async fn persist_rate_limit_snapshot(
        &self,
        credential_id: &str,
        snapshot: codex_protocol::protocol::RateLimitSnapshot,
    ) -> Result<(), AppError> {
        let limit_id = snapshot
            .limit_id
            .clone()
            .unwrap_or_else(|| "codex".to_string());
        let row_id = format!("{credential_id}:{limit_id}");
        let existing = credential_limit::Entity::find_by_id(row_id.clone())
            .one(self.db())
            .await
            .map_err(|err| AppError::internal(err.to_string()))?;

        let primary = snapshot.primary;
        let secondary = snapshot.secondary;
        let credits = snapshot.credits;
        let now = Utc::now();

        let mut active = existing
            .map(credential_limit::ActiveModel::from)
            .unwrap_or_else(|| credential_limit::ActiveModel {
                id: Set(row_id),
                credential_id: Set(credential_id.to_string()),
                limit_id: Set(limit_id),
                limit_name: Set(None),
                primary_used_percent: Set(None),
                primary_window_minutes: Set(None),
                primary_resets_at: Set(None),
                secondary_used_percent: Set(None),
                secondary_window_minutes: Set(None),
                secondary_resets_at: Set(None),
                has_credits: Set(None),
                unlimited: Set(None),
                balance: Set(None),
                plan_type: Set(None),
                updated_at: Set(now),
            });

        active.limit_name = Set(snapshot.limit_name);
        active.primary_used_percent = Set(primary.as_ref().map(|window| window.used_percent));
        active.primary_window_minutes =
            Set(primary.as_ref().and_then(|window| window.window_minutes));
        active.primary_resets_at = Set(unix_to_utc(
            primary.as_ref().and_then(|window| window.resets_at),
        ));
        active.secondary_used_percent = Set(secondary.as_ref().map(|window| window.used_percent));
        active.secondary_window_minutes =
            Set(secondary.as_ref().and_then(|window| window.window_minutes));
        active.secondary_resets_at = Set(unix_to_utc(
            secondary.as_ref().and_then(|window| window.resets_at),
        ));
        active.has_credits = Set(credits.as_ref().map(|value| value.has_credits));
        active.unlimited = Set(credits.as_ref().map(|value| value.unlimited));
        active.balance = Set(credits.and_then(|value| value.balance));
        active.plan_type = Set(snapshot
            .plan_type
            .map(|plan| format!("{plan:?}").to_ascii_lowercase()));
        active.updated_at = Set(now);

        if active.id.is_set() && active.credential_id.is_set() {
            active
                .insert(self.db())
                .await
                .map_err(|err| AppError::internal(err.to_string()))?;
        } else {
            active
                .update(self.db())
                .await
                .map_err(|err| AppError::internal(err.to_string()))?;
        }
        Ok(())
    }
}

fn unix_to_utc(timestamp: Option<i64>) -> Option<DateTime<Utc>> {
    timestamp.and_then(|value| DateTime::<Utc>::from_timestamp(value, 0))
}

fn score_credential(
    model: &credential::Model,
    limits: &[credential_limit::Model],
    active_requests: usize,
) -> f64 {
    let base = if limits.is_empty() {
        55.0
    } else {
        limits
            .iter()
            .map(limit_remaining_percent)
            .fold(100.0, f64::min)
    };

    let credits_bonus = if limits
        .iter()
        .any(|limit| limit.unlimited == Some(true) || limit.has_credits == Some(true))
    {
        10.0
    } else {
        0.0
    };

    let weight_bonus = (model.selection_weight.max(1) as f64) * 5.0;
    let failure_penalty = (model.failure_count as f64) * 8.0;
    let active_penalty = (active_requests as f64) * 15.0;

    base + credits_bonus + weight_bonus - failure_penalty - active_penalty
}

fn limit_remaining_percent(limit: &credential_limit::Model) -> f64 {
    let primary = limit
        .primary_used_percent
        .map(|value| 100.0 - value)
        .unwrap_or(100.0);
    let secondary = limit
        .secondary_used_percent
        .map(|value| 100.0 - value)
        .unwrap_or(100.0);
    primary.min(secondary)
}

pub fn hash_api_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn extract_token_flags(auth: Option<CodexAuth>) -> (bool, bool) {
    match auth {
        Some(auth) => {
            let data = auth.get_token_data().ok();
            let has_access = data
                .as_ref()
                .map(|token_data| !token_data.access_token.is_empty())
                .unwrap_or(false);
            let has_refresh = data
                .as_ref()
                .map(|token_data| !token_data.refresh_token.is_empty())
                .unwrap_or(false);
            (has_access, has_refresh)
        }
        None => (false, false),
    }
}

pub async fn credential_view_material(
    state: &AppState,
    model: credential::Model,
) -> Result<(bool, bool, Vec<credential_limit::Model>), AppError> {
    let auth = state.auth_manager(&model.id).await.auth().await;
    let token_flags = extract_token_flags(auth);
    let limits = credential_limit::Entity::find()
        .filter(credential_limit::Column::CredentialId.eq(model.id.clone()))
        .order_by_asc(credential_limit::Column::LimitId)
        .all(state.db())
        .await
        .map_err(|err| AppError::internal(err.to_string()))?;
    Ok((token_flags.0, token_flags.1, limits))
}

async fn initialize_database(db: &DatabaseConnection) -> Result<(), AppError> {
    let backend = DbBackend::Sqlite;
    let statements = [
        r#"
        CREATE TABLE IF NOT EXISTS credentials (
            id TEXT PRIMARY KEY NOT NULL,
            name TEXT NOT NULL,
            kind TEXT NOT NULL,
            enabled INTEGER NOT NULL,
            selection_weight INTEGER NOT NULL DEFAULT 1,
            notes TEXT NULL,
            upstream_base_url TEXT NULL,
            account_id TEXT NULL,
            account_email TEXT NULL,
            plan_type TEXT NULL,
            last_used_at TEXT NULL,
            last_limit_sync_at TEXT NULL,
            last_refresh_at TEXT NULL,
            last_error TEXT NULL,
            failure_count INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS credential_limits (
            id TEXT PRIMARY KEY NOT NULL,
            credential_id TEXT NOT NULL,
            limit_id TEXT NOT NULL,
            limit_name TEXT NULL,
            primary_used_percent REAL NULL,
            primary_window_minutes INTEGER NULL,
            primary_resets_at TEXT NULL,
            secondary_used_percent REAL NULL,
            secondary_window_minutes INTEGER NULL,
            secondary_resets_at TEXT NULL,
            has_credits INTEGER NULL,
            unlimited INTEGER NULL,
            balance TEXT NULL,
            plan_type TEXT NULL,
            updated_at TEXT NOT NULL
        )
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS api_keys (
            id TEXT PRIMARY KEY NOT NULL,
            name TEXT NOT NULL,
            key_hash TEXT NOT NULL UNIQUE,
            enabled INTEGER NOT NULL,
            is_admin INTEGER NOT NULL,
            expires_at TEXT NULL,
            last_used_at TEXT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS auth_sessions (
            id TEXT PRIMARY KEY NOT NULL,
            credential_id TEXT NOT NULL,
            method TEXT NOT NULL,
            status TEXT NOT NULL,
            authorization_url TEXT NULL,
            redirect_uri TEXT NULL,
            oauth_state TEXT NULL,
            pkce_code_verifier TEXT NULL,
            verification_url TEXT NULL,
            user_code TEXT NULL,
            device_auth_id TEXT NULL,
            device_code_interval_seconds INTEGER NULL,
            error_message TEXT NULL,
            completed_at TEXT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS request_records (
            id TEXT PRIMARY KEY NOT NULL,
            credential_id TEXT NOT NULL,
            credential_name TEXT NOT NULL,
            api_key_id TEXT NULL,
            api_key_name TEXT NULL,
            principal_kind TEXT NOT NULL,
            transport TEXT NOT NULL,
            request_method TEXT NOT NULL,
            request_path TEXT NOT NULL,
            upstream_status_code INTEGER NULL,
            request_success INTEGER NULL,
            error_phase TEXT NULL,
            error_code TEXT NULL,
            error_message TEXT NULL,
            response_id TEXT NULL,
            requested_model TEXT NULL,
            input_tokens INTEGER NOT NULL DEFAULT 0,
            cached_input_tokens INTEGER NOT NULL DEFAULT 0,
            output_tokens INTEGER NOT NULL DEFAULT 0,
            reasoning_output_tokens INTEGER NOT NULL DEFAULT 0,
            total_tokens INTEGER NOT NULL DEFAULT 0,
            usage_json TEXT NULL,
            request_started_at TEXT NOT NULL,
            request_completed_at TEXT NULL,
            duration_ms INTEGER NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )
        "#,
        r#"CREATE INDEX IF NOT EXISTS idx_credentials_enabled ON credentials(enabled)"#,
        r#"CREATE INDEX IF NOT EXISTS idx_credential_limits_credential_id ON credential_limits(credential_id)"#,
        r#"CREATE INDEX IF NOT EXISTS idx_auth_sessions_credential_id ON auth_sessions(credential_id)"#,
        r#"CREATE INDEX IF NOT EXISTS idx_auth_sessions_status ON auth_sessions(status)"#,
        r#"CREATE INDEX IF NOT EXISTS idx_request_records_credential_id ON request_records(credential_id)"#,
        r#"CREATE INDEX IF NOT EXISTS idx_request_records_api_key_id ON request_records(api_key_id)"#,
        r#"CREATE INDEX IF NOT EXISTS idx_request_records_success ON request_records(request_success)"#,
        r#"CREATE INDEX IF NOT EXISTS idx_request_records_started_at ON request_records(request_started_at)"#,
    ];

    for sql in statements {
        db.execute(Statement::from_string(backend, sql.to_string()))
            .await
            .map_err(|err| AppError::internal(err.to_string()))?;
    }

    Ok(())
}

async fn recover_auth_sessions(db: &DatabaseConnection) -> Result<(), AppError> {
    let statement = Statement::from_string(
        DbBackend::Sqlite,
        format!(
            "UPDATE auth_sessions \
             SET status = 'failed', \
                 error_message = 'service restarted before auth completed', \
                 completed_at = CURRENT_TIMESTAMP, \
                 updated_at = CURRENT_TIMESTAMP \
             WHERE status = '{}'",
            crate::models::AuthStatus::Pending.as_str()
        ),
    );
    db.execute(statement)
        .await
        .map_err(|err| AppError::internal(err.to_string()))?;
    Ok(())
}
