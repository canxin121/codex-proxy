use crate::entities::api_key;
use crate::entities::auth_session;
use crate::entities::credential;
use crate::entities::request_record;
use crate::error::AppError;
use crate::models::AuthStatus;
use crate::models::CredentialModelBreakdownView;
use crate::models::LastRequestErrorView;
use crate::models::ListRequestRecordsQuery;
use crate::models::RequestBreakdownRow;
use crate::models::RequestBreakdownView;
use crate::models::RequestDurationAggregateRow;
use crate::models::RequestDurationStatsView;
use crate::models::RequestRecordView;
use crate::models::RequestStatsAggregateRow;
use crate::models::RequestStatsSummaryView;
use crate::models::StatsOverviewView;
use crate::models::UsageStatsFiltersView;
use crate::models::UsageStatsQuery;
use crate::models::UsageStatsView;
use crate::models::UsageTimeBucketRow;
use crate::models::UsageTimeBucketView;
use crate::state::AppState;
use crate::state::AuthenticatedPrincipal;
use axum::http::StatusCode;
use chrono::DateTime;
use chrono::Utc;
use codex_protocol::protocol::TokenUsage;
use sea_orm::ActiveModelTrait;
use sea_orm::ColumnTrait;
use sea_orm::EntityTrait;
use sea_orm::PaginatorTrait;
use sea_orm::QueryFilter;
use sea_orm::QueryOrder;
use sea_orm::QuerySelect;
use sea_orm::Select;
use sea_orm::Set;
use sea_orm::sea_query::Alias;
use sea_orm::sea_query::BinOper;
use sea_orm::sea_query::Condition;
use sea_orm::sea_query::Expr;
use sea_orm::sea_query::Func;
use sea_orm::sea_query::SimpleExpr;
use serde_json::Value;
use tokio::runtime::Handle;
use uuid::Uuid;

const DEFAULT_REQUEST_RECORD_LIMIT: u64 = 100;
const MAX_REQUEST_RECORD_LIMIT: u64 = 1_000;
const DEFAULT_USAGE_BREAKDOWN_LIMIT: u64 = 8;
const MAX_USAGE_BREAKDOWN_LIMIT: u64 = 20;

#[derive(Copy, Clone)]
enum TimeBucket {
    Hour,
    Day,
}

impl TimeBucket {
    fn expression(self) -> SimpleExpr {
        match self {
            Self::Hour => Func::cust(Alias::new("substr"))
                .arg(Expr::col(request_record::Column::RequestStartedAt))
                .arg(12)
                .arg(2)
                .into(),
            Self::Day => Func::cust(Alias::new("substr"))
                .arg(Expr::col(request_record::Column::RequestStartedAt))
                .arg(1)
                .arg(10)
                .into(),
        }
    }

    fn normalize_label(self, value: Option<String>) -> String {
        let value = value.unwrap_or_else(|| "unknown".to_string());
        match self {
            Self::Hour => format!("{value}:00"),
            Self::Day => value,
        }
    }
}

#[derive(Copy, Clone)]
enum BreakdownDimension {
    Credential,
    ApiKey,
    Model,
    Path,
    Transport,
    StatusCode,
    ErrorPhase,
}

impl BreakdownDimension {
    fn expressions(self) -> (SimpleExpr, SimpleExpr) {
        match self {
            Self::Credential => (
                Expr::col(request_record::Column::CredentialId).into(),
                Expr::col(request_record::Column::CredentialName).into(),
            ),
            Self::ApiKey => (
                Expr::col(request_record::Column::ApiKeyId).if_null("system"),
                Expr::col(request_record::Column::ApiKeyName).if_null("system/admin"),
            ),
            Self::Model => (
                Expr::col(request_record::Column::RequestedModel).if_null("unknown"),
                Expr::col(request_record::Column::RequestedModel).if_null("unknown"),
            ),
            Self::Path => (path_expression(), path_expression()),
            Self::Transport => (
                Expr::col(request_record::Column::Transport).into(),
                Expr::col(request_record::Column::Transport).into(),
            ),
            Self::StatusCode => (
                Expr::expr(Func::cast_as(
                    Expr::col(request_record::Column::UpstreamStatusCode),
                    Alias::new("TEXT"),
                ))
                .if_null("unknown"),
                Expr::expr(Func::cast_as(
                    Expr::col(request_record::Column::UpstreamStatusCode),
                    Alias::new("TEXT"),
                ))
                .if_null("unknown"),
            ),
            Self::ErrorPhase => (
                Expr::col(request_record::Column::ErrorPhase).if_null("unknown"),
                Expr::col(request_record::Column::ErrorPhase).if_null("unknown"),
            ),
        }
    }

    fn forces_failure_scope(self) -> bool {
        matches!(self, Self::ErrorPhase)
    }
}

#[derive(Clone)]
pub struct RequestRecordStart {
    pub principal: AuthenticatedPrincipal,
    pub credential: credential::Model,
    pub transport: &'static str,
    pub method: String,
    pub path: String,
    pub requested_model: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RequestRecordFinalization {
    pub upstream_status_code: Option<i32>,
    pub request_success: bool,
    pub error_phase: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub response_id: Option<String>,
    pub requested_model: Option<String>,
    pub usage: Option<TokenUsage>,
    pub usage_json: Option<Value>,
}

impl RequestRecordFinalization {
    pub fn proxy_aborted() -> Self {
        Self {
            upstream_status_code: None,
            request_success: false,
            error_phase: Some("proxy_aborted".to_string()),
            error_code: Some("proxy_stream_aborted".to_string()),
            error_message: Some("request stream dropped before completion".to_string()),
            response_id: None,
            requested_model: None,
            usage: None,
            usage_json: None,
        }
    }
}

#[derive(Clone)]
struct PendingRequestRecord {
    state: AppState,
    request_id: String,
    started_at: DateTime<Utc>,
}

pub struct RequestRecordGuard {
    pending: Option<PendingRequestRecord>,
}

impl RequestRecordGuard {
    pub async fn finalize(
        &mut self,
        finalization: RequestRecordFinalization,
    ) -> Result<(), AppError> {
        let Some(pending) = self.pending.take() else {
            return Ok(());
        };
        finalize_request_record_impl(pending, finalization).await
    }
}

impl Drop for RequestRecordGuard {
    fn drop(&mut self) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        let fallback = RequestRecordFinalization::proxy_aborted();
        if let Ok(handle) = Handle::try_current() {
            handle.spawn(async move {
                let _ = finalize_request_record_impl(pending, fallback).await;
            });
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RequestObservation {
    requested_model: Option<String>,
    response_id: Option<String>,
    usage: Option<TokenUsage>,
    usage_json: Option<Value>,
    success: Option<bool>,
    error_phase: Option<String>,
    error_code: Option<String>,
    error_message: Option<String>,
    upstream_status_code: Option<i32>,
}

impl RequestObservation {
    pub fn new(requested_model: Option<String>) -> Self {
        Self {
            requested_model,
            ..Self::default()
        }
    }

    pub fn set_requested_model_if_missing(&mut self, requested_model: Option<String>) {
        if self.requested_model.is_none() {
            self.requested_model = requested_model;
        }
    }

    pub fn observe_json_value(&mut self, value: &Value) {
        self.observe_event_value(None, value);
    }

    pub fn observe_event_value(&mut self, explicit_kind: Option<&str>, value: &Value) {
        if self.response_id.is_none() {
            self.response_id = extract_response_id(value);
        }
        if self.requested_model.is_none() {
            self.requested_model = extract_requested_model_from_value(value);
        }
        if let Some(usage_json) = extract_usage_json(value) {
            self.usage = parse_token_usage(&usage_json);
            self.usage_json = Some(usage_json);
        }

        let event_kind = explicit_kind
            .or_else(|| value.get("type").and_then(Value::as_str))
            .or_else(|| {
                response_container(value)
                    .get("status")
                    .and_then(Value::as_str)
                    .map(|status| match status {
                        "completed" => "response.completed",
                        "failed" => "response.failed",
                        _ => "",
                    })
                    .filter(|kind| !kind.is_empty())
            });

        match event_kind {
            Some("response.completed") => {
                self.success = Some(true);
            }
            Some("response.failed") => {
                self.mark_failure(
                    "upstream_response",
                    extract_error_code(value),
                    extract_error_message(value)
                        .unwrap_or_else(|| "upstream response failed".to_string()),
                    extract_status_code(value),
                );
            }
            Some("response.incomplete") => {
                let reason = extract_incomplete_reason(value)
                    .unwrap_or_else(|| "response incomplete".to_string());
                self.mark_failure(
                    "upstream_response",
                    Some("response_incomplete".to_string()),
                    reason,
                    extract_status_code(value),
                );
            }
            Some("error") => {
                self.mark_failure(
                    "upstream_transport",
                    extract_error_code(value),
                    extract_error_message(value)
                        .unwrap_or_else(|| "upstream returned an error event".to_string()),
                    extract_status_code(value),
                );
            }
            _ => {
                if response_container(value)
                    .get("status")
                    .and_then(Value::as_str)
                    == Some("failed")
                {
                    self.mark_failure(
                        "upstream_response",
                        extract_error_code(value),
                        extract_error_message(value)
                            .unwrap_or_else(|| "upstream response failed".to_string()),
                        extract_status_code(value),
                    );
                }
            }
        }
    }

    pub fn mark_failure(
        &mut self,
        error_phase: impl Into<String>,
        error_code: Option<String>,
        error_message: impl Into<String>,
        upstream_status_code: Option<i32>,
    ) {
        self.success = Some(false);
        self.error_phase = Some(error_phase.into());
        self.error_code = error_code;
        self.error_message = Some(error_message.into());
        if upstream_status_code.is_some() {
            self.upstream_status_code = upstream_status_code;
        }
    }

    pub fn mark_failure_if_missing(
        &mut self,
        error_phase: impl Into<String>,
        error_code: Option<String>,
        error_message: impl Into<String>,
        upstream_status_code: Option<i32>,
    ) {
        if self.success.is_some() {
            return;
        }
        self.mark_failure(error_phase, error_code, error_message, upstream_status_code);
    }

    pub fn is_terminal(&self) -> bool {
        self.success.is_some()
    }

    pub fn finish_http_response(mut self, status: StatusCode) -> RequestRecordFinalization {
        self.upstream_status_code = Some(i32::from(status.as_u16()));
        if self.success.is_none() {
            if status.is_success() {
                self.success = Some(true);
            } else {
                self.mark_failure(
                    "upstream_http_status",
                    None,
                    format!("upstream returned {status}"),
                    Some(i32::from(status.as_u16())),
                );
            }
        }
        self.into_finalization()
    }

    pub fn finalize(self) -> RequestRecordFinalization {
        self.into_finalization()
    }

    pub fn finish_sse_response(mut self, status: StatusCode) -> RequestRecordFinalization {
        self.upstream_status_code = Some(i32::from(status.as_u16()));
        if self.success.is_none() {
            if status.is_success() {
                self.mark_failure(
                    "upstream_stream",
                    Some("missing_response_completed".to_string()),
                    "stream ended before response.completed",
                    Some(i32::from(status.as_u16())),
                );
            } else {
                self.mark_failure(
                    "upstream_http_status",
                    None,
                    format!("upstream returned {status}"),
                    Some(i32::from(status.as_u16())),
                );
            }
        }
        self.into_finalization()
    }

    pub fn finish_websocket(mut self) -> RequestRecordFinalization {
        if self.success.is_none() {
            self.mark_failure(
                "upstream_websocket",
                Some("websocket_closed_before_completion".to_string()),
                "websocket closed before response.completed",
                self.upstream_status_code,
            );
        }
        self.into_finalization()
    }

    fn into_finalization(self) -> RequestRecordFinalization {
        RequestRecordFinalization {
            upstream_status_code: self.upstream_status_code,
            request_success: self.success.unwrap_or(false),
            error_phase: self.error_phase,
            error_code: self.error_code,
            error_message: self.error_message,
            response_id: self.response_id,
            requested_model: self.requested_model,
            usage: self.usage,
            usage_json: self.usage_json,
        }
    }
}

#[derive(Debug, Default)]
pub struct SseEventParser {
    buffer: String,
}

impl SseEventParser {
    pub fn feed(&mut self, bytes: &[u8], observation: &mut RequestObservation) {
        self.buffer.push_str(&String::from_utf8_lossy(bytes));
        if self.buffer.contains('\r') {
            self.buffer = self.buffer.replace("\r\n", "\n");
        }

        while let Some(frame_end) = self.buffer.find("\n\n") {
            let frame = self.buffer[..frame_end].to_string();
            self.buffer.drain(..frame_end + 2);
            process_sse_frame(&frame, observation);
        }
    }

    pub fn finish(&mut self, observation: &mut RequestObservation) {
        if self.buffer.trim().is_empty() {
            self.buffer.clear();
            return;
        }

        let frame = std::mem::take(&mut self.buffer);
        process_sse_frame(&frame, observation);
    }
}

pub async fn start_request_record(
    state: &AppState,
    start: RequestRecordStart,
) -> Result<RequestRecordGuard, AppError> {
    let now = Utc::now();
    let request_id = Uuid::new_v4().to_string();

    request_record::ActiveModel {
        id: Set(request_id.clone()),
        credential_id: Set(start.credential.id.clone()),
        credential_name: Set(start.credential.name.clone()),
        api_key_id: Set(start.principal.api_key_id.clone()),
        api_key_name: Set(start.principal.api_key_name.clone()),
        principal_kind: Set(start.principal.principal_kind.as_str().to_string()),
        transport: Set(start.transport.to_string()),
        request_method: Set(start.method),
        request_path: Set(start.path),
        upstream_status_code: Set(None),
        request_success: Set(None),
        error_phase: Set(None),
        error_code: Set(None),
        error_message: Set(None),
        response_id: Set(None),
        requested_model: Set(start.requested_model),
        input_tokens: Set(0),
        cached_input_tokens: Set(0),
        output_tokens: Set(0),
        reasoning_output_tokens: Set(0),
        total_tokens: Set(0),
        usage_json: Set(None),
        request_started_at: Set(now),
        request_completed_at: Set(None),
        duration_ms: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(state.db())
    .await
    .map_err(|err| AppError::internal(err.to_string()))?;

    Ok(RequestRecordGuard {
        pending: Some(PendingRequestRecord {
            state: state.clone(),
            request_id,
            started_at: now,
        }),
    })
}

pub async fn request_stats_overall(state: &AppState) -> Result<RequestStatsSummaryView, AppError> {
    request_stats_with_scope(state, None, None, false).await
}

pub async fn request_stats_for_credential(
    state: &AppState,
    credential_id: &str,
) -> Result<RequestStatsSummaryView, AppError> {
    request_stats_with_scope(state, Some(credential_id), None, false).await
}

pub async fn request_stats_for_api_key(
    state: &AppState,
    api_key_id: &str,
) -> Result<RequestStatsSummaryView, AppError> {
    request_stats_with_scope(state, None, Some(api_key_id), false).await
}

pub async fn usage_stats(
    state: &AppState,
    query: &UsageStatsQuery,
) -> Result<UsageStatsView, AppError> {
    let top = query
        .top
        .unwrap_or(DEFAULT_USAGE_BREAKDOWN_LIMIT)
        .clamp(1, MAX_USAGE_BREAKDOWN_LIMIT);
    let only_failures = query.only_failures.unwrap_or(false);
    let credential_id = query.credential_id.as_deref();
    let api_key_id = query.api_key_id.as_deref();

    let (
        summary,
        duration,
        hourly,
        daily,
        credentials,
        api_keys,
        models,
        paths,
        transports,
        status_codes,
        error_phases,
    ) = tokio::try_join!(
        request_stats_with_scope(state, credential_id, api_key_id, only_failures),
        duration_stats_with_scope(state, credential_id, api_key_id, only_failures),
        usage_time_buckets_with_scope(
            state,
            credential_id,
            api_key_id,
            only_failures,
            TimeBucket::Hour
        ),
        usage_time_buckets_with_scope(
            state,
            credential_id,
            api_key_id,
            only_failures,
            TimeBucket::Day
        ),
        request_breakdown_with_scope(
            state,
            credential_id,
            api_key_id,
            only_failures,
            BreakdownDimension::Credential,
            top,
        ),
        request_breakdown_with_scope(
            state,
            credential_id,
            api_key_id,
            only_failures,
            BreakdownDimension::ApiKey,
            top,
        ),
        request_breakdown_with_scope(
            state,
            credential_id,
            api_key_id,
            only_failures,
            BreakdownDimension::Model,
            top,
        ),
        request_breakdown_with_scope(
            state,
            credential_id,
            api_key_id,
            only_failures,
            BreakdownDimension::Path,
            top,
        ),
        request_breakdown_with_scope(
            state,
            credential_id,
            api_key_id,
            only_failures,
            BreakdownDimension::Transport,
            top,
        ),
        request_breakdown_with_scope(
            state,
            credential_id,
            api_key_id,
            only_failures,
            BreakdownDimension::StatusCode,
            top,
        ),
        request_breakdown_with_scope(
            state,
            credential_id,
            api_key_id,
            only_failures,
            BreakdownDimension::ErrorPhase,
            top,
        ),
    )?;

    let credential_model_groups =
        credential_model_groups_with_scope(state, &credentials, api_key_id, only_failures, top)
            .await?;

    Ok(UsageStatsView {
        generated_at: Utc::now(),
        filters: UsageStatsFiltersView {
            credential_id: query.credential_id.clone(),
            api_key_id: query.api_key_id.clone(),
            only_failures,
            top,
        },
        summary,
        duration,
        hourly,
        daily,
        credentials,
        credential_model_groups,
        api_keys,
        models,
        paths,
        transports,
        status_codes,
        error_phases,
    })
}

pub async fn last_request_error_for_credential(
    state: &AppState,
    credential_id: &str,
) -> Result<Option<LastRequestErrorView>, AppError> {
    latest_request_errors_with_scope(state, Some(credential_id), None, 1)
        .await
        .map(|mut items| items.pop())
}

pub async fn last_request_error_for_api_key(
    state: &AppState,
    api_key_id: &str,
) -> Result<Option<LastRequestErrorView>, AppError> {
    latest_request_errors_with_scope(state, None, Some(api_key_id), 1)
        .await
        .map(|mut items| items.pop())
}

pub async fn latest_request_errors(
    state: &AppState,
    limit: u64,
) -> Result<Vec<LastRequestErrorView>, AppError> {
    latest_request_errors_with_scope(state, None, None, limit).await
}

pub async fn list_request_records(
    state: &AppState,
    query: &ListRequestRecordsQuery,
) -> Result<Vec<RequestRecordView>, AppError> {
    let mut select = request_record::Entity::find()
        .order_by_desc(request_record::Column::RequestStartedAt)
        .order_by_desc(request_record::Column::CreatedAt);

    if let Some(credential_id) = query.credential_id.as_deref() {
        select = select.filter(request_record::Column::CredentialId.eq(credential_id.to_string()));
    }
    if let Some(api_key_id) = query.api_key_id.as_deref() {
        select = select.filter(request_record::Column::ApiKeyId.eq(api_key_id.to_string()));
    }
    if query.only_failures.unwrap_or(false) {
        select = select.filter(request_record::Column::RequestSuccess.eq(false));
    }

    let limit = query
        .limit
        .unwrap_or(DEFAULT_REQUEST_RECORD_LIMIT)
        .clamp(1, MAX_REQUEST_RECORD_LIMIT);

    let models = select
        .limit(limit)
        .all(state.db())
        .await
        .map_err(|err| AppError::internal(err.to_string()))?;

    Ok(models
        .into_iter()
        .map(RequestRecordView::from_model)
        .collect())
}

pub async fn stats_overview(state: &AppState) -> Result<StatsOverviewView, AppError> {
    let enabled_credential_count = credential::Entity::find()
        .filter(credential::Column::Enabled.eq(true))
        .count(state.db())
        .await
        .map_err(|err| AppError::internal(err.to_string()))?;

    let credential_models = credential::Entity::find()
        .all(state.db())
        .await
        .map_err(|err| AppError::internal(err.to_string()))?;
    let mut authenticated_credential_count = 0_i64;
    for credential_model in credential_models {
        if state
            .auth_manager(&credential_model.id)
            .await
            .auth()
            .await
            .is_some()
        {
            authenticated_credential_count += 1;
        }
    }

    let total_api_key_count = api_key::Entity::find()
        .count(state.db())
        .await
        .map_err(|err| AppError::internal(err.to_string()))?;
    let enabled_api_key_count = api_key::Entity::find()
        .filter(api_key::Column::Enabled.eq(true))
        .count(state.db())
        .await
        .map_err(|err| AppError::internal(err.to_string()))?;
    let pending_auth_session_count = auth_session::Entity::find()
        .filter(auth_session::Column::Status.eq(AuthStatus::Pending.as_str()))
        .count(state.db())
        .await
        .map_err(|err| AppError::internal(err.to_string()))?;

    Ok(StatsOverviewView {
        generated_at: Utc::now(),
        active_request_count: state.active_requests_total(),
        enabled_credential_count: enabled_credential_count as i64,
        authenticated_credential_count,
        total_api_key_count: total_api_key_count as i64,
        enabled_api_key_count: enabled_api_key_count as i64,
        pending_auth_session_count: pending_auth_session_count as i64,
        request_stats: request_stats_overall(state).await?,
        latest_request_errors: latest_request_errors(state, 10).await?,
    })
}

pub fn extract_requested_model_from_bytes(bytes: &[u8]) -> Option<String> {
    serde_json::from_slice::<Value>(bytes)
        .ok()
        .and_then(|value| extract_requested_model_from_value(&value))
}

pub fn extract_requested_model_from_ws_text(text: &str) -> Option<String> {
    serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|value| extract_requested_model_from_value(&value))
}

async fn finalize_request_record_impl(
    pending: PendingRequestRecord,
    finalization: RequestRecordFinalization,
) -> Result<(), AppError> {
    let existing = request_record::Entity::find_by_id(pending.request_id.clone())
        .one(pending.state.db())
        .await
        .map_err(|err| AppError::internal(err.to_string()))?
        .ok_or_else(|| AppError::not_found("request record not found"))?;

    let completed_at = Utc::now();
    let duration_ms = (completed_at - pending.started_at).num_milliseconds();
    let mut active = request_record::ActiveModel::from(existing.clone());
    let usage_json = finalization
        .usage_json
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|err| AppError::internal(err.to_string()))?;

    active.upstream_status_code = Set(finalization.upstream_status_code);
    active.request_success = Set(Some(finalization.request_success));
    active.error_phase = Set(finalization.error_phase);
    active.error_code = Set(finalization.error_code);
    active.error_message = Set(finalization.error_message);
    active.response_id = Set(finalization.response_id.or(existing.response_id));
    active.requested_model = Set(finalization.requested_model.or(existing.requested_model));
    active.input_tokens = Set(finalization
        .usage
        .as_ref()
        .map(|usage| usage.input_tokens)
        .unwrap_or(0));
    active.cached_input_tokens = Set(finalization
        .usage
        .as_ref()
        .map(|usage| usage.cached_input_tokens)
        .unwrap_or(0));
    active.output_tokens = Set(finalization
        .usage
        .as_ref()
        .map(|usage| usage.output_tokens)
        .unwrap_or(0));
    active.reasoning_output_tokens = Set(finalization
        .usage
        .as_ref()
        .map(|usage| usage.reasoning_output_tokens)
        .unwrap_or(0));
    active.total_tokens = Set(finalization
        .usage
        .as_ref()
        .map(|usage| usage.total_tokens)
        .unwrap_or(0));
    active.usage_json = Set(usage_json);
    active.request_completed_at = Set(Some(completed_at));
    active.duration_ms = Set(Some(duration_ms));
    active.updated_at = Set(completed_at);

    active
        .update(pending.state.db())
        .await
        .map_err(|err| AppError::internal(err.to_string()))?;
    Ok(())
}

async fn request_stats_with_scope(
    state: &AppState,
    credential_id: Option<&str>,
    api_key_id: Option<&str>,
    only_failures: bool,
) -> Result<RequestStatsSummaryView, AppError> {
    let row = scoped_request_record_query(credential_id, api_key_id, only_failures)
        .select_only()
        .column_as(request_record::Column::Id.count(), "total_request_count")
        .expr_as(
            conditional_count_expr(Expr::col(request_record::Column::RequestSuccess).eq(true)),
            "success_request_count",
        )
        .expr_as(
            conditional_count_expr(Expr::col(request_record::Column::RequestSuccess).eq(false)),
            "failure_request_count",
        )
        .expr_as(
            conditional_count_expr(Expr::col(request_record::Column::Transport).eq("http")),
            "http_request_count",
        )
        .expr_as(
            conditional_count_expr(Expr::col(request_record::Column::Transport).eq("websocket")),
            "websocket_request_count",
        )
        .column_as(
            request_record::Column::RequestStartedAt.min(),
            "first_request_at",
        )
        .column_as(
            request_record::Column::RequestStartedAt.max(),
            "last_request_at",
        )
        .expr_as(success_timestamp_expr(true), "last_success_at")
        .expr_as(success_timestamp_expr(false), "last_failure_at")
        .column_as(request_record::Column::InputTokens.sum(), "input_tokens")
        .column_as(
            request_record::Column::CachedInputTokens.sum(),
            "cached_input_tokens",
        )
        .column_as(request_record::Column::OutputTokens.sum(), "output_tokens")
        .column_as(
            request_record::Column::ReasoningOutputTokens.sum(),
            "reasoning_output_tokens",
        )
        .column_as(request_record::Column::TotalTokens.sum(), "total_tokens")
        .into_model::<RequestStatsAggregateRow>()
        .one(state.db())
        .await
        .map_err(|err| AppError::internal(err.to_string()))?
        .unwrap_or_default();

    Ok(RequestStatsSummaryView::from_aggregate(row))
}

async fn duration_stats_with_scope(
    state: &AppState,
    credential_id: Option<&str>,
    api_key_id: Option<&str>,
    only_failures: bool,
) -> Result<RequestDurationStatsView, AppError> {
    let row = scoped_request_record_query(credential_id, api_key_id, only_failures)
        .select_only()
        .expr_as(
            Func::avg(Expr::col(request_record::Column::DurationMs)),
            "average_duration_ms",
        )
        .column_as(request_record::Column::DurationMs.max(), "max_duration_ms")
        .into_model::<RequestDurationAggregateRow>()
        .one(state.db())
        .await
        .map_err(|err| AppError::internal(err.to_string()))?
        .unwrap_or_default();

    Ok(RequestDurationStatsView::from_aggregate(row))
}

async fn usage_time_buckets_with_scope(
    state: &AppState,
    credential_id: Option<&str>,
    api_key_id: Option<&str>,
    only_failures: bool,
    bucket: TimeBucket,
) -> Result<Vec<UsageTimeBucketView>, AppError> {
    let bucket_expr = bucket.expression();
    let rows = scoped_request_record_query(credential_id, api_key_id, only_failures)
        .select_only()
        .expr_as(bucket_expr.clone(), "bucket")
        .column_as(request_record::Column::Id.count(), "total_request_count")
        .expr_as(
            conditional_count_expr(Expr::col(request_record::Column::RequestSuccess).eq(true)),
            "success_request_count",
        )
        .expr_as(
            conditional_count_expr(Expr::col(request_record::Column::RequestSuccess).eq(false)),
            "failure_request_count",
        )
        .column_as(request_record::Column::InputTokens.sum(), "input_tokens")
        .column_as(
            request_record::Column::CachedInputTokens.sum(),
            "cached_input_tokens",
        )
        .column_as(request_record::Column::OutputTokens.sum(), "output_tokens")
        .column_as(
            request_record::Column::ReasoningOutputTokens.sum(),
            "reasoning_output_tokens",
        )
        .column_as(request_record::Column::TotalTokens.sum(), "total_tokens")
        .group_by(bucket_expr.clone())
        .order_by_asc(bucket_expr)
        .into_model::<UsageTimeBucketRow>()
        .all(state.db())
        .await
        .map_err(|err| AppError::internal(err.to_string()))?;

    Ok(rows
        .into_iter()
        .map(|mut row| {
            row.bucket = Some(bucket.normalize_label(row.bucket));
            UsageTimeBucketView::from_row(row)
        })
        .collect())
}

async fn request_breakdown_with_scope(
    state: &AppState,
    credential_id: Option<&str>,
    api_key_id: Option<&str>,
    only_failures: bool,
    dimension: BreakdownDimension,
    limit: u64,
) -> Result<Vec<RequestBreakdownView>, AppError> {
    let (group_key_expr, group_label_expr) = dimension.expressions();
    let force_failures = only_failures || dimension.forces_failure_scope();

    let rows = scoped_request_record_query(credential_id, api_key_id, force_failures)
        .select_only()
        .expr_as(group_key_expr.clone(), "group_key")
        .expr_as(group_label_expr.clone(), "group_label")
        .column_as(request_record::Column::Id.count(), "total_request_count")
        .expr_as(
            conditional_count_expr(Expr::col(request_record::Column::RequestSuccess).eq(true)),
            "success_request_count",
        )
        .expr_as(
            conditional_count_expr(Expr::col(request_record::Column::RequestSuccess).eq(false)),
            "failure_request_count",
        )
        .column_as(
            request_record::Column::RequestStartedAt.max(),
            "last_request_at",
        )
        .expr_as(
            Func::avg(Expr::col(request_record::Column::DurationMs)),
            "average_duration_ms",
        )
        .column_as(request_record::Column::DurationMs.max(), "max_duration_ms")
        .column_as(request_record::Column::InputTokens.sum(), "input_tokens")
        .column_as(
            request_record::Column::CachedInputTokens.sum(),
            "cached_input_tokens",
        )
        .column_as(request_record::Column::OutputTokens.sum(), "output_tokens")
        .column_as(
            request_record::Column::ReasoningOutputTokens.sum(),
            "reasoning_output_tokens",
        )
        .column_as(request_record::Column::TotalTokens.sum(), "total_tokens")
        .group_by(group_key_expr)
        .group_by(group_label_expr)
        .order_by_desc(Expr::col(Alias::new("total_request_count")))
        .order_by_desc(Expr::col(Alias::new("total_tokens")))
        .order_by_asc(Expr::col(Alias::new("group_label")))
        .limit(limit.max(1))
        .into_model::<RequestBreakdownRow>()
        .all(state.db())
        .await
        .map_err(|err| AppError::internal(err.to_string()))?;

    Ok(rows
        .into_iter()
        .map(RequestBreakdownView::from_row)
        .collect())
}

async fn credential_model_groups_with_scope(
    state: &AppState,
    credentials: &[RequestBreakdownView],
    api_key_id: Option<&str>,
    only_failures: bool,
    top: u64,
) -> Result<Vec<CredentialModelBreakdownView>, AppError> {
    let mut groups = Vec::with_capacity(credentials.len());

    for credential in credentials {
        let models = request_breakdown_with_scope(
            state,
            Some(credential.key.as_str()),
            api_key_id,
            only_failures,
            BreakdownDimension::Model,
            top,
        )
        .await?;

        groups.push(CredentialModelBreakdownView {
            credential: credential.clone(),
            models,
        });
    }

    Ok(groups)
}

async fn latest_request_errors_with_scope(
    state: &AppState,
    credential_id: Option<&str>,
    api_key_id: Option<&str>,
    limit: u64,
) -> Result<Vec<LastRequestErrorView>, AppError> {
    let mut select = request_record::Entity::find()
        .filter(request_record::Column::RequestSuccess.eq(false))
        .order_by_desc(request_record::Column::RequestCompletedAt)
        .order_by_desc(request_record::Column::RequestStartedAt)
        .order_by_desc(request_record::Column::CreatedAt);

    if let Some(credential_id) = credential_id {
        select = select.filter(request_record::Column::CredentialId.eq(credential_id.to_string()));
    }
    if let Some(api_key_id) = api_key_id {
        select = select.filter(request_record::Column::ApiKeyId.eq(api_key_id.to_string()));
    }

    let models = select
        .limit(limit.max(1))
        .all(state.db())
        .await
        .map_err(|err| AppError::internal(err.to_string()))?;

    Ok(models
        .into_iter()
        .map(LastRequestErrorView::from_model)
        .collect())
}

fn scoped_request_record_query(
    credential_id: Option<&str>,
    api_key_id: Option<&str>,
    only_failures: bool,
) -> Select<request_record::Entity> {
    let mut select = request_record::Entity::find();
    if let Some(credential_id) = credential_id {
        select = select.filter(request_record::Column::CredentialId.eq(credential_id.to_string()));
    }
    if let Some(api_key_id) = api_key_id {
        select = select.filter(request_record::Column::ApiKeyId.eq(api_key_id.to_string()));
    }
    if only_failures {
        select = select.filter(request_record::Column::RequestSuccess.eq(false));
    }
    select
}

fn conditional_count_expr(condition: SimpleExpr) -> SimpleExpr {
    Func::sum(Expr::case(Condition::all().add(condition), 1).finally(0)).into()
}

fn success_timestamp_expr(success: bool) -> SimpleExpr {
    Func::max(Expr::case(
        Condition::all().add(Expr::col(request_record::Column::RequestSuccess).eq(success)),
        Expr::col(request_record::Column::RequestCompletedAt)
            .if_null(Expr::col(request_record::Column::RequestStartedAt)),
    ))
    .into()
}

fn path_expression() -> SimpleExpr {
    Expr::col(request_record::Column::RequestMethod)
        .binary(BinOper::Custom("||"), Expr::val(" "))
        .binary(
            BinOper::Custom("||"),
            Expr::col(request_record::Column::RequestPath),
        )
}

fn process_sse_frame(frame: &str, observation: &mut RequestObservation) {
    let mut event_kind: Option<String> = None;
    let mut data_lines = Vec::new();

    for line in frame.lines() {
        if let Some(value) = line.strip_prefix("event:") {
            event_kind = Some(value.trim().to_string());
        } else if let Some(value) = line.strip_prefix("data:") {
            data_lines.push(value.trim_start().to_string());
        }
    }

    if data_lines.is_empty() {
        return;
    }

    let data = data_lines.join("\n");
    if data.trim() == "[DONE]" {
        return;
    }

    if let Ok(value) = serde_json::from_str::<Value>(&data) {
        observation.observe_event_value(event_kind.as_deref(), &value);
    }
}

fn response_container(value: &Value) -> &Value {
    value.get("response").unwrap_or(value)
}

fn extract_requested_model_from_value(value: &Value) -> Option<String> {
    value
        .get("model")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or_else(|| {
            value
                .get("response")
                .and_then(|response| response.get("model"))
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
        .or_else(|| {
            value
                .get("request")
                .and_then(|request| request.get("model"))
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
}

fn extract_response_id(value: &Value) -> Option<String> {
    response_container(value)
        .get("id")
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn extract_usage_json(value: &Value) -> Option<Value> {
    response_container(value)
        .get("usage")
        .filter(|usage| !usage.is_null())
        .cloned()
}

fn parse_token_usage(value: &Value) -> Option<TokenUsage> {
    let input_tokens = extract_token_count(value, &["input_tokens", "prompt_tokens"])?;
    let output_tokens = extract_token_count(value, &["output_tokens", "completion_tokens"])?;
    let cached_input_tokens = extract_nested_token_count(
        value,
        &[
            ("input_tokens_details", "cached_tokens"),
            ("prompt_tokens_details", "cached_tokens"),
        ],
    )
    .unwrap_or(0);
    let reasoning_output_tokens = extract_nested_token_count(
        value,
        &[
            ("output_tokens_details", "reasoning_tokens"),
            ("completion_tokens_details", "reasoning_tokens"),
        ],
    )
    .unwrap_or(0);
    let total_tokens = extract_token_count(value, &["total_tokens"]).unwrap_or_else(|| {
        let total = input_tokens + output_tokens + reasoning_output_tokens;
        if total > 0 {
            total
        } else {
            total + cached_input_tokens
        }
    });

    Some(TokenUsage {
        input_tokens,
        cached_input_tokens,
        output_tokens,
        reasoning_output_tokens,
        total_tokens,
    })
}

fn extract_token_count(value: &Value, field_names: &[&str]) -> Option<i64> {
    field_names
        .iter()
        .find_map(|field_name| value.get(field_name).and_then(Value::as_i64))
}

fn extract_nested_token_count(value: &Value, field_names: &[(&str, &str)]) -> Option<i64> {
    field_names.iter().find_map(|(object_name, field_name)| {
        value
            .get(*object_name)
            .and_then(|details| details.get(*field_name))
            .and_then(Value::as_i64)
    })
}

fn extract_error_code(value: &Value) -> Option<String> {
    let error = response_container(value)
        .get("error")
        .or_else(|| value.get("error"))?;

    if let Some(code) = error.get("code").and_then(Value::as_str) {
        return Some(code.to_string());
    }
    if let Some(code) = error.get("type").and_then(Value::as_str) {
        return Some(code.to_string());
    }
    error.as_str().map(ToString::to_string)
}

fn extract_error_message(value: &Value) -> Option<String> {
    let error = response_container(value)
        .get("error")
        .or_else(|| value.get("error"))?;

    if let Some(message) = error.get("message").and_then(Value::as_str) {
        return Some(message.to_string());
    }
    error.as_str().map(ToString::to_string)
}

fn extract_status_code(value: &Value) -> Option<i32> {
    value
        .get("status")
        .or_else(|| value.get("status_code"))
        .or_else(|| response_container(value).get("status_code"))
        .and_then(Value::as_i64)
        .and_then(|status| i32::try_from(status).ok())
}

fn extract_incomplete_reason(value: &Value) -> Option<String> {
    response_container(value)
        .get("incomplete_details")
        .and_then(|details| details.get("reason"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn wrapped_websocket_error_status_code_is_recorded() {
        let mut observation = RequestObservation::new(None);
        observation.observe_json_value(&json!({
            "type": "error",
            "status_code": 401,
            "error": {
                "code": "invalid_api_key",
                "message": "bad auth"
            }
        }));

        let finalization = observation.finalize();
        assert_eq!(finalization.upstream_status_code, Some(401));
        assert!(!finalization.request_success);
        assert_eq!(finalization.error_code.as_deref(), Some("invalid_api_key"));
    }

    #[test]
    fn parse_token_usage_supports_openai_usage_shape() {
        let usage = parse_token_usage(&json!({
            "prompt_tokens": 12,
            "prompt_tokens_details": {
                "cached_tokens": 4
            },
            "completion_tokens": 7,
            "completion_tokens_details": {
                "reasoning_tokens": 3
            },
            "total_tokens": 22
        }))
        .expect("usage should parse");

        assert_eq!(usage.input_tokens, 12);
        assert_eq!(usage.cached_input_tokens, 4);
        assert_eq!(usage.output_tokens, 7);
        assert_eq!(usage.reasoning_output_tokens, 3);
        assert_eq!(usage.total_tokens, 22);
    }

    #[test]
    fn request_observation_extracts_usage_from_wrapped_response() {
        let mut observation = RequestObservation::new(None);
        observation.observe_json_value(&json!({
            "type": "response.completed",
            "response": {
                "id": "resp_123",
                "usage": {
                    "input_tokens": 9,
                    "input_tokens_details": {
                        "cached_tokens": 2
                    },
                    "output_tokens": 5,
                    "output_tokens_details": {
                        "reasoning_tokens": 1
                    },
                    "total_tokens": 15
                }
            }
        }));

        let finalization = observation.finalize();
        assert_eq!(finalization.response_id.as_deref(), Some("resp_123"));
        let usage = finalization.usage.expect("usage should be recorded");
        assert_eq!(usage.input_tokens, 9);
        assert_eq!(usage.cached_input_tokens, 2);
        assert_eq!(usage.output_tokens, 5);
        assert_eq!(usage.reasoning_output_tokens, 1);
        assert_eq!(usage.total_tokens, 15);
    }

    #[test]
    fn sse_parser_finishes_last_frame_without_trailing_blank_line() {
        let mut parser = SseEventParser::default();
        let mut observation = RequestObservation::new(None);

        parser.feed(
            b"event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_456\",\"usage\":{\"input_tokens\":8,\"input_tokens_details\":{\"cached_tokens\":1},\"output_tokens\":4,\"output_tokens_details\":{\"reasoning_tokens\":2},\"total_tokens\":14}}}",
            &mut observation,
        );
        parser.finish(&mut observation);

        let finalization = observation.finalize();
        let usage = finalization.usage.expect("usage should be recorded");
        assert_eq!(finalization.response_id.as_deref(), Some("resp_456"));
        assert_eq!(usage.input_tokens, 8);
        assert_eq!(usage.cached_input_tokens, 1);
        assert_eq!(usage.output_tokens, 4);
        assert_eq!(usage.reasoning_output_tokens, 2);
        assert_eq!(usage.total_tokens, 14);
    }
}
