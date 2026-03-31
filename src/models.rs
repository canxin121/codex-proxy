use crate::entities::api_key;
use crate::entities::auth_session;
use crate::entities::credential;
use crate::entities::credential_limit;
use crate::entities::request_record;
use chrono::DateTime;
use chrono::Utc;
use codex_login::AuthDotJson;
use codex_login::AuthMode as LoginAuthMode;
use codex_login::TokenData;
use sea_orm::FromQueryResult;
use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CredentialKind {
    ChatgptAuth,
}

impl CredentialKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ChatgptAuth => "chatgpt_auth",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "chatgpt_auth" => Some(Self::ChatgptAuth),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthMethod {
    Browser,
    DeviceCode,
}

impl AuthMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Browser => "browser",
            Self::DeviceCode => "device_code",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "browser" => Some(Self::Browser),
            "device_code" => Some(Self::DeviceCode),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthStatus {
    Pending,
    Completed,
    Failed,
    Cancelled,
}

impl AuthStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct AdminLoginRequest {
    #[serde(rename = "admin_password")]
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct AdminSessionView {
    pub principal_kind: String,
    #[serde(rename = "api_key_id")]
    pub api_key_id: Option<String>,
    #[serde(rename = "api_key_name")]
    pub api_key_name: Option<String>,
    #[serde(rename = "console_refresh_interval_seconds")]
    pub console_refresh_interval_seconds: i32,
    #[serde(rename = "admin_session_created_at")]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(rename = "admin_session_last_used_at")]
    pub last_used_at: Option<DateTime<Utc>>,
    #[serde(rename = "admin_session_expires_at")]
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct AdminLoginResponse {
    #[serde(rename = "admin_session_token")]
    pub session_token: String,
    #[serde(rename = "admin_session")]
    pub session: AdminSessionView,
}

#[derive(Debug, Deserialize, Default)]
pub struct CreateCredentialRequest {}

#[derive(Debug, Serialize)]
pub struct ExportCredentialJsonResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_mode: Option<LoginAuthMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<TokenData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_refresh: Option<DateTime<Utc>>,
    #[serde(rename = "chatgpt_account_email")]
    pub account_email: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ImportCredentialJsonRequest {
    #[serde(default)]
    pub auth_mode: Option<LoginAuthMode>,
    #[serde(default)]
    pub tokens: Option<TokenData>,
    #[serde(default)]
    pub last_refresh: Option<DateTime<Utc>>,
}

impl ImportCredentialJsonRequest {
    pub fn into_auth_dot_json(self) -> AuthDotJson {
        AuthDotJson {
            auth_mode: self.auth_mode,
            openai_api_key: None,
            tokens: self.tokens,
            last_refresh: self.last_refresh,
        }
    }
}

#[derive(Debug, Deserialize, Default, Clone, Copy)]
pub struct PaginationQuery {
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateCredentialRequest {
    #[serde(rename = "credential_name")]
    pub name: Option<String>,
    #[serde(rename = "is_enabled")]
    pub enabled: Option<bool>,
    #[serde(rename = "load_balance_weight")]
    pub selection_weight: Option<i32>,
    #[serde(rename = "credential_notes")]
    pub notes: Option<String>,
    pub upstream_base_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct StartBrowserAuthRequest {
    #[serde(rename = "credential_id")]
    pub credential_id: String,
}

#[derive(Debug, Deserialize)]
pub struct CompleteBrowserAuthRequest {
    #[serde(rename = "callback_url")]
    pub callback_url: String,
}

#[derive(Debug, Deserialize)]
pub struct StartDeviceCodeAuthRequest {
    #[serde(rename = "credential_id")]
    pub credential_id: String,
}

#[derive(Debug, Deserialize, Default)]
pub struct ListRequestRecordsQuery {
    pub limit: Option<u64>,
    pub offset: Option<u64>,
    #[serde(rename = "credential_id")]
    pub credential_id: Option<String>,
    #[serde(rename = "api_key_id")]
    pub api_key_id: Option<String>,
    #[serde(rename = "only_failures")]
    pub only_failures: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
pub struct UsageStatsQuery {
    #[serde(rename = "credential_id")]
    pub credential_id: Option<String>,
    #[serde(rename = "api_key_id")]
    pub api_key_id: Option<String>,
    #[serde(rename = "only_failures")]
    pub only_failures: Option<bool>,
    pub top: Option<u64>,
}

#[derive(Debug, Serialize, Clone)]
pub struct PaginatedResponse<T> {
    pub items: Vec<T>,
    pub total: u64,
    pub limit: u64,
    pub offset: u64,
}

#[derive(Debug, Serialize)]
pub struct CredentialView {
    #[serde(rename = "credential_id")]
    pub id: String,
    #[serde(rename = "credential_name")]
    pub name: String,
    #[serde(rename = "credential_auth_type")]
    pub kind: CredentialKind,
    #[serde(rename = "is_enabled")]
    pub enabled: bool,
    #[serde(rename = "load_balance_weight")]
    pub selection_weight: i32,
    #[serde(rename = "credential_notes")]
    pub notes: Option<String>,
    pub upstream_base_url: Option<String>,
    #[serde(rename = "chatgpt_account_id")]
    pub account_id: Option<String>,
    #[serde(rename = "chatgpt_account_email")]
    pub account_email: Option<String>,
    #[serde(rename = "chatgpt_plan_type")]
    pub plan_type: Option<String>,
    #[serde(rename = "credential_has_auth")]
    pub has_auth: bool,
    #[serde(rename = "has_auth_access_token")]
    pub has_access_token: bool,
    #[serde(rename = "has_auth_refresh_token")]
    pub has_refresh_token: bool,
    #[serde(rename = "last_credential_used_at")]
    pub last_used_at: Option<DateTime<Utc>>,
    pub last_limit_sync_at: Option<DateTime<Utc>>,
    #[serde(rename = "last_auth_refresh_at")]
    pub last_refresh_at: Option<DateTime<Utc>>,
    #[serde(rename = "last_upstream_error")]
    pub last_error: Option<String>,
    #[serde(rename = "upstream_failure_count")]
    pub failure_count: i32,
    #[serde(rename = "active_request_count")]
    pub active_requests: usize,
    #[serde(rename = "credential_auth_home")]
    pub auth_home: String,
    #[serde(rename = "credential_limits")]
    pub limits: Vec<CredentialLimitView>,
    #[serde(rename = "request_stats")]
    pub request_stats: RequestStatsSummaryView,
    #[serde(rename = "last_request_error")]
    pub last_request_error: Option<LastRequestErrorView>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct CredentialLimitView {
    #[serde(rename = "rate_limit_id")]
    pub limit_id: String,
    #[serde(rename = "rate_limit_name")]
    pub limit_name: Option<String>,
    #[serde(rename = "primary_window_used_percent")]
    pub primary_used_percent: Option<f64>,
    #[serde(rename = "primary_window_minutes")]
    pub primary_window_minutes: Option<i64>,
    #[serde(rename = "primary_window_resets_at")]
    pub primary_resets_at: Option<DateTime<Utc>>,
    #[serde(rename = "secondary_window_used_percent")]
    pub secondary_used_percent: Option<f64>,
    #[serde(rename = "secondary_window_minutes")]
    pub secondary_window_minutes: Option<i64>,
    #[serde(rename = "secondary_window_resets_at")]
    pub secondary_resets_at: Option<DateTime<Utc>>,
    #[serde(rename = "has_available_credits")]
    pub has_credits: Option<bool>,
    #[serde(rename = "is_unlimited")]
    pub unlimited: Option<bool>,
    #[serde(rename = "credit_balance_text")]
    pub balance: Option<String>,
    #[serde(rename = "limit_plan_type")]
    pub plan_type: Option<String>,
    #[serde(rename = "limit_updated_at")]
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct AuthSessionView {
    #[serde(rename = "auth_session_id")]
    pub id: String,
    #[serde(rename = "credential_id")]
    pub credential_id: String,
    #[serde(rename = "auth_method")]
    pub method: AuthMethod,
    #[serde(rename = "auth_status")]
    pub status: AuthStatus,
    #[serde(rename = "authorization_url")]
    pub authorization_url: Option<String>,
    #[serde(rename = "auth_redirect_url")]
    pub redirect_uri: Option<String>,
    #[serde(rename = "verification_url")]
    pub verification_url: Option<String>,
    #[serde(rename = "user_code")]
    pub user_code: Option<String>,
    #[serde(rename = "device_code_interval_seconds")]
    pub device_code_interval_seconds: Option<i32>,
    #[serde(rename = "auth_error")]
    pub error_message: Option<String>,
    #[serde(rename = "auth_completed_at")]
    pub completed_at: Option<DateTime<Utc>>,
    #[serde(rename = "auth_created_at")]
    pub created_at: DateTime<Utc>,
    #[serde(rename = "auth_updated_at")]
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Default, Clone)]
pub struct RequestUsageTotalsView {
    #[serde(rename = "read_input_tokens")]
    pub input_tokens: i64,
    #[serde(rename = "cache_read_input_tokens")]
    pub cached_input_tokens: i64,
    #[serde(rename = "write_output_tokens")]
    pub output_tokens: i64,
    #[serde(rename = "write_reasoning_tokens")]
    pub reasoning_output_tokens: i64,
    #[serde(rename = "all_tokens")]
    pub total_tokens: i64,
    #[serde(rename = "billable_input_tokens")]
    pub billable_input_tokens: i64,
}

#[derive(Debug, Serialize, Default, Clone)]
pub struct RequestStatsSummaryView {
    #[serde(rename = "total_request_count")]
    pub total_request_count: i64,
    #[serde(rename = "success_request_count")]
    pub success_request_count: i64,
    #[serde(rename = "failure_request_count")]
    pub failure_request_count: i64,
    #[serde(rename = "http_request_count")]
    pub http_request_count: i64,
    #[serde(rename = "websocket_request_count")]
    pub websocket_request_count: i64,
    #[serde(rename = "first_request_at")]
    pub first_request_at: Option<DateTime<Utc>>,
    #[serde(rename = "last_request_at")]
    pub last_request_at: Option<DateTime<Utc>>,
    #[serde(rename = "last_success_at")]
    pub last_success_at: Option<DateTime<Utc>>,
    #[serde(rename = "last_failure_at")]
    pub last_failure_at: Option<DateTime<Utc>>,
    #[serde(rename = "token_usage")]
    pub usage: RequestUsageTotalsView,
}

#[derive(Debug, Serialize, Default, Clone)]
pub struct RequestDurationStatsView {
    #[serde(rename = "average_duration_ms")]
    pub average_duration_ms: Option<f64>,
    #[serde(rename = "max_duration_ms")]
    pub max_duration_ms: Option<i64>,
}

#[derive(Debug, Serialize, Default, Clone)]
pub struct UsageTimeBucketView {
    pub bucket: String,
    #[serde(rename = "total_request_count")]
    pub total_request_count: i64,
    #[serde(rename = "success_request_count")]
    pub success_request_count: i64,
    #[serde(rename = "failure_request_count")]
    pub failure_request_count: i64,
    #[serde(rename = "token_usage")]
    pub usage: RequestUsageTotalsView,
}

#[derive(Debug, Serialize, Default, Clone)]
pub struct RequestBreakdownView {
    pub key: String,
    pub label: String,
    #[serde(rename = "total_request_count")]
    pub total_request_count: i64,
    #[serde(rename = "success_request_count")]
    pub success_request_count: i64,
    #[serde(rename = "failure_request_count")]
    pub failure_request_count: i64,
    #[serde(rename = "last_request_at")]
    pub last_request_at: Option<DateTime<Utc>>,
    #[serde(rename = "average_duration_ms")]
    pub average_duration_ms: Option<f64>,
    #[serde(rename = "max_duration_ms")]
    pub max_duration_ms: Option<i64>,
    #[serde(rename = "token_usage")]
    pub usage: RequestUsageTotalsView,
}

#[derive(Debug, Serialize, Default, Clone)]
pub struct CredentialModelBreakdownView {
    pub credential: RequestBreakdownView,
    pub models: Vec<RequestBreakdownView>,
}

#[derive(Debug, Serialize, Default, Clone)]
pub struct UsageStatsFiltersView {
    #[serde(rename = "credential_id")]
    pub credential_id: Option<String>,
    #[serde(rename = "api_key_id")]
    pub api_key_id: Option<String>,
    #[serde(rename = "only_failures")]
    pub only_failures: bool,
    pub top: u64,
}

#[derive(Debug, Serialize, Default, Clone)]
pub struct UsageStatsView {
    #[serde(rename = "generated_at")]
    pub generated_at: DateTime<Utc>,
    pub filters: UsageStatsFiltersView,
    pub summary: RequestStatsSummaryView,
    pub duration: RequestDurationStatsView,
    pub hourly: Vec<UsageTimeBucketView>,
    pub daily: Vec<UsageTimeBucketView>,
    pub credentials: Vec<RequestBreakdownView>,
    #[serde(rename = "credential_model_groups")]
    pub credential_model_groups: Vec<CredentialModelBreakdownView>,
    #[serde(rename = "api_keys")]
    pub api_keys: Vec<RequestBreakdownView>,
    pub models: Vec<RequestBreakdownView>,
    pub paths: Vec<RequestBreakdownView>,
    pub transports: Vec<RequestBreakdownView>,
    #[serde(rename = "status_codes")]
    pub status_codes: Vec<RequestBreakdownView>,
    #[serde(rename = "error_phases")]
    pub error_phases: Vec<RequestBreakdownView>,
}

#[derive(Debug, Serialize, Clone)]
pub struct LastRequestErrorView {
    #[serde(rename = "request_id")]
    pub request_id: String,
    #[serde(rename = "credential_id")]
    pub credential_id: String,
    #[serde(rename = "credential_name")]
    pub credential_name: String,
    #[serde(rename = "api_key_id")]
    pub api_key_id: Option<String>,
    #[serde(rename = "api_key_name")]
    pub api_key_name: Option<String>,
    #[serde(rename = "principal_kind")]
    pub principal_kind: String,
    #[serde(rename = "request_transport")]
    pub transport: String,
    #[serde(rename = "request_method")]
    pub method: String,
    #[serde(rename = "request_path")]
    pub path: String,
    #[serde(rename = "status_code")]
    pub status_code: Option<i32>,
    #[serde(rename = "error_phase")]
    pub error_phase: Option<String>,
    #[serde(rename = "error_code")]
    pub error_code: Option<String>,
    #[serde(rename = "error_message")]
    pub error_message: Option<String>,
    #[serde(rename = "error_at")]
    pub error_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, Clone)]
pub struct RequestRecordView {
    #[serde(rename = "request_id")]
    pub id: String,
    #[serde(rename = "credential_id")]
    pub credential_id: String,
    #[serde(rename = "credential_name")]
    pub credential_name: String,
    #[serde(rename = "api_key_id")]
    pub api_key_id: Option<String>,
    #[serde(rename = "api_key_name")]
    pub api_key_name: Option<String>,
    #[serde(rename = "principal_kind")]
    pub principal_kind: String,
    #[serde(rename = "request_transport")]
    pub transport: String,
    #[serde(rename = "request_method")]
    pub method: String,
    #[serde(rename = "request_path")]
    pub path: String,
    #[serde(rename = "requested_model")]
    pub requested_model: Option<String>,
    #[serde(rename = "response_id")]
    pub response_id: Option<String>,
    #[serde(rename = "status_code")]
    pub status_code: Option<i32>,
    #[serde(rename = "request_success")]
    pub success: Option<bool>,
    #[serde(rename = "error_phase")]
    pub error_phase: Option<String>,
    #[serde(rename = "error_code")]
    pub error_code: Option<String>,
    #[serde(rename = "error_message")]
    pub error_message: Option<String>,
    #[serde(rename = "request_started_at")]
    pub started_at: DateTime<Utc>,
    #[serde(rename = "request_completed_at")]
    pub completed_at: Option<DateTime<Utc>>,
    #[serde(rename = "request_duration_ms")]
    pub duration_ms: Option<i64>,
    #[serde(rename = "token_usage")]
    pub usage: RequestUsageTotalsView,
    #[serde(rename = "usage_json")]
    pub usage_json: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct StatsOverviewView {
    #[serde(rename = "generated_at")]
    pub generated_at: DateTime<Utc>,
    #[serde(rename = "active_request_count")]
    pub active_request_count: usize,
    #[serde(rename = "enabled_credential_count")]
    pub enabled_credential_count: i64,
    #[serde(rename = "authenticated_credential_count")]
    pub authenticated_credential_count: i64,
    #[serde(rename = "total_api_key_count")]
    pub total_api_key_count: i64,
    #[serde(rename = "enabled_api_key_count")]
    pub enabled_api_key_count: i64,
    #[serde(rename = "pending_auth_session_count")]
    pub pending_auth_session_count: i64,
    #[serde(rename = "request_stats")]
    pub request_stats: RequestStatsSummaryView,
    #[serde(rename = "latest_request_errors")]
    pub latest_request_errors: Vec<LastRequestErrorView>,
}

#[derive(Debug, Deserialize)]
pub struct CreateApiKeyRequest {
    #[serde(rename = "api_key_name")]
    pub name: String,
    #[serde(rename = "has_admin_access")]
    pub is_admin: Option<bool>,
    #[serde(rename = "api_key_expires_at")]
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateApiKeyRequest {
    #[serde(rename = "api_key_name")]
    pub name: Option<String>,
    #[serde(rename = "is_enabled")]
    pub enabled: Option<bool>,
    #[serde(rename = "api_key_expires_at")]
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct ApiKeyView {
    #[serde(rename = "api_key_id")]
    pub id: String,
    #[serde(rename = "api_key_name")]
    pub name: String,
    #[serde(rename = "is_enabled")]
    pub enabled: bool,
    #[serde(rename = "has_admin_access")]
    pub is_admin: bool,
    #[serde(rename = "api_key_expires_at")]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(rename = "last_api_key_used_at")]
    pub last_used_at: Option<DateTime<Utc>>,
    #[serde(rename = "request_stats")]
    pub request_stats: RequestStatsSummaryView,
    #[serde(rename = "last_request_error")]
    pub last_request_error: Option<LastRequestErrorView>,
    #[serde(rename = "api_key_created_at")]
    pub created_at: DateTime<Utc>,
    #[serde(rename = "api_key_updated_at")]
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct CreateApiKeyResponse {
    pub api_key_value: String,
    pub api_key_record: ApiKeyView,
}

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
}

#[derive(Debug, Clone, Default, FromQueryResult)]
pub struct RequestStatsAggregateRow {
    pub total_request_count: Option<i64>,
    pub success_request_count: Option<i64>,
    pub failure_request_count: Option<i64>,
    pub http_request_count: Option<i64>,
    pub websocket_request_count: Option<i64>,
    pub first_request_at: Option<DateTime<Utc>>,
    pub last_request_at: Option<DateTime<Utc>>,
    pub last_success_at: Option<DateTime<Utc>>,
    pub last_failure_at: Option<DateTime<Utc>>,
    pub input_tokens: Option<i64>,
    pub cached_input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub reasoning_output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
}

#[derive(Debug, Clone, Default, FromQueryResult)]
pub struct RequestDurationAggregateRow {
    pub average_duration_ms: Option<f64>,
    pub max_duration_ms: Option<i64>,
}

#[derive(Debug, Clone, Default, FromQueryResult)]
pub struct UsageTimeBucketRow {
    pub bucket: Option<String>,
    pub total_request_count: Option<i64>,
    pub success_request_count: Option<i64>,
    pub failure_request_count: Option<i64>,
    pub input_tokens: Option<i64>,
    pub cached_input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub reasoning_output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
}

#[derive(Debug, Clone, Default, FromQueryResult)]
pub struct RequestBreakdownRow {
    pub group_key: Option<String>,
    pub group_label: Option<String>,
    pub total_request_count: Option<i64>,
    pub success_request_count: Option<i64>,
    pub failure_request_count: Option<i64>,
    pub last_request_at: Option<DateTime<Utc>>,
    pub average_duration_ms: Option<f64>,
    pub max_duration_ms: Option<i64>,
    pub input_tokens: Option<i64>,
    pub cached_input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub reasoning_output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
}

impl CredentialLimitView {
    pub fn from_model(model: credential_limit::Model) -> Self {
        Self {
            limit_id: model.limit_id,
            limit_name: model.limit_name,
            primary_used_percent: model.primary_used_percent,
            primary_window_minutes: model.primary_window_minutes,
            primary_resets_at: model.primary_resets_at,
            secondary_used_percent: model.secondary_used_percent,
            secondary_window_minutes: model.secondary_window_minutes,
            secondary_resets_at: model.secondary_resets_at,
            has_credits: model.has_credits,
            unlimited: model.unlimited,
            balance: model.balance,
            plan_type: model.plan_type,
            updated_at: model.updated_at,
        }
    }
}

impl AuthSessionView {
    pub fn from_model(model: auth_session::Model) -> Option<Self> {
        Some(Self {
            id: model.id,
            credential_id: model.credential_id,
            method: AuthMethod::from_str(&model.method)?,
            status: AuthStatus::from_str(&model.status)?,
            authorization_url: model.authorization_url,
            redirect_uri: model.redirect_uri,
            verification_url: model.verification_url,
            user_code: model.user_code,
            device_code_interval_seconds: model.device_code_interval_seconds,
            error_message: model.error_message,
            completed_at: model.completed_at,
            created_at: model.created_at,
            updated_at: model.updated_at,
        })
    }
}

impl RequestUsageTotalsView {
    pub fn from_numbers(
        input_tokens: i64,
        cached_input_tokens: i64,
        output_tokens: i64,
        reasoning_output_tokens: i64,
        total_tokens: i64,
    ) -> Self {
        Self {
            input_tokens,
            cached_input_tokens,
            output_tokens,
            reasoning_output_tokens,
            total_tokens,
            billable_input_tokens: (input_tokens - cached_input_tokens).max(0),
        }
    }
}

impl RequestStatsSummaryView {
    pub fn from_aggregate(row: RequestStatsAggregateRow) -> Self {
        let input_tokens = row.input_tokens.unwrap_or(0);
        let cached_input_tokens = row.cached_input_tokens.unwrap_or(0);
        let output_tokens = row.output_tokens.unwrap_or(0);
        let reasoning_output_tokens = row.reasoning_output_tokens.unwrap_or(0);
        let total_tokens = row.total_tokens.unwrap_or(0);

        Self {
            total_request_count: row.total_request_count.unwrap_or(0),
            success_request_count: row.success_request_count.unwrap_or(0),
            failure_request_count: row.failure_request_count.unwrap_or(0),
            http_request_count: row.http_request_count.unwrap_or(0),
            websocket_request_count: row.websocket_request_count.unwrap_or(0),
            first_request_at: row.first_request_at,
            last_request_at: row.last_request_at,
            last_success_at: row.last_success_at,
            last_failure_at: row.last_failure_at,
            usage: RequestUsageTotalsView::from_numbers(
                input_tokens,
                cached_input_tokens,
                output_tokens,
                reasoning_output_tokens,
                total_tokens,
            ),
        }
    }
}

impl RequestDurationStatsView {
    pub fn from_aggregate(row: RequestDurationAggregateRow) -> Self {
        Self {
            average_duration_ms: row.average_duration_ms,
            max_duration_ms: row.max_duration_ms,
        }
    }
}

impl UsageTimeBucketView {
    pub fn from_row(row: UsageTimeBucketRow) -> Self {
        let bucket = row.bucket.unwrap_or_else(|| "unknown".to_string());
        let input_tokens = row.input_tokens.unwrap_or(0);
        let cached_input_tokens = row.cached_input_tokens.unwrap_or(0);
        let output_tokens = row.output_tokens.unwrap_or(0);
        let reasoning_output_tokens = row.reasoning_output_tokens.unwrap_or(0);
        let total_tokens = row.total_tokens.unwrap_or(0);

        Self {
            bucket,
            total_request_count: row.total_request_count.unwrap_or(0),
            success_request_count: row.success_request_count.unwrap_or(0),
            failure_request_count: row.failure_request_count.unwrap_or(0),
            usage: RequestUsageTotalsView::from_numbers(
                input_tokens,
                cached_input_tokens,
                output_tokens,
                reasoning_output_tokens,
                total_tokens,
            ),
        }
    }
}

impl RequestBreakdownView {
    pub fn from_row(row: RequestBreakdownRow) -> Self {
        let input_tokens = row.input_tokens.unwrap_or(0);
        let cached_input_tokens = row.cached_input_tokens.unwrap_or(0);
        let output_tokens = row.output_tokens.unwrap_or(0);
        let reasoning_output_tokens = row.reasoning_output_tokens.unwrap_or(0);
        let total_tokens = row.total_tokens.unwrap_or(0);

        Self {
            key: row.group_key.unwrap_or_else(|| "unknown".to_string()),
            label: row.group_label.unwrap_or_else(|| "unknown".to_string()),
            total_request_count: row.total_request_count.unwrap_or(0),
            success_request_count: row.success_request_count.unwrap_or(0),
            failure_request_count: row.failure_request_count.unwrap_or(0),
            last_request_at: row.last_request_at,
            average_duration_ms: row.average_duration_ms,
            max_duration_ms: row.max_duration_ms,
            usage: RequestUsageTotalsView::from_numbers(
                input_tokens,
                cached_input_tokens,
                output_tokens,
                reasoning_output_tokens,
                total_tokens,
            ),
        }
    }
}

impl LastRequestErrorView {
    pub fn from_model(model: request_record::Model) -> Self {
        Self {
            request_id: model.id,
            credential_id: model.credential_id,
            credential_name: model.credential_name,
            api_key_id: model.api_key_id,
            api_key_name: model.api_key_name,
            principal_kind: model.principal_kind,
            transport: model.transport,
            method: model.request_method,
            path: model.request_path,
            status_code: model.upstream_status_code,
            error_phase: model.error_phase,
            error_code: model.error_code,
            error_message: model.error_message,
            error_at: model
                .request_completed_at
                .or(Some(model.request_started_at)),
        }
    }
}

impl RequestRecordView {
    pub fn from_model(model: request_record::Model) -> Self {
        let usage_json = model
            .usage_json
            .as_deref()
            .and_then(|value| serde_json::from_str(value).ok());

        Self {
            id: model.id,
            credential_id: model.credential_id,
            credential_name: model.credential_name,
            api_key_id: model.api_key_id,
            api_key_name: model.api_key_name,
            principal_kind: model.principal_kind,
            transport: model.transport,
            method: model.request_method,
            path: model.request_path,
            requested_model: model.requested_model,
            response_id: model.response_id,
            status_code: model.upstream_status_code,
            success: model.request_success,
            error_phase: model.error_phase,
            error_code: model.error_code,
            error_message: model.error_message,
            started_at: model.request_started_at,
            completed_at: model.request_completed_at,
            duration_ms: model.duration_ms,
            usage: RequestUsageTotalsView::from_numbers(
                model.input_tokens,
                model.cached_input_tokens,
                model.output_tokens,
                model.reasoning_output_tokens,
                model.total_tokens,
            ),
            usage_json,
        }
    }
}

impl ApiKeyView {
    pub fn from_model(
        model: api_key::Model,
        request_stats: RequestStatsSummaryView,
        last_request_error: Option<LastRequestErrorView>,
    ) -> Self {
        Self {
            id: model.id,
            name: model.name,
            enabled: model.enabled,
            is_admin: model.is_admin,
            expires_at: model.expires_at,
            last_used_at: model.last_used_at,
            request_stats,
            last_request_error,
            created_at: model.created_at,
            updated_at: model.updated_at,
        }
    }
}

impl CredentialView {
    pub fn from_model(
        model: credential::Model,
        auth_home: String,
        has_access_token: bool,
        has_refresh_token: bool,
        active_requests: usize,
        limits: Vec<credential_limit::Model>,
        request_stats: RequestStatsSummaryView,
        last_request_error: Option<LastRequestErrorView>,
    ) -> Option<Self> {
        let kind = CredentialKind::from_str(&model.kind)?;
        Some(Self {
            id: model.id,
            name: model.name,
            kind,
            enabled: model.enabled,
            selection_weight: model.selection_weight,
            notes: model.notes,
            upstream_base_url: model.upstream_base_url,
            account_id: model.account_id,
            account_email: model.account_email,
            plan_type: model.plan_type,
            has_auth: has_access_token || has_refresh_token,
            has_access_token,
            has_refresh_token,
            last_used_at: model.last_used_at,
            last_limit_sync_at: model.last_limit_sync_at,
            last_refresh_at: model.last_refresh_at,
            last_error: model.last_error,
            failure_count: model.failure_count,
            active_requests,
            auth_home,
            limits: limits
                .into_iter()
                .map(CredentialLimitView::from_model)
                .collect(),
            request_stats,
            last_request_error,
            created_at: model.created_at,
            updated_at: model.updated_at,
        })
    }
}
