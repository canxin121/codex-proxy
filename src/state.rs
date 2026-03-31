use crate::config::AppConfig;
use crate::entities::admin_key;
use crate::entities::api_key;
use crate::entities::auth_session;
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
use codex_login::default_client::build_reqwest_client;
use codex_login::load_auth_dot_json;
use codex_login::token_data::parse_jwt_expiration;
use http::HeaderMap;
use rand::RngExt;
use rand::distr::Alphanumeric;
use sea_orm::ActiveModelTrait;
use sea_orm::ColumnTrait;
use sea_orm::ConnectOptions;
use sea_orm::ConnectionTrait;
use sea_orm::Database;
use sea_orm::DatabaseConnection;
use sea_orm::EntityTrait;
use sea_orm::QueryFilter;
use sea_orm::QueryOrder;
use sea_orm::Set;
use sea_orm::sea_query::Alias;
use sea_orm::sea_query::ColumnDef;
use sea_orm::sea_query::Expr;
use sea_orm::sea_query::Index;
use sea_orm::sea_query::Table;
use sha2::Digest;
use sha2::Sha256;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs::OpenOptions;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::RwLock;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use url::form_urlencoded;
use uuid::Uuid;

#[derive(Clone)]
pub struct AppState {
    inner: Arc<AppStateInner>,
}

struct AppStateInner {
    config: AppConfig,
    db: DatabaseConnection,
    http_client: reqwest::Client,
    auth_managers: RwLock<HashMap<String, Arc<AuthManager>>>,
    admin_sessions: Mutex<HashMap<String, AdminSessionRecord>>,
    active_requests: Mutex<HashMap<String, usize>>,
    auth_cancellations: Mutex<HashMap<String, CancellationToken>>,
}

#[derive(Clone)]
struct AdminSessionRecord {
    created_at: DateTime<Utc>,
    last_used_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

pub struct CreatedAdminSession {
    pub session_token: String,
    pub created_at: DateTime<Utc>,
    pub last_used_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct AuthenticatedPrincipal {
    pub principal_kind: AuthenticatedPrincipalKind,
    pub api_key_id: Option<String>,
    pub api_key_name: Option<String>,
    pub admin_session_created_at: Option<DateTime<Utc>>,
    pub admin_session_last_used_at: Option<DateTime<Utc>>,
    pub admin_session_expires_at: Option<DateTime<Utc>>,
    pub admin_key_id: Option<String>,
    pub admin_key_name: Option<String>,
    pub admin_key_last_used_at: Option<DateTime<Utc>>,
    pub admin_key_expires_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthenticatedPrincipalKind {
    AdminSession,
    AdminKey,
    ApiKey,
}

impl AuthenticatedPrincipalKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AdminSession => "admin_session",
            Self::AdminKey => "admin_key",
            Self::ApiKey => "api_key",
        }
    }
}

const ADMIN_SESSION_TTL_DAYS: i64 = 30;
const TRANSIENT_QUOTA_BLOCK_MINUTES: i64 = 5;

pub struct SelectedCredential {
    pub model: credential::Model,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct CredentialQuotaAssessment {
    available: bool,
    next_retry_at: Option<DateTime<Utc>>,
    remaining_percent: Option<f64>,
    has_available_credits: bool,
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
        std::fs::create_dir_all(&config.data_dir)?;
        std::fs::create_dir_all(config.data_dir.join("credentials"))?;
        validate_sqlite_database_url(&config.database_url)?;
        ensure_sqlite_database_writable(&config.database_url)?;

        let db = Database::connect(database_connect_options(&config))
            .await
            .map_err(|err| AppError::internal(err.to_string()))?;
        ensure_sqlite_connection_writable(&db, &config.database_url).await?;
        initialize_database(&db).await?;
        recover_auth_sessions(&db).await?;
        retain_latest_auth_session_per_credential(&db).await?;

        let http_client = build_reqwest_client();

        Ok(Self {
            inner: Arc::new(AppStateInner {
                config,
                db,
                http_client,
                auth_managers: RwLock::new(HashMap::new()),
                admin_sessions: Mutex::new(HashMap::new()),
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
        manager.set_forced_chatgpt_workspace_id(self.config().forced_chatgpt_workspace_id.clone());

        if let Ok(mut guard) = self.inner.auth_managers.write() {
            guard.insert(credential_id.to_string(), Arc::clone(&manager));
        }

        manager
    }

    pub fn verify_admin_password(&self, password: &str) -> bool {
        hash_secret(password) == self.config().admin_password_hash
    }

    pub fn create_admin_session(&self) -> CreatedAdminSession {
        let now = Utc::now();
        let expires_at = now + chrono::Duration::days(ADMIN_SESSION_TTL_DAYS);
        let session_token = generate_session_token();
        let session_key = hash_secret(&session_token);
        let record = AdminSessionRecord {
            created_at: now,
            last_used_at: now,
            expires_at,
        };

        if let Ok(mut guard) = self.inner.admin_sessions.lock() {
            retain_active_admin_sessions(&mut guard, now);
            guard.insert(session_key, record.clone());
        }

        CreatedAdminSession {
            session_token,
            created_at: record.created_at,
            last_used_at: record.last_used_at,
            expires_at: record.expires_at,
        }
    }

    pub fn revoke_admin_session(&self, session_token: &str) -> bool {
        let now = Utc::now();
        self.inner
            .admin_sessions
            .lock()
            .ok()
            .map(|mut guard| {
                retain_active_admin_sessions(&mut guard, now);
                guard.remove(&hash_secret(session_token)).is_some()
            })
            .unwrap_or(false)
    }

    pub async fn authenticate_bearer(
        &self,
        bearer: &str,
        require_admin: bool,
        allow_admin_session: bool,
    ) -> Result<AuthenticatedPrincipal, AppError> {
        if require_admin {
            if allow_admin_session
                && let Some(record) = self.authenticate_admin_session_token(bearer)
            {
                return Ok(AuthenticatedPrincipal {
                    principal_kind: AuthenticatedPrincipalKind::AdminSession,
                    api_key_id: None,
                    api_key_name: None,
                    admin_session_created_at: Some(record.created_at),
                    admin_session_last_used_at: Some(record.last_used_at),
                    admin_session_expires_at: Some(record.expires_at),
                    admin_key_id: None,
                    admin_key_name: None,
                    admin_key_last_used_at: None,
                    admin_key_expires_at: None,
                });
            }
            return self.authenticate_admin_key(bearer).await;
        }
        self.authenticate_api_key(bearer).await
    }

    async fn authenticate_admin_key(
        &self,
        bearer: &str,
    ) -> Result<AuthenticatedPrincipal, AppError> {
        let key_hash = hash_secret(bearer);
        let record = admin_key::Entity::find()
            .filter(admin_key::Column::KeyHash.eq(key_hash))
            .one(self.db())
            .await
            .map_err(|err| AppError::internal(err.to_string()))?
            .ok_or_else(|| AppError::unauthorized("invalid admin key"))?;

        if !record.enabled {
            return Err(AppError::forbidden("admin key is disabled"));
        }
        if let Some(expires_at) = record.expires_at
            && expires_at < Utc::now()
        {
            return Err(AppError::forbidden("admin key has expired"));
        }

        let admin_key_id = record.id.clone();
        let admin_key_name = record.name.clone();
        let admin_key_expires_at = record.expires_at;
        let now = Utc::now();
        let mut active = admin_key::ActiveModel::from(record);
        active.last_used_at = Set(Some(now));
        active.updated_at = Set(now);
        active
            .update(self.db())
            .await
            .map_err(|err| AppError::internal(err.to_string()))?;

        Ok(AuthenticatedPrincipal {
            principal_kind: AuthenticatedPrincipalKind::AdminKey,
            api_key_id: None,
            api_key_name: None,
            admin_session_created_at: None,
            admin_session_last_used_at: None,
            admin_session_expires_at: None,
            admin_key_id: Some(admin_key_id),
            admin_key_name: Some(admin_key_name),
            admin_key_last_used_at: Some(now),
            admin_key_expires_at,
        })
    }

    async fn authenticate_api_key(&self, bearer: &str) -> Result<AuthenticatedPrincipal, AppError> {
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
            admin_session_created_at: None,
            admin_session_last_used_at: None,
            admin_session_expires_at: None,
            admin_key_id: None,
            admin_key_name: None,
            admin_key_last_used_at: None,
            admin_key_expires_at: None,
        })
    }

    fn authenticate_admin_session_token(&self, session_token: &str) -> Option<AdminSessionRecord> {
        let now = Utc::now();
        let session_key = hash_secret(session_token);
        let mut guard = self.inner.admin_sessions.lock().ok()?;
        retain_active_admin_sessions(&mut guard, now);
        let record = guard.get_mut(&session_key)?;
        record.last_used_at = now;
        Some(record.clone())
    }

    pub async fn select_credential(
        &self,
        preferred_id: Option<&str>,
        excluded_ids: &HashSet<String>,
    ) -> Result<SelectedCredential, AppError> {
        let now = Utc::now();
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
            let limits = credential_limit::Entity::find()
                .filter(credential_limit::Column::CredentialId.eq(model.id.clone()))
                .all(self.db())
                .await
                .map_err(|err| AppError::internal(err.to_string()))?;
            let quota = assess_credential_quota(&limits, now);
            if !quota.available {
                return Err(no_available_quota_error(
                    "preferred credential does not currently have available quota",
                    quota.next_retry_at,
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

        let enabled_credential_ids = credentials
            .iter()
            .map(|model| model.id.clone())
            .collect::<Vec<_>>();
        let limit_rows = credential_limit::Entity::find()
            .filter(credential_limit::Column::CredentialId.is_in(enabled_credential_ids))
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

        let mut any_authenticated = false;
        let mut blocked_next_retry_at: Option<DateTime<Utc>> = None;
        let mut saw_excluded_candidate = false;
        let mut best: Option<(f64, credential::Model)> = None;
        for model in credentials {
            let is_excluded = excluded_ids.contains(&model.id);
            if !self.credential_has_active_auth(&model.id).await {
                continue;
            }
            any_authenticated = true;
            let quota = assess_credential_quota(
                limits_by_credential
                    .get(&model.id)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]),
                now,
            );
            if !quota.available {
                if let Some(retry_at) = quota.next_retry_at {
                    blocked_next_retry_at = Some(match blocked_next_retry_at {
                        Some(current) => current.min(retry_at),
                        None => retry_at,
                    });
                }
                if is_excluded {
                    saw_excluded_candidate = true;
                }
                continue;
            }
            if is_excluded {
                saw_excluded_candidate = true;
                continue;
            }
            let score = score_credential(&model, quota, self.active_requests_for(&model.id));

            match best.as_ref() {
                Some((best_score, _)) if *best_score >= score => {}
                _ => best = Some((score, model)),
            }
        }

        let (_, model) = match best {
            Some(best) => best,
            None if any_authenticated || saw_excluded_candidate => {
                return Err(no_available_quota_error(
                    "no Codex credentials currently have available quota",
                    blocked_next_retry_at,
                ));
            }
            None => {
                return Err(AppError::service_unavailable(
                    "no enabled authenticated Codex credentials are available",
                ));
            }
        };
        Ok(SelectedCredential { model })
    }

    async fn credential_has_active_auth(&self, credential_id: &str) -> bool {
        let manager = self.auth_manager(credential_id).await;
        let Some(auth) = manager.auth().await else {
            return false;
        };
        !auth_requires_reauthentication(&manager, &auth)
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
        let should_auto_name = (existing.account_email.is_none() && existing.account_id.is_none())
            || existing.name.starts_with("importing-");
        let derived_name = account_email
            .clone()
            .or_else(|| account_id.clone())
            .unwrap_or_else(|| existing.name.clone());

        let mut active = credential::ActiveModel::from(existing);
        if should_auto_name {
            active.name = Set(derived_name);
        }
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

    pub async fn mark_credential_quota_exhausted(
        &self,
        credential_id: &str,
        retry_at: Option<DateTime<Utc>>,
    ) -> Result<(), AppError> {
        let row_id = format!("{credential_id}:codex");
        let existing = credential_limit::Entity::find_by_id(row_id.clone())
            .one(self.db())
            .await
            .map_err(|err| AppError::internal(err.to_string()))?;
        let now = Utc::now();

        let mut active = existing
            .map(credential_limit::ActiveModel::from)
            .unwrap_or_else(|| credential_limit::ActiveModel {
                id: Set(row_id),
                credential_id: Set(credential_id.to_string()),
                limit_id: Set("codex".to_string()),
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

        active.primary_used_percent = Set(Some(100.0));
        active.primary_window_minutes = Set(Some(TRANSIENT_QUOTA_BLOCK_MINUTES));
        active.primary_resets_at = Set(retry_at);
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
        self.mark_limit_sync_now(credential_id).await?;
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
            self.mark_limit_sync_now(credential_id).await?;
        }
        Ok(())
    }

    pub async fn update_rate_limit_snapshot(
        &self,
        credential_id: &str,
        snapshot: codex_protocol::protocol::RateLimitSnapshot,
    ) -> Result<(), AppError> {
        self.persist_rate_limit_snapshot(credential_id, snapshot)
            .await?;
        self.mark_limit_sync_now(credential_id).await
    }

    async fn mark_limit_sync_now(&self, credential_id: &str) -> Result<(), AppError> {
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

        apply_rate_limit_snapshot_to_active_model(&mut active, snapshot);
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

fn auth_requires_reauthentication(manager: &AuthManager, auth: &CodexAuth) -> bool {
    if !auth.is_chatgpt_auth() || manager.refresh_failure_for_auth(auth).is_none() {
        return false;
    }

    let Ok(token_data) = auth.get_token_data() else {
        return true;
    };

    match parse_jwt_expiration(&token_data.access_token) {
        Ok(Some(expires_at)) => expires_at <= Utc::now(),
        Ok(None) | Err(_) => true,
    }
}

fn no_available_quota_error(message: &str, next_retry_at: Option<DateTime<Utc>>) -> AppError {
    match next_retry_at {
        Some(next_retry_at) => AppError::service_unavailable(format!(
            "{message}; next retry at {}",
            next_retry_at.to_rfc3339()
        )),
        None => AppError::service_unavailable(message.to_string()),
    }
}

fn assess_credential_quota(
    limits: &[credential_limit::Model],
    now: DateTime<Utc>,
) -> CredentialQuotaAssessment {
    if limits.is_empty() {
        return CredentialQuotaAssessment {
            available: true,
            next_retry_at: None,
            remaining_percent: None,
            has_available_credits: false,
        };
    }

    let mut available = true;
    let mut next_retry_at: Option<DateTime<Utc>> = None;
    let mut blocked_without_retry = false;
    let mut remaining_percent: Option<f64> = None;
    let mut has_available_credits = false;

    for limit in limits {
        match (limit.unlimited, limit.has_credits) {
            (Some(true), _) | (_, Some(true)) => {
                has_available_credits = true;
            }
            _ => {}
        }

        for window in [
            assess_limit_window(
                limit.primary_used_percent,
                limit.primary_resets_at,
                limit.primary_window_minutes,
                limit.updated_at,
                now,
            ),
            assess_limit_window(
                limit.secondary_used_percent,
                limit.secondary_resets_at,
                limit.secondary_window_minutes,
                limit.updated_at,
                now,
            ),
        ]
        .into_iter()
        .flatten()
        {
            remaining_percent = Some(match remaining_percent {
                Some(current) => current.min(window.remaining_percent),
                None => window.remaining_percent,
            });

            if window.blocked {
                available = false;
                if let Some(window_retry_at) = window.next_retry_at {
                    next_retry_at = Some(match next_retry_at {
                        Some(current) => current.max(window_retry_at),
                        None => window_retry_at,
                    });
                } else {
                    blocked_without_retry = true;
                }
            }
        }
    }

    if blocked_without_retry {
        next_retry_at = None;
    }

    CredentialQuotaAssessment {
        available,
        next_retry_at,
        remaining_percent,
        has_available_credits,
    }
}

#[derive(Debug, Clone, Copy)]
struct LimitWindowAssessment {
    blocked: bool,
    next_retry_at: Option<DateTime<Utc>>,
    remaining_percent: f64,
}

fn assess_limit_window(
    used_percent: Option<f64>,
    resets_at: Option<DateTime<Utc>>,
    window_minutes: Option<i64>,
    updated_at: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Option<LimitWindowAssessment> {
    let used_percent = used_percent?.clamp(0.0, 100.0);
    if used_percent < 100.0 {
        return Some(LimitWindowAssessment {
            blocked: false,
            next_retry_at: None,
            remaining_percent: 100.0 - used_percent,
        });
    }

    let next_retry_at = effective_limit_window_reset_at(resets_at, window_minutes, updated_at)
        .or_else(|| transient_quota_retry_at(updated_at));
    match next_retry_at {
        Some(next_retry_at) if next_retry_at > now => Some(LimitWindowAssessment {
            blocked: true,
            next_retry_at: Some(next_retry_at),
            remaining_percent: 0.0,
        }),
        Some(_) => Some(LimitWindowAssessment {
            blocked: false,
            next_retry_at: None,
            remaining_percent: 100.0,
        }),
        None => Some(LimitWindowAssessment {
            blocked: true,
            next_retry_at: None,
            remaining_percent: 0.0,
        }),
    }
}

fn effective_limit_window_reset_at(
    resets_at: Option<DateTime<Utc>>,
    window_minutes: Option<i64>,
    updated_at: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    resets_at.or_else(|| {
        window_minutes
            .filter(|minutes| *minutes > 0)
            .map(chrono::Duration::minutes)
            .and_then(|duration| updated_at.checked_add_signed(duration))
    })
}

fn transient_quota_retry_at(updated_at: DateTime<Utc>) -> Option<DateTime<Utc>> {
    updated_at.checked_add_signed(chrono::Duration::minutes(TRANSIENT_QUOTA_BLOCK_MINUTES))
}

fn apply_rate_limit_snapshot_to_active_model(
    active: &mut credential_limit::ActiveModel,
    snapshot: codex_protocol::protocol::RateLimitSnapshot,
) {
    let primary = snapshot.primary;
    let secondary = snapshot.secondary;

    if let Some(limit_name) = snapshot.limit_name {
        active.limit_name = Set(Some(limit_name));
    }
    active.primary_used_percent = Set(primary.as_ref().map(|window| window.used_percent));
    active.primary_window_minutes = Set(primary.as_ref().and_then(|window| window.window_minutes));
    active.primary_resets_at = Set(unix_to_utc(
        primary.as_ref().and_then(|window| window.resets_at),
    ));
    active.secondary_used_percent = Set(secondary.as_ref().map(|window| window.used_percent));
    active.secondary_window_minutes =
        Set(secondary.as_ref().and_then(|window| window.window_minutes));
    active.secondary_resets_at = Set(unix_to_utc(
        secondary.as_ref().and_then(|window| window.resets_at),
    ));

    if let Some(credits) = snapshot.credits {
        active.has_credits = Set(Some(credits.has_credits));
        active.unlimited = Set(Some(credits.unlimited));
        active.balance = Set(credits.balance);
    }

    if let Some(plan_type) = snapshot.plan_type {
        active.plan_type = Set(Some(format!("{plan_type:?}").to_ascii_lowercase()));
    }
}

fn score_credential(
    model: &credential::Model,
    quota: CredentialQuotaAssessment,
    active_requests: usize,
) -> f64 {
    let base = quota.remaining_percent.unwrap_or(55.0);
    let credits_bonus = if quota.has_available_credits {
        10.0
    } else {
        0.0
    };

    let weight_bonus = (model.selection_weight.max(1) as f64) * 5.0;
    let failure_penalty = (model.failure_count as f64) * 8.0;
    let active_penalty = (active_requests as f64) * 15.0;

    base + credits_bonus + weight_bonus - failure_penalty - active_penalty
}

pub fn hash_api_key(key: &str) -> String {
    hash_secret(key)
}

fn hash_secret(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn generate_session_token() -> String {
    let suffix = rand::rng()
        .sample_iter(Alphanumeric)
        .take(40)
        .map(char::from)
        .collect::<String>();
    format!("cps_{suffix}")
}

fn database_connect_options(config: &AppConfig) -> ConnectOptions {
    let mut options = ConnectOptions::new(config.database_url.clone());
    options.sqlx_logging(false);
    options.connect_timeout(Duration::from_secs(10));
    options.acquire_timeout(Duration::from_secs(10));

    if is_sqlite_database_url(&config.database_url) {
        options.max_connections(1);
        options.min_connections(1);
        options.idle_timeout(Duration::from_secs(30));
        // Periodically recycle SQLite connections so the service can recover
        // from external db-file replacement without a full process restart.
        options.max_lifetime(Duration::from_secs(60));
        options.test_before_acquire(true);
        options.map_sqlx_sqlite_opts(|opts| {
            opts.read_only(false)
                .create_if_missing(true)
                .busy_timeout(Duration::from_secs(5))
        });
    }

    options
}

fn validate_sqlite_database_url(database_url: &str) -> Result<(), AppError> {
    if !is_sqlite_database_url(database_url) {
        return Ok(());
    }

    if let Some(mode) = sqlite_query_value(database_url, "mode")
        && mode.eq_ignore_ascii_case("ro")
    {
        return Err(AppError::internal(
            "sqlite database URL is configured as read-only (mode=ro). Use mode=rwc for write access.",
        ));
    }

    if let Some(immutable) = sqlite_query_value(database_url, "immutable")
        && matches!(
            immutable.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    {
        return Err(AppError::internal(
            "sqlite database URL sets immutable=1, which is read-only. Remove immutable to allow writes.",
        ));
    }

    Ok(())
}

async fn ensure_sqlite_connection_writable(
    db: &DatabaseConnection,
    database_url: &str,
) -> Result<(), AppError> {
    if !is_sqlite_database_url(database_url) {
        return Ok(());
    }

    db.execute_unprepared("BEGIN IMMEDIATE; ROLLBACK;")
        .await
        .map_err(|err| {
            let message = err.to_string();
            if sqlite_readonly_error(&message) {
                AppError::internal(format!(
                    "sqlite database is not writable: {message}. \
This often means the db file or its directory became read-only, or the db file was replaced while the process was running."
                ))
            } else {
                AppError::internal(message)
            }
        })?;

    Ok(())
}

fn is_sqlite_database_url(database_url: &str) -> bool {
    database_url.trim_start().starts_with("sqlite:")
}

fn sqlite_readonly_error(message: &str) -> bool {
    let lowered = message.to_ascii_lowercase();
    lowered.contains("readonly database")
        || lowered.contains("sqlite_readonly")
        || lowered.contains("(code: 1032)")
}

fn ensure_sqlite_database_writable(database_url: &str) -> Result<(), AppError> {
    let Some(database_path) = sqlite_database_path(database_url) else {
        return Ok(());
    };

    let database_dir = database_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    std::fs::create_dir_all(&database_dir).map_err(|err| {
        AppError::internal(format!(
            "sqlite database directory {} is not writable: {}. Set CODEX_PROXY_DATA_DIR or CODEX_PROXY_DATABASE_URL to a writable location.",
            database_dir.display(),
            err
        ))
    })?;
    ensure_directory_writable(&database_dir)?;

    if database_path.exists() {
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(&database_path)
            .map_err(|err| {
                AppError::internal(format!(
                    "sqlite database file {} is not writable: {}. Set CODEX_PROXY_DATA_DIR or CODEX_PROXY_DATABASE_URL to a writable location.",
                    database_path.display(),
                    err
                ))
            })?;
    }

    Ok(())
}

fn ensure_directory_writable(path: &Path) -> Result<(), AppError> {
    let probe_path = path.join(format!(".codex-proxy-write-probe-{}", Uuid::new_v4()));
    OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&probe_path)
        .map_err(|err| {
            AppError::internal(format!(
                "sqlite database directory {} is not writable: {}. Set CODEX_PROXY_DATA_DIR or CODEX_PROXY_DATABASE_URL to a writable location.",
                path.display(),
                err
            ))
        })?;
    let _ = std::fs::remove_file(probe_path);
    Ok(())
}

fn sqlite_database_path(database_url: &str) -> Option<PathBuf> {
    let database_url = database_url.trim();
    if !is_sqlite_database_url(database_url) {
        return None;
    }
    if database_url.starts_with("sqlite::memory:") {
        return None;
    }
    if sqlite_query_value(database_url, "mode")
        .is_some_and(|mode| mode.eq_ignore_ascii_case("memory"))
    {
        return None;
    }

    let path = database_url.strip_prefix("sqlite:")?;
    let path = path.split_once('#').map(|(path, _)| path).unwrap_or(path);
    let path = path.split_once('?').map(|(path, _)| path).unwrap_or(path);
    let path = path.strip_prefix("//").unwrap_or(path);
    if path.is_empty() || path == ":memory:" {
        return None;
    }

    Some(PathBuf::from(path))
}

fn sqlite_query_value(database_url: &str, key: &str) -> Option<String> {
    let query = sqlite_url_query(database_url)?;
    form_urlencoded::parse(query.as_bytes()).find_map(|(name, value)| {
        if name.eq_ignore_ascii_case(key) {
            Some(value.into_owned())
        } else {
            None
        }
    })
}

fn sqlite_url_query(database_url: &str) -> Option<&str> {
    let (_, query) = database_url.split_once('?')?;
    Some(
        query
            .split_once('#')
            .map(|(query, _)| query)
            .unwrap_or(query),
    )
}

fn retain_active_admin_sessions(
    sessions: &mut HashMap<String, AdminSessionRecord>,
    now: DateTime<Utc>,
) {
    sessions.retain(|_, record| record.expires_at > now);
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
    let backend = db.get_database_backend();
    let credentials_table = Alias::new("credentials");
    let credential_limits_table = Alias::new("credential_limits");
    let admin_keys_table = Alias::new("admin_keys");
    let api_keys_table = Alias::new("api_keys");
    let auth_sessions_table = Alias::new("auth_sessions");
    let request_records_table = Alias::new("request_records");

    let statements = vec![
        backend.build(
            &Table::create()
                .if_not_exists()
                .table(credentials_table.clone())
                .col(
                    ColumnDef::new(credential::Column::Id)
                        .string()
                        .not_null()
                        .primary_key(),
                )
                .col(ColumnDef::new(credential::Column::Name).string().not_null())
                .col(ColumnDef::new(credential::Column::Kind).string().not_null())
                .col(
                    ColumnDef::new(credential::Column::Enabled)
                        .boolean()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(credential::Column::SelectionWeight)
                        .integer()
                        .not_null()
                        .default(1),
                )
                .col(ColumnDef::new(credential::Column::Notes).string())
                .col(ColumnDef::new(credential::Column::UpstreamBaseUrl).string())
                .col(ColumnDef::new(credential::Column::AccountId).string())
                .col(ColumnDef::new(credential::Column::AccountEmail).string())
                .col(ColumnDef::new(credential::Column::PlanType).string())
                .col(ColumnDef::new(credential::Column::LastUsedAt).date_time())
                .col(ColumnDef::new(credential::Column::LastLimitSyncAt).date_time())
                .col(ColumnDef::new(credential::Column::LastRefreshAt).date_time())
                .col(ColumnDef::new(credential::Column::LastError).string())
                .col(
                    ColumnDef::new(credential::Column::FailureCount)
                        .integer()
                        .not_null()
                        .default(0),
                )
                .col(
                    ColumnDef::new(credential::Column::CreatedAt)
                        .date_time()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(credential::Column::UpdatedAt)
                        .date_time()
                        .not_null(),
                )
                .to_owned(),
        ),
        backend.build(
            &Table::create()
                .if_not_exists()
                .table(credential_limits_table.clone())
                .col(
                    ColumnDef::new(credential_limit::Column::Id)
                        .string()
                        .not_null()
                        .primary_key(),
                )
                .col(
                    ColumnDef::new(credential_limit::Column::CredentialId)
                        .string()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(credential_limit::Column::LimitId)
                        .string()
                        .not_null(),
                )
                .col(ColumnDef::new(credential_limit::Column::LimitName).string())
                .col(ColumnDef::new(credential_limit::Column::PrimaryUsedPercent).double())
                .col(ColumnDef::new(credential_limit::Column::PrimaryWindowMinutes).big_integer())
                .col(ColumnDef::new(credential_limit::Column::PrimaryResetsAt).date_time())
                .col(ColumnDef::new(credential_limit::Column::SecondaryUsedPercent).double())
                .col(ColumnDef::new(credential_limit::Column::SecondaryWindowMinutes).big_integer())
                .col(ColumnDef::new(credential_limit::Column::SecondaryResetsAt).date_time())
                .col(ColumnDef::new(credential_limit::Column::HasCredits).boolean())
                .col(ColumnDef::new(credential_limit::Column::Unlimited).boolean())
                .col(ColumnDef::new(credential_limit::Column::Balance).string())
                .col(ColumnDef::new(credential_limit::Column::PlanType).string())
                .col(
                    ColumnDef::new(credential_limit::Column::UpdatedAt)
                        .date_time()
                        .not_null(),
                )
                .to_owned(),
        ),
        backend.build(
            &Table::create()
                .if_not_exists()
                .table(admin_keys_table.clone())
                .col(
                    ColumnDef::new(admin_key::Column::Id)
                        .string()
                        .not_null()
                        .primary_key(),
                )
                .col(ColumnDef::new(admin_key::Column::Name).string().not_null())
                .col(
                    ColumnDef::new(admin_key::Column::KeyHash)
                        .string()
                        .not_null()
                        .unique_key(),
                )
                .col(
                    ColumnDef::new(admin_key::Column::Enabled)
                        .boolean()
                        .not_null(),
                )
                .col(ColumnDef::new(admin_key::Column::ExpiresAt).date_time())
                .col(ColumnDef::new(admin_key::Column::LastUsedAt).date_time())
                .col(
                    ColumnDef::new(admin_key::Column::CreatedAt)
                        .date_time()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(admin_key::Column::UpdatedAt)
                        .date_time()
                        .not_null(),
                )
                .to_owned(),
        ),
        backend.build(
            &Table::create()
                .if_not_exists()
                .table(api_keys_table.clone())
                .col(
                    ColumnDef::new(api_key::Column::Id)
                        .string()
                        .not_null()
                        .primary_key(),
                )
                .col(ColumnDef::new(api_key::Column::Name).string().not_null())
                .col(
                    ColumnDef::new(api_key::Column::KeyHash)
                        .string()
                        .not_null()
                        .unique_key(),
                )
                .col(
                    ColumnDef::new(api_key::Column::Enabled)
                        .boolean()
                        .not_null(),
                )
                .col(ColumnDef::new(api_key::Column::ExpiresAt).date_time())
                .col(ColumnDef::new(api_key::Column::LastUsedAt).date_time())
                .col(
                    ColumnDef::new(api_key::Column::CreatedAt)
                        .date_time()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(api_key::Column::UpdatedAt)
                        .date_time()
                        .not_null(),
                )
                .to_owned(),
        ),
        backend.build(
            &Table::create()
                .if_not_exists()
                .table(auth_sessions_table.clone())
                .col(
                    ColumnDef::new(crate::entities::auth_session::Column::Id)
                        .string()
                        .not_null()
                        .primary_key(),
                )
                .col(
                    ColumnDef::new(crate::entities::auth_session::Column::CredentialId)
                        .string()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(crate::entities::auth_session::Column::Method)
                        .string()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(crate::entities::auth_session::Column::Status)
                        .string()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(crate::entities::auth_session::Column::AuthorizationUrl)
                        .string(),
                )
                .col(ColumnDef::new(crate::entities::auth_session::Column::RedirectUri).string())
                .col(ColumnDef::new(crate::entities::auth_session::Column::OauthState).string())
                .col(
                    ColumnDef::new(crate::entities::auth_session::Column::PkceCodeVerifier)
                        .string(),
                )
                .col(
                    ColumnDef::new(crate::entities::auth_session::Column::VerificationUrl).string(),
                )
                .col(ColumnDef::new(crate::entities::auth_session::Column::UserCode).string())
                .col(ColumnDef::new(crate::entities::auth_session::Column::DeviceAuthId).string())
                .col(
                    ColumnDef::new(
                        crate::entities::auth_session::Column::DeviceCodeIntervalSeconds,
                    )
                    .integer(),
                )
                .col(ColumnDef::new(crate::entities::auth_session::Column::ErrorMessage).string())
                .col(ColumnDef::new(crate::entities::auth_session::Column::CompletedAt).date_time())
                .col(
                    ColumnDef::new(crate::entities::auth_session::Column::CreatedAt)
                        .date_time()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(crate::entities::auth_session::Column::UpdatedAt)
                        .date_time()
                        .not_null(),
                )
                .to_owned(),
        ),
        backend.build(
            &Table::create()
                .if_not_exists()
                .table(request_records_table.clone())
                .col(
                    ColumnDef::new(crate::entities::request_record::Column::Id)
                        .string()
                        .not_null()
                        .primary_key(),
                )
                .col(
                    ColumnDef::new(crate::entities::request_record::Column::CredentialId)
                        .string()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(crate::entities::request_record::Column::CredentialName)
                        .string()
                        .not_null(),
                )
                .col(ColumnDef::new(crate::entities::request_record::Column::ApiKeyId).string())
                .col(ColumnDef::new(crate::entities::request_record::Column::ApiKeyName).string())
                .col(
                    ColumnDef::new(crate::entities::request_record::Column::PrincipalKind)
                        .string()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(crate::entities::request_record::Column::Transport)
                        .string()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(crate::entities::request_record::Column::RequestMethod)
                        .string()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(crate::entities::request_record::Column::RequestPath)
                        .string()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(crate::entities::request_record::Column::UpstreamStatusCode)
                        .integer(),
                )
                .col(
                    ColumnDef::new(crate::entities::request_record::Column::RequestSuccess)
                        .boolean(),
                )
                .col(ColumnDef::new(crate::entities::request_record::Column::ErrorPhase).string())
                .col(ColumnDef::new(crate::entities::request_record::Column::ErrorCode).string())
                .col(ColumnDef::new(crate::entities::request_record::Column::ErrorMessage).string())
                .col(ColumnDef::new(crate::entities::request_record::Column::ResponseId).string())
                .col(
                    ColumnDef::new(crate::entities::request_record::Column::RequestedModel)
                        .string(),
                )
                .col(
                    ColumnDef::new(crate::entities::request_record::Column::InputTokens)
                        .big_integer()
                        .not_null()
                        .default(0_i64),
                )
                .col(
                    ColumnDef::new(crate::entities::request_record::Column::CachedInputTokens)
                        .big_integer()
                        .not_null()
                        .default(0_i64),
                )
                .col(
                    ColumnDef::new(crate::entities::request_record::Column::OutputTokens)
                        .big_integer()
                        .not_null()
                        .default(0_i64),
                )
                .col(
                    ColumnDef::new(crate::entities::request_record::Column::ReasoningOutputTokens)
                        .big_integer()
                        .not_null()
                        .default(0_i64),
                )
                .col(
                    ColumnDef::new(crate::entities::request_record::Column::TotalTokens)
                        .big_integer()
                        .not_null()
                        .default(0_i64),
                )
                .col(ColumnDef::new(crate::entities::request_record::Column::UsageJson).string())
                .col(
                    ColumnDef::new(crate::entities::request_record::Column::RequestStartedAt)
                        .date_time()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(crate::entities::request_record::Column::RequestCompletedAt)
                        .date_time(),
                )
                .col(
                    ColumnDef::new(crate::entities::request_record::Column::DurationMs)
                        .big_integer(),
                )
                .col(
                    ColumnDef::new(crate::entities::request_record::Column::CreatedAt)
                        .date_time()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(crate::entities::request_record::Column::UpdatedAt)
                        .date_time()
                        .not_null(),
                )
                .to_owned(),
        ),
        backend.build(
            &Index::create()
                .if_not_exists()
                .name("idx_credentials_enabled")
                .table(credentials_table)
                .col(credential::Column::Enabled)
                .to_owned(),
        ),
        backend.build(
            &Index::create()
                .if_not_exists()
                .name("idx_credential_limits_credential_id")
                .table(credential_limits_table)
                .col(credential_limit::Column::CredentialId)
                .to_owned(),
        ),
        backend.build(
            &Index::create()
                .if_not_exists()
                .name("idx_admin_keys_enabled")
                .table(admin_keys_table)
                .col(admin_key::Column::Enabled)
                .to_owned(),
        ),
        backend.build(
            &Index::create()
                .if_not_exists()
                .name("idx_auth_sessions_credential_id")
                .table(auth_sessions_table.clone())
                .col(crate::entities::auth_session::Column::CredentialId)
                .to_owned(),
        ),
        backend.build(
            &Index::create()
                .if_not_exists()
                .name("idx_auth_sessions_status")
                .table(auth_sessions_table)
                .col(crate::entities::auth_session::Column::Status)
                .to_owned(),
        ),
        backend.build(
            &Index::create()
                .if_not_exists()
                .name("idx_request_records_credential_id")
                .table(request_records_table.clone())
                .col(crate::entities::request_record::Column::CredentialId)
                .to_owned(),
        ),
        backend.build(
            &Index::create()
                .if_not_exists()
                .name("idx_request_records_api_key_id")
                .table(request_records_table.clone())
                .col(crate::entities::request_record::Column::ApiKeyId)
                .to_owned(),
        ),
        backend.build(
            &Index::create()
                .if_not_exists()
                .name("idx_request_records_success")
                .table(request_records_table.clone())
                .col(crate::entities::request_record::Column::RequestSuccess)
                .to_owned(),
        ),
        backend.build(
            &Index::create()
                .if_not_exists()
                .name("idx_request_records_started_at")
                .table(request_records_table)
                .col(crate::entities::request_record::Column::RequestStartedAt)
                .to_owned(),
        ),
    ];

    for statement in statements {
        db.execute(statement)
            .await
            .map_err(|err| AppError::internal(err.to_string()))?;
    }

    Ok(())
}

async fn recover_auth_sessions(db: &DatabaseConnection) -> Result<(), AppError> {
    crate::entities::auth_session::Entity::update_many()
        .col_expr(
            crate::entities::auth_session::Column::Status,
            Expr::value(crate::models::AuthStatus::Failed.as_str()),
        )
        .col_expr(
            crate::entities::auth_session::Column::ErrorMessage,
            Expr::value("service restarted before auth completed"),
        )
        .col_expr(
            crate::entities::auth_session::Column::CompletedAt,
            Expr::current_timestamp().into(),
        )
        .col_expr(
            crate::entities::auth_session::Column::UpdatedAt,
            Expr::current_timestamp().into(),
        )
        .filter(
            crate::entities::auth_session::Column::Status
                .eq(crate::models::AuthStatus::Pending.as_str()),
        )
        .exec(db)
        .await
        .map_err(|err| AppError::internal(err.to_string()))?;
    Ok(())
}

async fn retain_latest_auth_session_per_credential(
    db: &DatabaseConnection,
) -> Result<(), AppError> {
    let sessions = auth_session::Entity::find()
        .order_by_asc(auth_session::Column::CredentialId)
        .order_by_desc(auth_session::Column::CreatedAt)
        .all(db)
        .await
        .map_err(|err| AppError::internal(err.to_string()))?;

    let mut seen_credentials = HashSet::new();
    for session in sessions {
        if seen_credentials.insert(session.credential_id.clone()) {
            continue;
        }
        auth_session::Entity::delete_by_id(session.id)
            .exec(db)
            .await
            .map_err(|err| AppError::internal(err.to_string()))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::CredentialQuotaAssessment;
    use super::apply_rate_limit_snapshot_to_active_model;
    use super::assess_credential_quota;
    use super::sqlite_database_path;
    use super::sqlite_query_value;
    use super::validate_sqlite_database_url;
    use crate::entities::credential_limit;
    use crate::error::AppError;
    use chrono::DateTime;
    use chrono::Utc;
    use codex_protocol::protocol::CreditsSnapshot;
    use codex_protocol::protocol::RateLimitSnapshot;
    use codex_protocol::protocol::RateLimitWindow;
    use std::path::PathBuf;

    fn timestamp(value: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(value, 0).expect("timestamp should be valid")
    }

    fn limit_model() -> credential_limit::Model {
        credential_limit::Model {
            id: "cred-1:codex".to_string(),
            credential_id: "cred-1".to_string(),
            limit_id: "codex".to_string(),
            limit_name: None,
            primary_used_percent: None,
            primary_window_minutes: None,
            primary_resets_at: None,
            secondary_used_percent: None,
            secondary_window_minutes: None,
            secondary_resets_at: None,
            has_credits: None,
            unlimited: None,
            balance: None,
            plan_type: None,
            updated_at: timestamp(1_700_000_000),
        }
    }

    #[test]
    fn sqlite_database_path_extracts_file_backed_sqlite_urls() {
        assert_eq!(
            sqlite_database_path("sqlite:///tmp/codex-proxy/codex-proxy.sqlite?mode=rwc"),
            Some(PathBuf::from("/tmp/codex-proxy/codex-proxy.sqlite"))
        );
        assert_eq!(
            sqlite_database_path("sqlite://relative/codex-proxy.sqlite"),
            Some(PathBuf::from("relative/codex-proxy.sqlite"))
        );
        assert_eq!(
            sqlite_database_path("sqlite:relative/codex-proxy.sqlite"),
            Some(PathBuf::from("relative/codex-proxy.sqlite"))
        );
        assert_eq!(
            sqlite_database_path("sqlite:./relative/codex-proxy.sqlite?cache=shared"),
            Some(PathBuf::from("./relative/codex-proxy.sqlite"))
        );
    }

    #[test]
    fn sqlite_database_path_ignores_memory_databases() {
        assert_eq!(sqlite_database_path("sqlite::memory:"), None);
        assert_eq!(sqlite_database_path("sqlite://:memory:"), None);
        assert_eq!(sqlite_database_path("sqlite:file.db?mode=memory"), None);
    }

    #[test]
    fn sqlite_query_value_parses_case_insensitive_keys() {
        assert_eq!(
            sqlite_query_value("sqlite:///tmp/db.sqlite?mode=rwc&immutable=1", "mode").as_deref(),
            Some("rwc")
        );
        assert_eq!(
            sqlite_query_value("sqlite:///tmp/db.sqlite?Mode=ro", "mode").as_deref(),
            Some("ro")
        );
    }

    #[test]
    fn validate_sqlite_database_url_rejects_readonly_flags() {
        match validate_sqlite_database_url("sqlite:///tmp/db.sqlite?mode=ro") {
            Err(AppError::Internal(message)) => {
                assert!(message.contains("read-only"));
            }
            other => panic!("expected read-only mode to be rejected, got {other:?}"),
        }

        match validate_sqlite_database_url("sqlite:///tmp/db.sqlite?immutable=1") {
            Err(AppError::Internal(message)) => {
                assert!(message.contains("immutable=1"));
            }
            other => panic!("expected immutable=1 to be rejected, got {other:?}"),
        }
    }

    #[test]
    fn validate_sqlite_database_url_accepts_writable_urls() {
        assert!(validate_sqlite_database_url("sqlite:///tmp/db.sqlite?mode=rwc").is_ok());
        assert!(validate_sqlite_database_url("sqlite://relative/db.sqlite").is_ok());
        assert!(validate_sqlite_database_url("postgres://localhost/db").is_ok());
    }

    #[test]
    fn assess_credential_quota_blocks_future_reset_windows() {
        let mut limit = limit_model();
        limit.primary_used_percent = Some(100.0);
        limit.primary_resets_at = Some(timestamp(1_700_000_300));

        let assessment = assess_credential_quota(&[limit], timestamp(1_700_000_000));

        assert_eq!(
            assessment,
            CredentialQuotaAssessment {
                available: false,
                next_retry_at: Some(timestamp(1_700_000_300)),
                remaining_percent: Some(0.0),
                has_available_credits: false,
            }
        );
    }

    #[test]
    fn assess_credential_quota_retries_after_reset_time_passes() {
        let mut limit = limit_model();
        limit.primary_used_percent = Some(100.0);
        limit.primary_resets_at = Some(timestamp(1_700_000_300));

        let assessment = assess_credential_quota(&[limit], timestamp(1_700_000_301));

        assert_eq!(
            assessment,
            CredentialQuotaAssessment {
                available: true,
                next_retry_at: None,
                remaining_percent: Some(100.0),
                has_available_credits: false,
            }
        );
    }

    #[test]
    fn assess_credential_quota_does_not_block_when_credit_tracking_is_unavailable() {
        let mut limit = limit_model();
        limit.has_credits = Some(false);
        limit.unlimited = Some(false);

        let assessment = assess_credential_quota(&[limit], timestamp(1_700_000_000));

        assert_eq!(
            assessment,
            CredentialQuotaAssessment {
                available: true,
                next_retry_at: None,
                remaining_percent: None,
                has_available_credits: false,
            }
        );
    }

    #[test]
    fn assess_credential_quota_temporarily_blocks_unknown_full_windows() {
        let mut limit = limit_model();
        limit.primary_used_percent = Some(100.0);

        let assessment = assess_credential_quota(&[limit], timestamp(1_700_000_000));

        assert_eq!(
            assessment,
            CredentialQuotaAssessment {
                available: false,
                next_retry_at: Some(timestamp(1_700_000_300)),
                remaining_percent: Some(0.0),
                has_available_credits: false,
            }
        );
    }

    #[test]
    fn apply_rate_limit_snapshot_preserves_existing_metadata_when_missing() {
        let mut existing = limit_model();
        existing.limit_name = Some("codex".to_string());
        existing.has_credits = Some(true);
        existing.unlimited = Some(false);
        existing.balance = Some("12".to_string());
        existing.plan_type = Some("pro".to_string());

        let mut active = credential_limit::ActiveModel::from(existing);
        apply_rate_limit_snapshot_to_active_model(
            &mut active,
            RateLimitSnapshot {
                limit_id: Some("codex".to_string()),
                limit_name: None,
                primary: Some(RateLimitWindow {
                    used_percent: 42.0,
                    window_minutes: Some(60),
                    resets_at: Some(1_700_000_300),
                }),
                secondary: None,
                credits: None,
                plan_type: None,
            },
        );

        assert_eq!(
            active.limit_name.take().flatten(),
            Some("codex".to_string())
        );
        assert_eq!(active.has_credits.take().flatten(), Some(true));
        assert_eq!(active.unlimited.take().flatten(), Some(false));
        assert_eq!(active.balance.take().flatten(), Some("12".to_string()));
        assert_eq!(active.plan_type.take().flatten(), Some("pro".to_string()));
        assert_eq!(active.primary_used_percent.take().flatten(), Some(42.0));
        assert_eq!(active.primary_window_minutes.take().flatten(), Some(60));
        assert_eq!(
            active.primary_resets_at.take().flatten(),
            Some(timestamp(1_700_000_300))
        );
    }

    #[test]
    fn apply_rate_limit_snapshot_overwrites_metadata_when_present() {
        let mut active = credential_limit::ActiveModel::from(limit_model());
        apply_rate_limit_snapshot_to_active_model(
            &mut active,
            RateLimitSnapshot {
                limit_id: Some("codex".to_string()),
                limit_name: Some("codex_other".to_string()),
                primary: None,
                secondary: None,
                credits: Some(CreditsSnapshot {
                    has_credits: false,
                    unlimited: true,
                    balance: Some("99".to_string()),
                }),
                plan_type: None,
            },
        );

        assert_eq!(
            active.limit_name.take().flatten(),
            Some("codex_other".to_string())
        );
        assert_eq!(active.has_credits.take().flatten(), Some(false));
        assert_eq!(active.unlimited.take().flatten(), Some(true));
        assert_eq!(active.balance.take().flatten(), Some("99".to_string()));
    }
}
