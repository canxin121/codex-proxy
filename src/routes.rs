use crate::auth_flow;
use crate::entities::api_key;
use crate::entities::auth_session;
use crate::entities::credential;
use crate::entities::credential_limit;
use crate::error::AppError;
use crate::models::ApiKeyView;
use crate::models::AuthMethod;
use crate::models::AuthSessionView;
use crate::models::AuthStatus;
use crate::models::CompleteBrowserAuthRequest;
use crate::models::CreateApiKeyRequest;
use crate::models::CreateApiKeyResponse;
use crate::models::CreateCredentialRequest;
use crate::models::CredentialView;
use crate::models::HealthResponse;
use crate::models::ListRequestRecordsQuery;
use crate::models::RequestRecordView;
use crate::models::StartBrowserAuthRequest;
use crate::models::StartDeviceCodeAuthRequest;
use crate::models::StatsOverviewView;
use crate::models::UpdateApiKeyRequest;
use crate::models::UpdateCredentialRequest;
use crate::request_stats::RequestObservation;
use crate::request_stats::RequestRecordFinalization;
use crate::request_stats::RequestRecordStart;
use crate::request_stats::SseEventParser;
use crate::request_stats::extract_requested_model_from_bytes;
use crate::request_stats::extract_requested_model_from_ws_text;
use crate::request_stats::last_request_error_for_api_key;
use crate::request_stats::last_request_error_for_credential;
use crate::request_stats::list_request_records as query_request_records;
use crate::request_stats::request_stats_for_api_key;
use crate::request_stats::request_stats_for_credential;
use crate::request_stats::start_request_record;
use crate::request_stats::stats_overview;
use crate::state::AppState;
use crate::state::AuthenticatedPrincipal;
use crate::state::credential_view_material;
use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::body::Bytes;
use axum::extract::Path;
use axum::extract::Query;
use axum::extract::State;
use axum::extract::ws::Message as ClientWsMessage;
use axum::extract::ws::WebSocket;
use axum::extract::ws::WebSocketUpgrade;
use axum::http::HeaderMap;
use axum::http::Method;
use axum::http::Response;
use axum::http::StatusCode;
use axum::http::header::AUTHORIZATION;
use axum::response::Html;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::routing::get_service;
use axum::routing::post;
use chrono::Utc;
use codex_api::rate_limits::parse_all_rate_limits;
use codex_api::rate_limits::parse_rate_limit_event;
use codex_login::AuthManager;
use codex_login::CodexAuth;
use codex_login::ServerOptions;
use codex_login::complete_device_code_login;
use codex_login::request_device_code;
use futures::SinkExt;
use futures::StreamExt;
use rand::Rng;
use rand::distr::Alphanumeric;
use sea_orm::ActiveModelTrait;
use sea_orm::ColumnTrait;
use sea_orm::EntityTrait;
use sea_orm::QueryFilter;
use sea_orm::QueryOrder;
use sea_orm::Set;
use serde::Deserialize;
use serde_json::Value;
use std::borrow::Cow;
use std::path::PathBuf;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message as UpstreamWsMessage;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tower_http::services::ServeDir;
use tracing::warn;
use uuid::Uuid;

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(health))
        .route("/readyz", get(health))
        .route(
            "/admin/credentials",
            get(list_credentials).post(create_credential),
        )
        .route(
            "/admin/credentials/{id}",
            get(get_credential)
                .patch(update_credential)
                .delete(delete_credential),
        )
        .route("/admin/credentials/{id}/refresh", post(refresh_credential))
        .route("/admin/auth/sessions", get(list_auth_sessions))
        .route("/admin/auth/sessions/{id}", get(get_auth_session))
        .route("/admin/auth/sessions/{id}/cancel", post(cancel_auth_session))
        .route("/admin/auth/browser", post(start_browser_auth))
        .route(
            "/admin/auth/browser/{id}/complete",
            post(complete_browser_auth),
        )
        .route("/admin/auth/device-code", post(start_device_code_auth))
        .route("/admin/api-keys", get(list_api_keys).post(create_api_key))
        .route(
            "/admin/api-keys/{id}",
            get(get_api_key)
                .patch(update_api_key)
                .delete(delete_api_key),
        )
        .route("/admin/stats/overview", get(get_stats_overview))
        .route("/admin/stats/requests", get(list_request_records_admin))
        .route(
            "/responses",
            post(proxy_responses_http).get(proxy_responses_ws),
        )
        .route(
            "/v1/responses",
            post(proxy_responses_http).get(proxy_responses_ws),
        )
        .route("/responses/compact", post(proxy_compact_http))
        .route("/v1/responses/compact", post(proxy_compact_http))
        .route("/models", get(proxy_models_http))
        .route("/v1/models", get(proxy_models_http))
        .nest_service("/assets", get_service(ServeDir::new(ui_dist_dir().join("assets"))))
        .route("/", get(serve_ui_root))
        .route("/{*path}", get(serve_ui_spa))
        .with_state(state)
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn list_credentials(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<CredentialView>>, AppError> {
    require_admin(&state, &headers).await?;
    let models = credential::Entity::find()
        .order_by_asc(credential::Column::CreatedAt)
        .all(state.db())
        .await
        .map_err(|err| AppError::internal(err.to_string()))?;

    let mut items = Vec::with_capacity(models.len());
    for model in models {
        items.push(credential_to_view(&state, model).await?);
    }

    Ok(Json(items))
}

async fn get_credential(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<CredentialView>, AppError> {
    require_admin(&state, &headers).await?;
    let model = find_credential(&state, &id).await?;
    Ok(Json(credential_to_view(&state, model).await?))
}

async fn create_credential(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateCredentialRequest>,
) -> Result<(StatusCode, Json<CredentialView>), AppError> {
    require_admin(&state, &headers).await?;
    validate_create_credential(&payload)?;

    let now = Utc::now();
    let model = credential::ActiveModel {
        id: Set(Uuid::new_v4().to_string()),
        name: Set(payload.name.clone()),
        kind: Set(crate::models::CredentialKind::ChatgptAuth
            .as_str()
            .to_string()),
        enabled: Set(payload.enabled.unwrap_or(true)),
        selection_weight: Set(payload.selection_weight.unwrap_or(1).max(1)),
        notes: Set(payload.notes.clone()),
        upstream_base_url: Set(payload.upstream_base_url.clone()),
        account_id: Set(None),
        account_email: Set(None),
        plan_type: Set(None),
        last_used_at: Set(None),
        last_limit_sync_at: Set(None),
        last_refresh_at: Set(None),
        last_error: Set(None),
        failure_count: Set(0),
        created_at: Set(now),
        updated_at: Set(now),
    };
    let inserted = model
        .insert(state.db())
        .await
        .map_err(|err| AppError::internal(err.to_string()))?;

    Ok((
        StatusCode::CREATED,
        Json(credential_to_view(&state, inserted).await?),
    ))
}

async fn update_credential(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(payload): Json<UpdateCredentialRequest>,
) -> Result<Json<CredentialView>, AppError> {
    require_admin(&state, &headers).await?;
    let existing = find_credential(&state, &id).await?;

    let mut active = credential::ActiveModel::from(existing);
    if let Some(name) = payload.name {
        active.name = Set(name);
    }
    if let Some(enabled) = payload.enabled {
        active.enabled = Set(enabled);
    }
    if let Some(weight) = payload.selection_weight {
        active.selection_weight = Set(weight.max(1));
    }
    if let Some(notes) = payload.notes {
        active.notes = Set(Some(notes));
    }
    if let Some(upstream_base_url) = payload.upstream_base_url {
        active.upstream_base_url = Set(Some(upstream_base_url));
    }
    active.updated_at = Set(Utc::now());

    let updated = active
        .update(state.db())
        .await
        .map_err(|err| AppError::internal(err.to_string()))?;
    let synced = state
        .sync_credential_from_auth(&updated.id)
        .await
        .unwrap_or(updated);
    Ok(Json(credential_to_view(&state, synced).await?))
}

async fn delete_credential(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, AppError> {
    require_admin(&state, &headers).await?;
    find_credential(&state, &id).await?;
    cancel_pending_auth_sessions_for_credential(&state, &id).await?;

    auth_session::Entity::delete_many()
        .filter(auth_session::Column::CredentialId.eq(id.clone()))
        .exec(state.db())
        .await
        .map_err(|err| AppError::internal(err.to_string()))?;

    credential_limit::Entity::delete_many()
        .filter(credential_limit::Column::CredentialId.eq(id.clone()))
        .exec(state.db())
        .await
        .map_err(|err| AppError::internal(err.to_string()))?;
    credential::Entity::delete_by_id(id.clone())
        .exec(state.db())
        .await
        .map_err(|err| AppError::internal(err.to_string()))?;

    state.invalidate_auth_manager(&id);
    let home = state.credential_home(&id);
    if home.exists() {
        std::fs::remove_dir_all(home)?;
    }

    Ok(StatusCode::NO_CONTENT)
}

async fn refresh_credential(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<CredentialView>, AppError> {
    require_admin(&state, &headers).await?;
    let model = find_credential(&state, &id).await?;
    let manager = state.auth_manager(&model.id).await;
    manager
        .refresh_token_from_authority()
        .await
        .map_err(|err| AppError::bad_gateway(err.to_string()))?;
    let synced = state.sync_credential_from_auth(&model.id).await?;
    Ok(Json(credential_to_view(&state, synced).await?))
}

async fn list_auth_sessions(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<AuthSessionView>>, AppError> {
    require_admin(&state, &headers).await?;
    let models = auth_session::Entity::find()
        .order_by_desc(auth_session::Column::CreatedAt)
        .all(state.db())
        .await
        .map_err(|err| AppError::internal(err.to_string()))?;
    let items = models
        .into_iter()
        .map(auth_session_to_view)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(items))
}

async fn get_auth_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<AuthSessionView>, AppError> {
    require_admin(&state, &headers).await?;
    let model = find_auth_session(&state, &id).await?;
    Ok(Json(auth_session_to_view(model)?))
}

async fn start_browser_auth(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<StartBrowserAuthRequest>,
) -> Result<(StatusCode, Json<AuthSessionView>), AppError> {
    require_admin(&state, &headers).await?;
    find_credential(&state, &payload.credential_id).await?;
    cancel_pending_auth_sessions_for_credential(&state, &payload.credential_id).await?;

    let auth_start = auth_flow::start_browser_auth(state.config());
    let now = Utc::now();
    let model = auth_session::ActiveModel {
        id: Set(Uuid::new_v4().to_string()),
        credential_id: Set(payload.credential_id),
        method: Set(AuthMethod::Browser.as_str().to_string()),
        status: Set(AuthStatus::Pending.as_str().to_string()),
        authorization_url: Set(Some(auth_start.authorization_url)),
        redirect_uri: Set(Some(auth_start.redirect_uri)),
        oauth_state: Set(Some(auth_start.oauth_state)),
        pkce_code_verifier: Set(Some(auth_start.pkce_code_verifier)),
        verification_url: Set(None),
        user_code: Set(None),
        device_auth_id: Set(None),
        device_code_interval_seconds: Set(None),
        error_message: Set(None),
        completed_at: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    };
    let inserted = model
        .insert(state.db())
        .await
        .map_err(|err| AppError::internal(err.to_string()))?;
    Ok((StatusCode::CREATED, Json(auth_session_to_view(inserted)?)))
}

async fn complete_browser_auth(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(payload): Json<CompleteBrowserAuthRequest>,
) -> Result<Json<AuthSessionView>, AppError> {
    require_admin(&state, &headers).await?;
    let session = find_auth_session(&state, &id).await?;
    ensure_auth_session_state(&session, AuthMethod::Browser, AuthStatus::Pending)?;

    let oauth_state = session
        .oauth_state
        .as_deref()
        .ok_or_else(|| AppError::internal("browser auth session is missing oauth_state"))?;
    let redirect_uri = session
        .redirect_uri
        .clone()
        .ok_or_else(|| AppError::internal("browser auth session is missing redirect_uri"))?;
    let pkce_code_verifier = session
        .pkce_code_verifier
        .clone()
        .ok_or_else(|| AppError::internal("browser auth session is missing pkce_code_verifier"))?;

    let completion = match auth_flow::parse_browser_callback(&payload.callback_url, oauth_state) {
        Ok(completion) => completion,
        Err(err) => {
            mark_auth_session_failed(&state, &session.id, err.to_string()).await?;
            return Err(err);
        }
    };
    if let Err(err) = auth_flow::complete_browser_auth(
        state.config(),
        &state.credential_home(&session.credential_id),
        &redirect_uri,
        &pkce_code_verifier,
        &completion.oauth_code,
    )
    .await
    {
        mark_auth_session_failed(&state, &session.id, err.to_string()).await?;
        return Err(err);
    }

    state.invalidate_auth_manager(&session.credential_id);
    let _ = state
        .sync_credential_from_auth(&session.credential_id)
        .await?;
    let updated = set_auth_session_status(&state, &session.id, AuthStatus::Completed, None).await?;
    Ok(Json(auth_session_to_view(updated)?))
}

async fn start_device_code_auth(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<StartDeviceCodeAuthRequest>,
) -> Result<(StatusCode, Json<AuthSessionView>), AppError> {
    require_admin(&state, &headers).await?;
    let credential = find_credential(&state, &payload.credential_id).await?;
    cancel_pending_auth_sessions_for_credential(&state, &credential.id).await?;

    let opts = auth_server_options(&state, &credential.id);
    let device_code = request_device_code(&opts)
        .await
        .map_err(|err| AppError::bad_gateway(err.to_string()))?;

    let now = Utc::now();
    let model = auth_session::ActiveModel {
        id: Set(Uuid::new_v4().to_string()),
        credential_id: Set(credential.id.clone()),
        method: Set(AuthMethod::DeviceCode.as_str().to_string()),
        status: Set(AuthStatus::Pending.as_str().to_string()),
        authorization_url: Set(None),
        redirect_uri: Set(None),
        oauth_state: Set(None),
        pkce_code_verifier: Set(None),
        verification_url: Set(Some(device_code.verification_url.clone())),
        user_code: Set(Some(device_code.user_code.clone())),
        device_auth_id: Set(None),
        device_code_interval_seconds: Set(None),
        error_message: Set(None),
        completed_at: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    };
    let inserted = model
        .insert(state.db())
        .await
        .map_err(|err| AppError::internal(err.to_string()))?;

    spawn_device_code_auth_task(
        state.clone(),
        inserted.id.clone(),
        inserted.credential_id.clone(),
        opts,
        device_code,
    );

    Ok((StatusCode::CREATED, Json(auth_session_to_view(inserted)?)))
}

async fn cancel_auth_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<AuthSessionView>, AppError> {
    require_admin(&state, &headers).await?;
    let session = find_auth_session(&state, &id).await?;
    if auth_session_status(&session)? != AuthStatus::Pending {
        return Ok(Json(auth_session_to_view(session)?));
    }

    if let Some(cancellation) = state.take_auth_cancellation(&session.id) {
        cancellation.cancel();
    }
    let updated = set_auth_session_status(&state, &session.id, AuthStatus::Cancelled, None).await?;
    Ok(Json(auth_session_to_view(updated)?))
}

async fn list_api_keys(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<ApiKeyView>>, AppError> {
    require_admin(&state, &headers).await?;
    let models = api_key::Entity::find()
        .order_by_asc(api_key::Column::CreatedAt)
        .all(state.db())
        .await
        .map_err(|err| AppError::internal(err.to_string()))?;
    let mut items = Vec::with_capacity(models.len());
    for model in models {
        items.push(api_key_to_view(&state, model).await?);
    }
    Ok(Json(items))
}

async fn get_api_key(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<ApiKeyView>, AppError> {
    require_admin(&state, &headers).await?;
    let model = find_api_key(&state, &id).await?;
    Ok(Json(api_key_to_view(&state, model).await?))
}

async fn create_api_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateApiKeyRequest>,
) -> Result<(StatusCode, Json<CreateApiKeyResponse>), AppError> {
    require_admin(&state, &headers).await?;
    let plain_text = generate_proxy_api_key();
    let now = Utc::now();
    let model = api_key::ActiveModel {
        id: Set(Uuid::new_v4().to_string()),
        name: Set(payload.name),
        key_hash: Set(crate::state::hash_api_key(&plain_text)),
        enabled: Set(true),
        is_admin: Set(payload.is_admin.unwrap_or(false)),
        expires_at: Set(payload.expires_at),
        last_used_at: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    };
    let inserted = model
        .insert(state.db())
        .await
        .map_err(|err| AppError::internal(err.to_string()))?;

    Ok((
        StatusCode::CREATED,
        Json(CreateApiKeyResponse {
            api_key_value: plain_text,
            api_key_record: api_key_to_view(&state, inserted).await?,
        }),
    ))
}

async fn update_api_key(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(payload): Json<UpdateApiKeyRequest>,
) -> Result<Json<ApiKeyView>, AppError> {
    require_admin(&state, &headers).await?;
    let mut active = api_key::ActiveModel::from(find_api_key(&state, &id).await?);
    if let Some(name) = payload.name {
        active.name = Set(name);
    }
    if let Some(enabled) = payload.enabled {
        active.enabled = Set(enabled);
    }
    if let Some(expires_at) = payload.expires_at {
        active.expires_at = Set(Some(expires_at));
    }
    active.updated_at = Set(Utc::now());

    let updated = active
        .update(state.db())
        .await
        .map_err(|err| AppError::internal(err.to_string()))?;
    Ok(Json(api_key_to_view(&state, updated).await?))
}

async fn get_stats_overview(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<StatsOverviewView>, AppError> {
    require_admin(&state, &headers).await?;
    Ok(Json(stats_overview(&state).await?))
}

async fn list_request_records_admin(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListRequestRecordsQuery>,
) -> Result<Json<Vec<RequestRecordView>>, AppError> {
    require_admin(&state, &headers).await?;
    Ok(Json(query_request_records(&state, &query).await?))
}

async fn delete_api_key(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, AppError> {
    require_admin(&state, &headers).await?;
    find_api_key(&state, &id).await?;
    api_key::Entity::delete_by_id(id)
        .exec(state.db())
        .await
        .map_err(|err| AppError::internal(err.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn proxy_responses_http(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response<Body>, AppError> {
    proxy_http(&state, &method, "/responses", headers, body).await
}

async fn proxy_compact_http(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response<Body>, AppError> {
    proxy_http(&state, &method, "/responses/compact", headers, body).await
}

async fn proxy_models_http(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response<Body>, AppError> {
    proxy_http(&state, &method, "/models", headers, Bytes::new()).await
}

async fn proxy_http(
    state: &AppState,
    method: &Method,
    normalized_path: &'static str,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response<Body>, AppError> {
    let principal = require_client(state, &headers).await?;
    let preferred_credential = preferred_credential_from_headers(&headers);
    let selected = state
        .select_credential(preferred_credential.as_deref())
        .await?;
    let requested_model = extract_requested_model_from_bytes(&body);
    let mut request_record = start_request_record(
        state,
        RequestRecordStart {
            principal,
            credential: selected.model.clone(),
            transport: "http",
            method: method.to_string(),
            path: normalized_path.to_string(),
            requested_model: requested_model.clone(),
        },
    )
    .await?;
    let manager = state.auth_manager(&selected.model.id).await;
    let lease = state.acquire_request_lease(selected.model.id.clone());
    let response = match send_http_with_recovery(
        state,
        &selected.model,
        &manager,
        method,
        normalized_path,
        &headers,
        body,
    )
    .await
    {
        Ok(response) => response,
        Err(err) => {
            let mut observation = RequestObservation::new(requested_model);
            observation.mark_failure("upstream_request", None, err.to_string(), None);
            let finalization = observation.finalize();
            request_record.finalize(finalization.clone()).await?;
            sync_credential_transient_state(state, &selected.model.id, &finalization).await?;
            return Err(err);
        }
    };

    state.record_credential_touch(&selected.model.id).await?;
    state
        .update_rate_limits_from_headers(&selected.model.id, response.headers())
        .await?;
    let status = response.status();
    let response_headers = response.headers().clone();

    let is_event_stream = response_headers
        .get(http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.contains("text/event-stream"))
        .unwrap_or(false);

    if is_event_stream {
        let state = state.clone();
        let credential_id = selected.model.id.clone();
        let body_stream = async_stream::stream! {
            let _lease = lease;
            let mut stream = response.bytes_stream();
            let mut parser = SseEventParser::default();
            let mut observation = RequestObservation::new(requested_model);
            let mut request_record = request_record;
            let mut finalized = false;

            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(bytes) => {
                        parser.feed(&bytes, &mut observation);
                        yield Result::<Bytes, std::io::Error>::Ok(bytes);
                    }
                    Err(err) => {
                        observation.mark_failure_if_missing(
                            "upstream_stream",
                            Some("stream_read_error".to_string()),
                            err.to_string(),
                            Some(i32::from(status.as_u16())),
                        );
                        let finalization = std::mem::take(&mut observation).finalize();
                        let _ = request_record.finalize(finalization.clone()).await;
                        let _ = sync_credential_transient_state(&state, &credential_id, &finalization).await;
                        finalized = true;
                        yield Result::<Bytes, std::io::Error>::Err(std::io::Error::other(err));
                        break;
                    }
                }
            }

            if !finalized {
                let finalization = observation.finish_sse_response(status);
                let _ = request_record.finalize(finalization.clone()).await;
                let _ = sync_credential_transient_state(&state, &credential_id, &finalization).await;
            }
        };

        return build_client_response(status, &response_headers, Body::from_stream(body_stream));
    }

    let _lease = lease;
    let body_bytes = match response.bytes().await {
        Ok(body_bytes) => body_bytes,
        Err(err) => {
            let mut observation = RequestObservation::new(requested_model);
            observation.mark_failure(
                "upstream_body",
                Some("body_read_error".to_string()),
                err.to_string(),
                Some(i32::from(status.as_u16())),
            );
            let finalization = observation.finalize();
            request_record.finalize(finalization.clone()).await?;
            sync_credential_transient_state(state, &selected.model.id, &finalization).await?;
            return Err(AppError::bad_gateway(err.to_string()));
        }
    };

    let mut observation = RequestObservation::new(requested_model);
    if let Ok(value) = serde_json::from_slice::<Value>(&body_bytes) {
        observation.observe_json_value(&value);
    }
    let finalization = observation.finish_http_response(status);
    request_record.finalize(finalization.clone()).await?;
    sync_credential_transient_state(state, &selected.model.id, &finalization).await?;

    build_client_response(status, &response_headers, Body::from(body_bytes))
}

async fn proxy_responses_ws(
    State(state): State<AppState>,
    ws: WebSocketUpgrade,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    let principal = require_client(&state, &headers).await?;
    let preferred_credential = preferred_credential_from_headers(&headers);
    let selected = state
        .select_credential(preferred_credential.as_deref())
        .await?;
    let client_headers = headers.clone();
    let credential_id = selected.model.id.clone();
    let manager = state.auth_manager(&credential_id).await;

    Ok(ws.on_upgrade(move |socket| async move {
        if let Err(err) = run_ws_proxy(
            socket,
            state,
            principal,
            selected.model,
            manager,
            client_headers,
        )
        .await
        {
            warn!(error = %err, credential_id = %credential_id, "websocket proxy ended with error");
        }
    }))
}

async fn run_ws_proxy(
    mut client_socket: WebSocket,
    state: AppState,
    principal: AuthenticatedPrincipal,
    credential: credential::Model,
    manager: std::sync::Arc<AuthManager>,
    client_headers: HeaderMap,
) -> Result<(), AppError> {
    let _lease = state.acquire_request_lease(credential.id.clone());
    let mut request_record = start_request_record(
        &state,
        RequestRecordStart {
            principal,
            credential: credential.clone(),
            transport: "websocket",
            method: Method::GET.to_string(),
            path: "/responses".to_string(),
            requested_model: None,
        },
    )
    .await?;
    let mut observation = RequestObservation::new(None);

    state.record_credential_touch(&credential.id).await?;

    let (mut upstream, response_headers) =
        match connect_ws_with_recovery(&state, &credential, &manager, &client_headers).await {
            Ok(value) => value,
            Err(err) => {
                observation.mark_failure("upstream_connect", None, err.to_string(), None);
                let finalization = observation.finalize();
                request_record.finalize(finalization.clone()).await?;
                sync_credential_transient_state(&state, &credential.id, &finalization).await?;
                return Err(err);
            }
        };
    state
        .update_rate_limits_from_headers(&credential.id, &response_headers)
        .await?;

    let mut terminal_error: Option<AppError> = None;

    loop {
        tokio::select! {
            maybe_client_message = client_socket.recv() => {
                let Some(client_message) = maybe_client_message else {
                    observation.mark_failure_if_missing(
                        "client_websocket",
                        Some("client_closed".to_string()),
                        "client websocket closed before response.completed",
                        None,
                    );
                    let _ = upstream.send(UpstreamWsMessage::Close(None)).await;
                    break;
                };
                match client_message {
                    Ok(message) => {
                        if let ClientWsMessage::Text(text) = &message {
                            observation.set_requested_model_if_missing(
                                extract_requested_model_from_ws_text(text.as_str()),
                            );
                        }
                        if let Some(upstream_message) = map_client_ws_message(message) {
                            if let Err(err) = upstream.send(upstream_message).await {
                                observation.mark_failure_if_missing(
                                    "upstream_websocket_send",
                                    Some("upstream_send_failed".to_string()),
                                    err.to_string(),
                                    None,
                                );
                                terminal_error = Some(AppError::bad_gateway(err.to_string()));
                                break;
                            }
                        } else {
                            observation.mark_failure_if_missing(
                                "client_websocket",
                                Some("client_closed".to_string()),
                                "client websocket closed before response.completed",
                                None,
                            );
                            break;
                        }
                    }
                    Err(err) => {
                        observation.mark_failure_if_missing(
                            "client_websocket",
                            Some("client_receive_error".to_string()),
                            format!("client websocket error: {err}"),
                            None,
                        );
                        break;
                    }
                }
            }
            maybe_upstream_message = upstream.next() => {
                let Some(upstream_message) = maybe_upstream_message else {
                    observation.mark_failure_if_missing(
                        "upstream_websocket",
                        Some("upstream_closed".to_string()),
                        "websocket closed before response.completed",
                        None,
                    );
                    let _ = client_socket.send(ClientWsMessage::Close(None)).await;
                    break;
                };
                match upstream_message {
                    Ok(message) => {
                        let client_message = match map_upstream_ws_message(
                            &state,
                            &credential.id,
                            &mut observation,
                            message,
                        )
                        .await
                        {
                            Ok(message) => message,
                            Err(err) => {
                                observation.mark_failure_if_missing(
                                    "upstream_websocket",
                                    Some("upstream_event_processing_failed".to_string()),
                                    err.to_string(),
                                    None,
                                );
                                terminal_error = Some(err);
                                break;
                            }
                        };
                        if let Some(client_message) = client_message {
                            if let Err(err) = client_socket.send(client_message).await {
                                observation.mark_failure_if_missing(
                                    "client_websocket_send",
                                    Some("client_send_failed".to_string()),
                                    err.to_string(),
                                    None,
                                );
                                break;
                            }
                        } else {
                            observation.mark_failure_if_missing(
                                "upstream_websocket",
                                Some("upstream_closed".to_string()),
                                "websocket closed before response.completed",
                                None,
                            );
                            break;
                        }
                    }
                    Err(err) => {
                        observation.mark_failure_if_missing(
                            "upstream_websocket",
                            Some("upstream_receive_error".to_string()),
                            format!("upstream websocket error: {err}"),
                            None,
                        );
                        terminal_error = Some(AppError::bad_gateway(err.to_string()));
                        let _ = client_socket.send(ClientWsMessage::Close(None)).await;
                        break;
                    }
                }
            }
        }
    }

    let finalization = observation.finish_websocket();
    request_record.finalize(finalization.clone()).await?;
    sync_credential_transient_state(&state, &credential.id, &finalization).await?;

    if let Some(err) = terminal_error {
        return Err(err);
    }

    Ok(())
}

async fn connect_ws_with_recovery(
    state: &AppState,
    credential: &credential::Model,
    manager: &std::sync::Arc<AuthManager>,
    client_headers: &HeaderMap,
) -> Result<
    (
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        HeaderMap,
    ),
    AppError,
> {
    let mut recovery = Some(manager.unauthorized_recovery());
    loop {
        let auth = manager
            .auth()
            .await
            .ok_or_else(|| AppError::service_unavailable("credential auth is unavailable"))?;
        let request = build_upstream_ws_request(state, credential, client_headers, &auth)?;
        match connect_async(request).await {
            Ok((stream, response)) => return Ok((stream, response.headers().clone())),
            Err(err) if is_ws_unauthorized(&err) => {
                if let Some(recovery) = recovery.as_mut()
                    && recovery.has_next()
                {
                    recovery
                        .next()
                        .await
                        .map_err(|refresh_err| AppError::bad_gateway(refresh_err.to_string()))?;
                    state.sync_credential_from_auth(&credential.id).await?;
                    continue;
                }
                return Err(AppError::bad_gateway("upstream websocket rejected auth"));
            }
            Err(err) => return Err(AppError::bad_gateway(err.to_string())),
        }
    }
}

async fn map_upstream_ws_message(
    state: &AppState,
    credential_id: &str,
    observation: &mut RequestObservation,
    message: UpstreamWsMessage,
) -> Result<Option<ClientWsMessage>, AppError> {
    match message {
        UpstreamWsMessage::Text(text) => {
            sync_rate_limits_from_ws_text(state, credential_id, text.as_str()).await?;
            if let Ok(value) = serde_json::from_str::<Value>(text.as_str()) {
                observation.observe_json_value(&value);
            }
            Ok(Some(ClientWsMessage::Text(text.to_string().into())))
        }
        UpstreamWsMessage::Binary(data) => Ok(Some(ClientWsMessage::Binary(data))),
        UpstreamWsMessage::Ping(data) => Ok(Some(ClientWsMessage::Ping(data))),
        UpstreamWsMessage::Pong(data) => Ok(Some(ClientWsMessage::Pong(data))),
        UpstreamWsMessage::Close(_) => Ok(None),
        UpstreamWsMessage::Frame(_) => Ok(None),
    }
}

fn map_client_ws_message(message: ClientWsMessage) -> Option<UpstreamWsMessage> {
    match message {
        ClientWsMessage::Text(text) => Some(UpstreamWsMessage::Text(text.to_string().into())),
        ClientWsMessage::Binary(data) => Some(UpstreamWsMessage::Binary(data)),
        ClientWsMessage::Ping(data) => Some(UpstreamWsMessage::Ping(data)),
        ClientWsMessage::Pong(data) => Some(UpstreamWsMessage::Pong(data)),
        ClientWsMessage::Close(_) => None,
    }
}

async fn sync_rate_limits_from_ws_text(
    state: &AppState,
    credential_id: &str,
    text: &str,
) -> Result<(), AppError> {
    if let Some(snapshot) = parse_rate_limit_event(text) {
        state
            .update_rate_limits_from_headers(
                credential_id,
                &header_map_from_snapshots(std::slice::from_ref(&snapshot)),
            )
            .await?;
    }

    let json = match serde_json::from_str::<WsEnvelope>(text) {
        Ok(json) => json,
        Err(_) => return Ok(()),
    };
    if let Some(headers) = json.headers {
        let header_map = header_map_from_json(headers)?;
        let snapshots = parse_all_rate_limits(&header_map);
        if !snapshots.is_empty() {
            state
                .update_rate_limits_from_headers(credential_id, &header_map)
                .await?;
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct WsEnvelope {
    headers: Option<Value>,
}

fn header_map_from_json(value: Value) -> Result<HeaderMap, AppError> {
    let object = value
        .as_object()
        .ok_or_else(|| AppError::bad_request("websocket headers field must be an object"))?;
    let mut headers = HeaderMap::new();
    for (name, value) in object {
        let Some(text) = json_header_value(value) else {
            continue;
        };
        let header_name = http::header::HeaderName::try_from(name.as_str())
            .map_err(|err| AppError::bad_request(err.to_string()))?;
        let header_value = http::HeaderValue::from_str(text.as_ref())
            .map_err(|err| AppError::bad_request(err.to_string()))?;
        let _ = headers.insert(header_name, header_value);
    }
    Ok(headers)
}

fn json_header_value(value: &Value) -> Option<Cow<'_, str>> {
    if let Some(text) = value.as_str() {
        return Some(Cow::Borrowed(text));
    }
    if let Some(number) = value.as_i64() {
        return Some(Cow::Owned(number.to_string()));
    }
    if let Some(number) = value.as_u64() {
        return Some(Cow::Owned(number.to_string()));
    }
    if let Some(number) = value.as_f64() {
        return Some(Cow::Owned(number.to_string()));
    }
    value
        .as_bool()
        .map(|boolean| Cow::Owned(boolean.to_string()))
}

fn header_map_from_snapshots(
    snapshots: &[codex_protocol::protocol::RateLimitSnapshot],
) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for snapshot in snapshots {
        let limit_id = snapshot
            .limit_id
            .clone()
            .unwrap_or_else(|| "codex".to_string())
            .replace('_', "-");
        if let Some(window) = snapshot.primary.as_ref() {
            insert_header_value(
                &mut headers,
                format!("x-{limit_id}-primary-used-percent"),
                window.used_percent.to_string(),
            );
            if let Some(window_minutes) = window.window_minutes {
                insert_header_value(
                    &mut headers,
                    format!("x-{limit_id}-primary-window-minutes"),
                    window_minutes.to_string(),
                );
            }
            if let Some(reset_at) = window.resets_at {
                insert_header_value(
                    &mut headers,
                    format!("x-{limit_id}-primary-reset-at"),
                    reset_at.to_string(),
                );
            }
        }
        if let Some(window) = snapshot.secondary.as_ref() {
            insert_header_value(
                &mut headers,
                format!("x-{limit_id}-secondary-used-percent"),
                window.used_percent.to_string(),
            );
            if let Some(window_minutes) = window.window_minutes {
                insert_header_value(
                    &mut headers,
                    format!("x-{limit_id}-secondary-window-minutes"),
                    window_minutes.to_string(),
                );
            }
            if let Some(reset_at) = window.resets_at {
                insert_header_value(
                    &mut headers,
                    format!("x-{limit_id}-secondary-reset-at"),
                    reset_at.to_string(),
                );
            }
        }
    }
    headers
}

fn insert_header_value(headers: &mut HeaderMap, name: String, value: String) {
    if let (Ok(name), Ok(value)) = (
        http::header::HeaderName::try_from(name),
        http::HeaderValue::from_str(&value),
    ) {
        let _ = headers.insert(name, value);
    }
}

async fn send_http_with_recovery(
    state: &AppState,
    credential: &credential::Model,
    manager: &std::sync::Arc<AuthManager>,
    method: &Method,
    normalized_path: &str,
    client_headers: &HeaderMap,
    body: Bytes,
) -> Result<reqwest::Response, AppError> {
    let mut recovery = Some(manager.unauthorized_recovery());
    loop {
        let auth = manager
            .auth()
            .await
            .ok_or_else(|| AppError::service_unavailable("credential auth is unavailable"))?;
        let url = upstream_http_url(state, credential, normalized_path);
        let headers = build_upstream_http_headers(client_headers, &auth)?;
        let mut request = state
            .http_client()
            .request(
                reqwest::Method::from_bytes(method.as_str().as_bytes())
                    .map_err(|err| AppError::bad_request(err.to_string()))?,
                url,
            )
            .headers(headers);
        if !body.is_empty() {
            request = request.body(body.clone());
        }
        let response = request
            .send()
            .await
            .map_err(|err| AppError::bad_gateway(err.to_string()))?;
        if response.status() != StatusCode::UNAUTHORIZED {
            return Ok(response);
        }
        if let Some(recovery) = recovery.as_mut()
            && recovery.has_next()
        {
            recovery
                .next()
                .await
                .map_err(|err| AppError::bad_gateway(err.to_string()))?;
            state.sync_credential_from_auth(&credential.id).await?;
            continue;
        }
        return Ok(response);
    }
}

fn upstream_http_url(
    state: &AppState,
    credential: &credential::Model,
    normalized_path: &str,
) -> String {
    format!(
        "{}/{}",
        state.provider_base_url(credential).trim_end_matches('/'),
        normalized_path.trim_start_matches('/'),
    )
}

fn build_upstream_http_headers(
    client_headers: &HeaderMap,
    auth: &CodexAuth,
) -> Result<reqwest::header::HeaderMap, AppError> {
    let mut headers = reqwest::header::HeaderMap::new();
    for (name, value) in client_headers {
        if should_skip_request_header(name.as_str()) {
            continue;
        }
        let _ = headers.insert(name, value.clone());
    }

    let bearer = auth
        .get_token()
        .map_err(|err| AppError::service_unavailable(err.to_string()))?;
    let header = reqwest::header::HeaderValue::from_str(&format!("Bearer {bearer}"))
        .map_err(|err| AppError::bad_request(err.to_string()))?;
    let _ = headers.insert(reqwest::header::AUTHORIZATION, header);
    if let Some(account_id) = auth.get_account_id() {
        let header = reqwest::header::HeaderValue::from_str(account_id.as_str())
            .map_err(|err| AppError::bad_request(err.to_string()))?;
        let _ = headers.insert("ChatGPT-Account-ID", header);
    }
    Ok(headers)
}

fn build_upstream_ws_request(
    state: &AppState,
    credential: &credential::Model,
    client_headers: &HeaderMap,
    auth: &CodexAuth,
) -> Result<http::Request<()>, AppError> {
    let url = upstream_http_url(state, credential, "/responses")
        .replace("https://", "wss://")
        .replace("http://", "ws://");
    let mut request = url
        .into_client_request()
        .map_err(|err| AppError::bad_request(err.to_string()))?;
    {
        let headers = request.headers_mut();
        for (name, value) in client_headers {
            if should_skip_ws_request_header(name.as_str()) {
                continue;
            }
            let _ = headers.insert(name, value.clone());
        }
        let bearer = auth
            .get_token()
            .map_err(|err| AppError::service_unavailable(err.to_string()))?;
        let header = http::HeaderValue::from_str(&format!("Bearer {bearer}"))
            .map_err(|err| AppError::bad_request(err.to_string()))?;
        let _ = headers.insert(AUTHORIZATION, header);
        if let Some(account_id) = auth.get_account_id() {
            let header = http::HeaderValue::from_str(account_id.as_str())
                .map_err(|err| AppError::bad_request(err.to_string()))?;
            let _ = headers.insert("ChatGPT-Account-ID", header);
        }
    }
    Ok(request)
}

fn is_ws_unauthorized(err: &tokio_tungstenite::tungstenite::Error) -> bool {
    match err {
        tokio_tungstenite::tungstenite::Error::Http(response) => {
            response.status() == StatusCode::UNAUTHORIZED
        }
        _ => false,
    }
}

fn should_skip_request_header(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    matches!(
        name.as_str(),
        "authorization"
            | "connection"
            | "content-length"
            | "host"
            | "upgrade"
            | "proxy-connection"
            | "accept-encoding"
    )
}

fn should_skip_ws_request_header(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    should_skip_request_header(name.as_str()) || name.starts_with("sec-websocket-")
}

fn should_skip_response_header(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    matches!(
        name.as_str(),
        "connection"
            | "content-length"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

async fn require_admin(state: &AppState, headers: &HeaderMap) -> Result<(), AppError> {
    let bearer = extract_bearer(headers)?;
    state.authenticate_api_key(&bearer, true).await?;
    Ok(())
}

async fn require_client(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<AuthenticatedPrincipal, AppError> {
    let bearer = extract_bearer(headers)?;
    state.authenticate_api_key(&bearer, false).await
}

fn preferred_credential_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-codex-proxy-credential-id")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn extract_bearer(headers: &HeaderMap) -> Result<String, AppError> {
    let raw = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| AppError::unauthorized("missing bearer token"))?;
    let token = raw
        .strip_prefix("Bearer ")
        .or_else(|| raw.strip_prefix("bearer "))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::unauthorized("invalid bearer token"))?;
    Ok(token.to_string())
}

async fn credential_to_view(
    state: &AppState,
    model: credential::Model,
) -> Result<CredentialView, AppError> {
    let (has_access_token, has_refresh_token, limits) =
        credential_view_material(state, model.clone()).await?;
    let request_stats = request_stats_for_credential(state, &model.id).await?;
    let last_request_error = last_request_error_for_credential(state, &model.id).await?;
    CredentialView::from_model(
        model.clone(),
        state.credential_home(&model.id).display().to_string(),
        has_access_token,
        has_refresh_token,
        state.active_requests_for(&model.id),
        limits,
        request_stats,
        last_request_error,
    )
    .ok_or_else(|| AppError::internal("failed to render credential view"))
}

async fn api_key_to_view(state: &AppState, model: api_key::Model) -> Result<ApiKeyView, AppError> {
    let request_stats = request_stats_for_api_key(state, &model.id).await?;
    let last_request_error = last_request_error_for_api_key(state, &model.id).await?;
    Ok(ApiKeyView::from_model(
        model,
        request_stats,
        last_request_error,
    ))
}

fn auth_session_to_view(model: auth_session::Model) -> Result<AuthSessionView, AppError> {
    AuthSessionView::from_model(model)
        .ok_or_else(|| AppError::internal("failed to render auth session view"))
}

async fn find_credential(state: &AppState, id: &str) -> Result<credential::Model, AppError> {
    credential::Entity::find_by_id(id.to_string())
        .one(state.db())
        .await
        .map_err(|err| AppError::internal(err.to_string()))?
        .ok_or_else(|| AppError::not_found("credential not found"))
}

async fn find_auth_session(state: &AppState, id: &str) -> Result<auth_session::Model, AppError> {
    auth_session::Entity::find_by_id(id.to_string())
        .one(state.db())
        .await
        .map_err(|err| AppError::internal(err.to_string()))?
        .ok_or_else(|| AppError::not_found("auth session not found"))
}

async fn find_api_key(state: &AppState, id: &str) -> Result<api_key::Model, AppError> {
    api_key::Entity::find_by_id(id.to_string())
        .one(state.db())
        .await
        .map_err(|err| AppError::internal(err.to_string()))?
        .ok_or_else(|| AppError::not_found("api key not found"))
}

fn validate_create_credential(payload: &CreateCredentialRequest) -> Result<(), AppError> {
    if payload.name.trim().is_empty() {
        return Err(AppError::bad_request("credential name must not be empty"));
    }
    Ok(())
}

fn auth_server_options(state: &AppState, credential_id: &str) -> ServerOptions {
    let mut options = ServerOptions::new(
        state.credential_home(credential_id),
        state.config().auth_client_id.clone(),
        state.config().forced_chatgpt_workspace_id.clone(),
        codex_login::AuthCredentialsStoreMode::File,
    );
    options.issuer = state.config().auth_issuer.clone();
    options.open_browser = false;
    options
}

fn ensure_auth_session_state(
    session: &auth_session::Model,
    expected_method: AuthMethod,
    expected_status: AuthStatus,
) -> Result<(), AppError> {
    let actual_method = auth_session_method(session)?;
    if actual_method != expected_method {
        return Err(AppError::bad_request(format!(
            "auth session method mismatch: expected {}, got {}",
            expected_method.as_str(),
            actual_method.as_str()
        )));
    }

    let actual_status = auth_session_status(session)?;
    if actual_status != expected_status {
        return Err(AppError::bad_request(format!(
            "auth session status mismatch: expected {}, got {}",
            expected_status.as_str(),
            actual_status.as_str()
        )));
    }

    Ok(())
}

fn auth_session_method(session: &auth_session::Model) -> Result<AuthMethod, AppError> {
    AuthMethod::from_str(&session.method)
        .ok_or_else(|| AppError::internal("unsupported auth session method"))
}

fn auth_session_status(session: &auth_session::Model) -> Result<AuthStatus, AppError> {
    AuthStatus::from_str(&session.status)
        .ok_or_else(|| AppError::internal("unsupported auth session status"))
}

async fn set_auth_session_status(
    state: &AppState,
    auth_session_id: &str,
    status: AuthStatus,
    error_message: Option<String>,
) -> Result<auth_session::Model, AppError> {
    let existing = find_auth_session(state, auth_session_id).await?;
    let mut active = auth_session::ActiveModel::from(existing);
    active.status = Set(status.as_str().to_string());
    active.error_message = Set(error_message);
    active.completed_at = Set(match status {
        AuthStatus::Pending => None,
        AuthStatus::Completed | AuthStatus::Failed | AuthStatus::Cancelled => Some(Utc::now()),
    });
    active.updated_at = Set(Utc::now());
    active
        .update(state.db())
        .await
        .map_err(|err| AppError::internal(err.to_string()))
}

async fn mark_auth_session_failed(
    state: &AppState,
    auth_session_id: &str,
    error_message: String,
) -> Result<(), AppError> {
    let _ = set_auth_session_status(
        state,
        auth_session_id,
        AuthStatus::Failed,
        Some(error_message),
    )
    .await?;
    Ok(())
}

async fn cancel_pending_auth_sessions_for_credential(
    state: &AppState,
    credential_id: &str,
) -> Result<(), AppError> {
    let pending_sessions = auth_session::Entity::find()
        .filter(auth_session::Column::CredentialId.eq(credential_id.to_string()))
        .filter(auth_session::Column::Status.eq(AuthStatus::Pending.as_str()))
        .all(state.db())
        .await
        .map_err(|err| AppError::internal(err.to_string()))?;

    for session in pending_sessions {
        if let Some(cancellation) = state.take_auth_cancellation(&session.id) {
            cancellation.cancel();
        }
        let _ = set_auth_session_status(state, &session.id, AuthStatus::Cancelled, None).await?;
    }

    Ok(())
}

fn spawn_device_code_auth_task(
    state: AppState,
    auth_session_id: String,
    credential_id: String,
    options: ServerOptions,
    device_code: codex_login::DeviceCode,
) {
    let cancellation = tokio_util::sync::CancellationToken::new();
    state.register_auth_cancellation(auth_session_id.clone(), cancellation.clone());

    tokio::spawn(async move {
        let result = tokio::select! {
            _ = cancellation.cancelled() => Err("auth session was cancelled".to_string()),
            outcome = complete_device_code_login(options, device_code) => {
                outcome.map_err(|err| err.to_string())
            }
        };

        state.clear_auth_cancellation(&auth_session_id);

        match result {
            Ok(()) => {
                state.invalidate_auth_manager(&credential_id);
                if let Err(err) = state.sync_credential_from_auth(&credential_id).await {
                    let _ =
                        mark_auth_session_failed(&state, &auth_session_id, err.to_string()).await;
                    return;
                }
                let _ =
                    set_auth_session_status(&state, &auth_session_id, AuthStatus::Completed, None)
                        .await;
            }
            Err(message) if message == "auth session was cancelled" => {
                let _ =
                    set_auth_session_status(&state, &auth_session_id, AuthStatus::Cancelled, None)
                        .await;
            }
            Err(message) => {
                let _ = set_auth_session_status(
                    &state,
                    &auth_session_id,
                    AuthStatus::Failed,
                    Some(message),
                )
                .await;
            }
        }
    });
}

fn generate_proxy_api_key() -> String {
    let suffix = rand::rng()
        .sample_iter(Alphanumeric)
        .take(40)
        .map(char::from)
        .collect::<String>();
    format!("cpk_{suffix}")
}

fn build_client_response(
    status: StatusCode,
    response_headers: &HeaderMap,
    body: Body,
) -> Result<Response<Body>, AppError> {
    let mut builder = Response::builder().status(status);
    for (name, value) in response_headers {
        if should_skip_response_header(name.as_str()) {
            continue;
        }
        builder = builder.header(name, value);
    }
    builder
        .body(body)
        .map_err(|err| AppError::internal(err.to_string()))
}

async fn sync_credential_transient_state(
    state: &AppState,
    credential_id: &str,
    finalization: &RequestRecordFinalization,
) -> Result<(), AppError> {
    if finalization.request_success {
        state.clear_credential_error(credential_id).await?;
        return Ok(());
    }

    let message = finalization
        .error_message
        .clone()
        .or_else(|| finalization.error_code.clone())
        .or_else(|| {
            finalization
                .upstream_status_code
                .map(|status| format!("upstream returned {status}"))
        })
        .unwrap_or_else(|| "upstream request failed".to_string());
    state
        .record_credential_error(credential_id, message)
        .await?;
    Ok(())
}

fn ui_dist_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ui/dist")
}

async fn serve_ui_root() -> Result<Html<String>, AppError> {
    serve_ui_index().await
}

async fn serve_ui_spa(Path(path): Path<String>) -> Result<Html<String>, AppError> {
    if is_reserved_api_prefix(path.as_str()) {
        return Err(AppError::not_found("route not found"));
    }
    serve_ui_index().await
}

async fn serve_ui_index() -> Result<Html<String>, AppError> {
    let index_path = ui_dist_dir().join("index.html");
    let contents = tokio::fs::read_to_string(index_path)
        .await
        .map_err(|_| AppError::service_unavailable("frontend assets are not built"))?;
    Ok(Html(contents))
}

fn is_reserved_api_prefix(path: &str) -> bool {
    matches!(
        path.split('/').next().unwrap_or_default(),
        "admin" | "responses" | "v1" | "healthz" | "readyz"
    )
}
