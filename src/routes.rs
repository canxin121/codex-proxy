use crate::auth_flow;
use crate::entities::api_key;
use crate::entities::auth_session;
use crate::entities::credential;
use crate::entities::credential_limit;
use crate::error::AppError;
use crate::models::AdminLoginRequest;
use crate::models::AdminLoginResponse;
use crate::models::AdminSessionView;
use crate::models::ApiKeyView;
use crate::models::AuthMethod;
use crate::models::AuthSessionView;
use crate::models::AuthStatus;
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
use crate::request_stats::RequestRecordGuard;
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
use crate::state::AuthenticatedPrincipalKind;
use crate::state::RequestLease;
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
use axum::response::Redirect;
use axum::routing::get;
use axum::routing::get_service;
use axum::routing::post;
use chrono::Utc;
use codex_api::rate_limits::parse_all_rate_limits;
use codex_api::rate_limits::parse_rate_limit_event;
use codex_client::RetryOn;
use codex_client::RetryPolicy;
use codex_client::TransportError;
use codex_client::backoff;
use codex_client::maybe_build_rustls_client_config_with_custom_ca;
use codex_login::AuthManager;
use codex_login::CodexAuth;
use codex_login::ServerOptions;
use codex_login::complete_device_code_login;
use codex_login::default_client::default_headers as codex_default_headers;
use codex_login::request_device_code;
use futures::SinkExt;
use futures::StreamExt;
use rand::RngExt;
use rand::distr::Alphanumeric;
use sea_orm::ActiveModelTrait;
use sea_orm::ColumnTrait;
use sea_orm::EntityTrait;
use sea_orm::QueryFilter;
use sea_orm::QueryOrder;
use sea_orm::Set;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::path::PathBuf;
use tokio_tungstenite::Connector;
use tokio_tungstenite::connect_async_tls_with_config;
use tokio_tungstenite::tungstenite::Message as UpstreamWsMessage;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tower_http::services::ServeDir;
use tracing::warn;
use tungstenite::Error as UpstreamWsError;
use tungstenite::extensions::ExtensionsConfig;
use tungstenite::extensions::compression::deflate::DeflateConfig;
use tungstenite::protocol::WebSocketConfig;
use url::Url;
use uuid::Uuid;

#[derive(Debug, Deserialize, Default)]
struct BrowserAuthCallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

struct ConnectedUpstreamWebSocket {
    stream: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    response_headers: HeaderMap,
}

struct UpstreamWsHttpFailure {
    status: StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
}

enum UpstreamWsConnectOutcome {
    Connected(ConnectedUpstreamWebSocket),
    HttpFailure(UpstreamWsHttpFailure),
}

struct PreparedUpstreamHttpRequest {
    headers: reqwest::header::HeaderMap,
    body: Bytes,
}

struct ActiveWebsocketRequest {
    request_record: RequestRecordGuard,
    observation: RequestObservation,
    _lease: RequestLease,
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(health))
        .route("/readyz", get(health))
        .route("/admin/session", get(get_admin_session))
        .route("/admin/session/login", post(login_admin_session))
        .route("/admin/session/logout", post(logout_admin_session))
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
        .route(
            "/admin/auth/sessions/{id}/cancel",
            post(cancel_auth_session),
        )
        .route("/admin/auth/browser", post(start_browser_auth))
        .route("/admin/auth/browser/callback", get(browser_auth_callback))
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
        .nest_service(
            "/assets",
            get_service(ServeDir::new(ui_dist_dir().join("assets"))),
        )
        .route("/", get(serve_ui_root))
        .route("/{*path}", get(serve_ui_spa))
        .with_state(state)
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn login_admin_session(
    State(state): State<AppState>,
    Json(payload): Json<AdminLoginRequest>,
) -> Result<Json<AdminLoginResponse>, AppError> {
    if payload.password.trim().is_empty() {
        return Err(AppError::bad_request("admin password must not be empty"));
    }
    if !state.verify_admin_password(&payload.password) {
        return Err(AppError::unauthorized("invalid admin password"));
    }

    let created = state.create_admin_session();
    Ok(Json(AdminLoginResponse {
        session_token: created.session_token,
        session: AdminSessionView {
            principal_kind: "admin_session".to_string(),
            api_key_id: None,
            api_key_name: None,
            created_at: Some(created.created_at),
            last_used_at: Some(created.last_used_at),
            expires_at: Some(created.expires_at),
        },
    }))
}

async fn get_admin_session(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<AdminSessionView>, AppError> {
    let principal = require_admin(&state, &headers).await?;
    Ok(Json(admin_session_view(&principal)))
}

async fn logout_admin_session(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, AppError> {
    let bearer = extract_bearer(&headers)?;
    let principal = state.authenticate_bearer(&bearer, true, true).await?;
    if principal.principal_kind != AuthenticatedPrincipalKind::AdminSession {
        return Err(AppError::bad_request(
            "only password-based admin sessions can be logged out through this endpoint",
        ));
    }

    state.revoke_admin_session(&bearer);
    Ok(StatusCode::NO_CONTENT)
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
    Json(_payload): Json<CreateCredentialRequest>,
) -> Result<(StatusCode, Json<CredentialView>), AppError> {
    require_admin(&state, &headers).await?;
    let now = Utc::now();
    let credential_id = Uuid::new_v4().to_string();
    let model = credential::ActiveModel {
        id: Set(credential_id.clone()),
        name: Set(format!("importing-{}", &credential_id[..8])),
        kind: Set(crate::models::CredentialKind::ChatgptAuth
            .as_str()
            .to_string()),
        enabled: Set(true),
        selection_weight: Set(1),
        notes: Set(None),
        upstream_base_url: Set(None),
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

    let redirect_uri = browser_auth_api_callback_url(&headers, state.config())?;
    let auth_start = auth_flow::start_browser_auth(state.config(), redirect_uri);
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

async fn browser_auth_callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<BrowserAuthCallbackQuery>,
) -> Redirect {
    let Some(oauth_state) = query
        .state
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    else {
        return Redirect::to(&browser_auth_result_redirect_path(
            None,
            None,
            AuthStatus::Failed.as_str(),
            Some("oauth callback is missing state".to_string()),
        ));
    };

    let session = match find_pending_browser_auth_session_by_state(&state, oauth_state).await {
        Ok(session) => session,
        Err(err) => {
            return Redirect::to(&browser_auth_result_redirect_path(
                None,
                None,
                AuthStatus::Failed.as_str(),
                Some(err.to_string()),
            ));
        }
    };

    let callback_url = match browser_auth_callback_request_url(&headers, state.config(), &query) {
        Ok(url) => url,
        Err(err) => {
            let _ = mark_auth_session_failed(&state, &session.id, err.to_string()).await;
            return Redirect::to(&browser_auth_result_redirect_path(
                Some(&session.id),
                Some(&session.credential_id),
                AuthStatus::Failed.as_str(),
                Some(err.to_string()),
            ));
        }
    };

    let completion = match auth_flow::parse_browser_callback(&callback_url, oauth_state) {
        Ok(completion) => completion,
        Err(err) => {
            let _ = mark_auth_session_failed(&state, &session.id, err.to_string()).await;
            return Redirect::to(&browser_auth_result_redirect_path(
                Some(&session.id),
                Some(&session.credential_id),
                AuthStatus::Failed.as_str(),
                Some(err.to_string()),
            ));
        }
    };

    let redirect_uri = match session.redirect_uri.clone() {
        Some(value) => value,
        None => {
            let message = "browser auth session is missing redirect_uri".to_string();
            let _ = mark_auth_session_failed(&state, &session.id, message.clone()).await;
            return Redirect::to(&browser_auth_result_redirect_path(
                Some(&session.id),
                Some(&session.credential_id),
                AuthStatus::Failed.as_str(),
                Some(message),
            ));
        }
    };
    let pkce_code_verifier = match session.pkce_code_verifier.clone() {
        Some(value) => value,
        None => {
            let message = "browser auth session is missing pkce_code_verifier".to_string();
            let _ = mark_auth_session_failed(&state, &session.id, message.clone()).await;
            return Redirect::to(&browser_auth_result_redirect_path(
                Some(&session.id),
                Some(&session.credential_id),
                AuthStatus::Failed.as_str(),
                Some(message),
            ));
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
        let _ = mark_auth_session_failed(&state, &session.id, err.to_string()).await;
        return Redirect::to(&browser_auth_result_redirect_path(
            Some(&session.id),
            Some(&session.credential_id),
            AuthStatus::Failed.as_str(),
            Some(err.to_string()),
        ));
    }

    state.invalidate_auth_manager(&session.credential_id);
    if let Err(err) = state
        .sync_credential_from_auth(&session.credential_id)
        .await
    {
        let _ = mark_auth_session_failed(&state, &session.id, err.to_string()).await;
        return Redirect::to(&browser_auth_result_redirect_path(
            Some(&session.id),
            Some(&session.credential_id),
            AuthStatus::Failed.as_str(),
            Some(err.to_string()),
        ));
    }
    let update_result =
        set_auth_session_status(&state, &session.id, AuthStatus::Completed, None).await;
    match update_result {
        Ok(updated) => Redirect::to(&browser_auth_result_redirect_path(
            Some(&updated.id),
            Some(&updated.credential_id),
            AuthStatus::Completed.as_str(),
            None,
        )),
        Err(err) => Redirect::to(&browser_auth_result_redirect_path(
            Some(&session.id),
            Some(&session.credential_id),
            AuthStatus::Failed.as_str(),
            Some(err.to_string()),
        )),
    }
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
                        if !finalized && observation.is_terminal() {
                            let finalization = std::mem::take(&mut observation).finalize();
                            let _ = request_record.finalize(finalization.clone()).await;
                            let _ = sync_credential_transient_state(&state, &credential_id, &finalization).await;
                            finalized = true;
                        }
                        yield Result::<Bytes, std::io::Error>::Ok(bytes);
                    }
                    Err(err) => {
                        if !finalized {
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
                        }
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
) -> Result<Response<Body>, AppError> {
    let principal = require_client(&state, &headers).await?;
    let preferred_credential = preferred_credential_from_headers(&headers);
    let selected = state
        .select_credential(preferred_credential.as_deref())
        .await?;
    let client_headers = headers.clone();
    let credential = selected.model;
    let credential_id = credential.id.clone();
    let websocket_prompt_cache_key = resolve_session_key_from_headers(&client_headers)
        .unwrap_or_else(|| codex_prompt_cache_key(&credential.id));
    let manager = state.auth_manager(&credential_id).await;

    let upstream =
        match connect_ws_with_recovery(&state, &credential, &manager, &client_headers).await {
            Ok(UpstreamWsConnectOutcome::Connected(upstream)) => upstream,
            Ok(UpstreamWsConnectOutcome::HttpFailure(failure)) => {
                let mut request_record = start_request_record(
                    &state,
                    websocket_request_record_start(principal.clone(), credential.clone(), None),
                )
                .await?;
                state
                    .update_rate_limits_from_headers(&credential.id, &failure.headers)
                    .await?;
                let mut observation = RequestObservation::new(None);
                if let Ok(value) = serde_json::from_slice::<Value>(&failure.body) {
                    observation.observe_json_value(&value);
                }
                let finalization = observation.finish_http_response(failure.status);
                request_record.finalize(finalization.clone()).await?;
                sync_credential_transient_state(&state, &credential.id, &finalization).await?;
                return build_client_response(
                    failure.status,
                    &failure.headers,
                    Body::from(failure.body),
                );
            }
            Err(err) => {
                let mut request_record = start_request_record(
                    &state,
                    websocket_request_record_start(principal.clone(), credential.clone(), None),
                )
                .await?;
                let mut observation = RequestObservation::new(None);
                observation.mark_failure("upstream_connect", None, err.to_string(), None);
                let finalization = observation.finalize();
                request_record.finalize(finalization.clone()).await?;
                sync_credential_transient_state(&state, &credential.id, &finalization).await?;
                return Err(err);
            }
        };

    state
        .update_rate_limits_from_headers(&credential.id, &upstream.response_headers)
        .await?;

    let upstream_headers = upstream.response_headers.clone();
    let mut response = ws.on_upgrade(move |socket| async move {
        if let Err(err) = run_ws_proxy(
            socket,
            state,
            principal,
            credential.clone(),
            upstream.stream,
            websocket_prompt_cache_key.clone(),
        )
        .await
        {
            warn!(error = %err, credential_id = %credential_id, "websocket proxy ended with error");
        }
    });

    copy_upstream_ws_handshake_headers(response.headers_mut(), &upstream_headers);
    Ok(response)
}

async fn run_ws_proxy(
    mut client_socket: WebSocket,
    state: AppState,
    principal: AuthenticatedPrincipal,
    credential: credential::Model,
    mut upstream: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    prompt_cache_key: String,
) -> Result<(), AppError> {
    let mut current_request: Option<ActiveWebsocketRequest> = None;
    let mut terminal_error: Option<AppError> = None;

    loop {
        tokio::select! {
            maybe_client_message = client_socket.recv() => {
                let Some(client_message) = maybe_client_message else {
                    if let Some(active_request) = current_request.as_mut() {
                        active_request.observation.mark_failure_if_missing(
                            "client_websocket",
                            Some("client_closed".to_string()),
                            "client websocket closed before response.completed",
                            None,
                        );
                        let finalization = std::mem::take(&mut active_request.observation).finish_websocket();
                        finalize_active_websocket_request(
                            &state,
                            &credential.id,
                            &mut current_request,
                            finalization,
                        )
                        .await?;
                    }
                    let _ = upstream.send(UpstreamWsMessage::Close(None)).await;
                    break;
                };
                match client_message {
                    Ok(message) => {
                        let is_client_close = matches!(message, ClientWsMessage::Close(_));
                        if let ClientWsMessage::Text(text) = &message {
                            let requested_model = extract_requested_model_from_ws_text(text.as_str());
                            if let Some(active_request) = current_request.as_mut() {
                                active_request
                                    .observation
                                    .set_requested_model_if_missing(requested_model);
                            } else {
                                current_request = Some(
                                    start_active_websocket_request(
                                        &state,
                                        &principal,
                                        &credential,
                                        requested_model,
                                    )
                                    .await?,
                                );
                            }
                        }
                        let Some(upstream_message) = map_client_ws_message(
                            message,
                            Some(prompt_cache_key.as_str()),
                        ) else {
                            continue;
                        };
                        if let Err(err) = upstream.send(upstream_message).await {
                            if let Some(active_request) = current_request.as_mut() {
                                active_request.observation.mark_failure_if_missing(
                                    "upstream_websocket_send",
                                    Some("upstream_send_failed".to_string()),
                                    err.to_string(),
                                    None,
                                );
                                let finalization = std::mem::take(&mut active_request.observation).finish_websocket();
                                finalize_active_websocket_request(
                                    &state,
                                    &credential.id,
                                    &mut current_request,
                                    finalization,
                                )
                                .await?;
                            } else {
                                terminal_error = Some(AppError::bad_gateway(err.to_string()));
                            }
                            break;
                        }
                        if is_client_close {
                            if let Some(active_request) = current_request.as_mut() {
                                active_request.observation.mark_failure_if_missing(
                                    "client_websocket",
                                    Some("client_closed".to_string()),
                                    "client websocket closed before response.completed",
                                    None,
                                );
                                let finalization = std::mem::take(&mut active_request.observation).finish_websocket();
                                finalize_active_websocket_request(
                                    &state,
                                    &credential.id,
                                    &mut current_request,
                                    finalization,
                                )
                                .await?;
                            }
                            break;
                        }
                    }
                    Err(err) => {
                        if let Some(active_request) = current_request.as_mut() {
                            active_request.observation.mark_failure_if_missing(
                                "client_websocket",
                                Some("client_receive_error".to_string()),
                                format!("client websocket error: {err}"),
                                None,
                            );
                            let finalization = std::mem::take(&mut active_request.observation).finish_websocket();
                            finalize_active_websocket_request(
                                &state,
                                &credential.id,
                                &mut current_request,
                                finalization,
                            )
                            .await?;
                        }
                        break;
                    }
                }
            }
            maybe_upstream_message = upstream.next() => {
                let Some(upstream_message) = maybe_upstream_message else {
                    if let Some(active_request) = current_request.as_mut() {
                        active_request.observation.mark_failure_if_missing(
                            "upstream_websocket",
                            Some("upstream_closed".to_string()),
                            "websocket closed before response.completed",
                            None,
                        );
                        let finalization = std::mem::take(&mut active_request.observation).finish_websocket();
                        finalize_active_websocket_request(
                            &state,
                            &credential.id,
                            &mut current_request,
                            finalization,
                        )
                        .await?;
                    }
                    let _ = client_socket.send(ClientWsMessage::Close(None)).await;
                    break;
                };
                match upstream_message {
                    Ok(message) => {
                        let is_upstream_close = matches!(message, UpstreamWsMessage::Close(_));
                        let client_message = match map_upstream_ws_message(
                            &state,
                            &credential.id,
                            current_request
                                .as_mut()
                                .map(|active_request| &mut active_request.observation),
                            message,
                        )
                        .await
                        {
                            Ok(message) => message,
                            Err(err) => {
                                if let Some(active_request) = current_request.as_mut() {
                                    active_request.observation.mark_failure_if_missing(
                                        "upstream_websocket",
                                        Some("upstream_event_processing_failed".to_string()),
                                        err.to_string(),
                                        None,
                                    );
                                    let finalization = std::mem::take(&mut active_request.observation).finish_websocket();
                                    finalize_active_websocket_request(
                                        &state,
                                        &credential.id,
                                        &mut current_request,
                                        finalization,
                                    )
                                    .await?;
                                } else {
                                    terminal_error = Some(err);
                                }
                                break;
                            }
                        };

                        if let Some(active_request) = current_request.as_mut()
                            && active_request.observation.is_terminal()
                        {
                            let finalization = std::mem::take(&mut active_request.observation).finalize();
                            finalize_active_websocket_request(
                                &state,
                                &credential.id,
                                &mut current_request,
                                finalization,
                            )
                            .await?;
                        }

                        let Some(client_message) = client_message else {
                            if is_upstream_close {
                                let _ = client_socket.send(ClientWsMessage::Close(None)).await;
                                break;
                            }
                            continue;
                        };
                        if let Err(err) = client_socket.send(client_message).await {
                            if let Some(active_request) = current_request.as_mut() {
                                active_request.observation.mark_failure_if_missing(
                                    "client_websocket_send",
                                    Some("client_send_failed".to_string()),
                                    err.to_string(),
                                    None,
                                );
                                let finalization = std::mem::take(&mut active_request.observation).finish_websocket();
                                finalize_active_websocket_request(
                                    &state,
                                    &credential.id,
                                    &mut current_request,
                                    finalization,
                                )
                                .await?;
                            }
                            break;
                        }
                        if is_upstream_close {
                            if let Some(active_request) = current_request.as_mut() {
                                active_request.observation.mark_failure_if_missing(
                                    "upstream_websocket",
                                    Some("upstream_closed".to_string()),
                                    "websocket closed before response.completed",
                                    None,
                                );
                                let finalization = std::mem::take(&mut active_request.observation).finish_websocket();
                                finalize_active_websocket_request(
                                    &state,
                                    &credential.id,
                                    &mut current_request,
                                    finalization,
                                )
                                .await?;
                            }
                            break;
                        }
                    }
                    Err(err) => {
                        if let Some(active_request) = current_request.as_mut() {
                            active_request.observation.mark_failure_if_missing(
                                "upstream_websocket",
                                Some("upstream_receive_error".to_string()),
                                format!("upstream websocket error: {err}"),
                                None,
                            );
                            let finalization = std::mem::take(&mut active_request.observation).finish_websocket();
                            finalize_active_websocket_request(
                                &state,
                                &credential.id,
                                &mut current_request,
                                finalization,
                            )
                            .await?;
                        } else {
                            terminal_error = Some(AppError::bad_gateway(err.to_string()));
                        }
                        let _ = client_socket.send(ClientWsMessage::Close(None)).await;
                        break;
                    }
                }
            }
        }
    }

    if let Some(err) = terminal_error {
        return Err(err);
    }

    Ok(())
}

fn websocket_request_record_start(
    principal: AuthenticatedPrincipal,
    credential: credential::Model,
    requested_model: Option<String>,
) -> RequestRecordStart {
    RequestRecordStart {
        principal,
        credential,
        transport: "websocket",
        method: Method::GET.to_string(),
        path: "/responses".to_string(),
        requested_model,
    }
}

async fn start_active_websocket_request(
    state: &AppState,
    principal: &AuthenticatedPrincipal,
    credential: &credential::Model,
    requested_model: Option<String>,
) -> Result<ActiveWebsocketRequest, AppError> {
    let lease = state.acquire_request_lease(credential.id.clone());
    state.record_credential_touch(&credential.id).await?;
    let request_record = start_request_record(
        state,
        websocket_request_record_start(
            principal.clone(),
            credential.clone(),
            requested_model.clone(),
        ),
    )
    .await?;
    Ok(ActiveWebsocketRequest {
        request_record,
        observation: RequestObservation::new(requested_model),
        _lease: lease,
    })
}

async fn finalize_active_websocket_request(
    state: &AppState,
    credential_id: &str,
    current_request: &mut Option<ActiveWebsocketRequest>,
    finalization: RequestRecordFinalization,
) -> Result<(), AppError> {
    let Some(mut active_request) = current_request.take() else {
        return Ok(());
    };
    active_request
        .request_record
        .finalize(finalization.clone())
        .await?;
    sync_credential_transient_state(state, credential_id, &finalization).await
}

async fn connect_ws_with_recovery(
    state: &AppState,
    credential: &credential::Model,
    manager: &std::sync::Arc<AuthManager>,
    client_headers: &HeaderMap,
) -> Result<UpstreamWsConnectOutcome, AppError> {
    let mut recovery = Some(manager.unauthorized_recovery());
    loop {
        let auth = manager
            .auth()
            .await
            .ok_or_else(|| AppError::service_unavailable("credential auth is unavailable"))?;
        let request = build_upstream_ws_request(state, credential, client_headers, &auth)?;
        match connect_upstream_ws(request).await {
            Ok((stream, response_headers)) => {
                return Ok(UpstreamWsConnectOutcome::Connected(
                    ConnectedUpstreamWebSocket {
                        stream,
                        response_headers,
                    },
                ));
            }
            Err(UpstreamWsError::Http(response))
                if response.status() == StatusCode::UNAUTHORIZED =>
            {
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
                return Ok(UpstreamWsConnectOutcome::HttpFailure(
                    upstream_ws_http_failure(*response),
                ));
            }
            Err(UpstreamWsError::Http(response)) => {
                return Ok(UpstreamWsConnectOutcome::HttpFailure(
                    upstream_ws_http_failure(*response),
                ));
            }
            Err(err) => return Err(AppError::bad_gateway(err.to_string())),
        }
    }
}

async fn connect_upstream_ws(
    request: http::Request<()>,
) -> Result<
    (
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        HeaderMap,
    ),
    UpstreamWsError,
> {
    let connector = maybe_build_rustls_client_config_with_custom_ca()
        .map_err(|err| UpstreamWsError::Io(std::io::Error::other(err.to_string())))?
        .map(Connector::Rustls);

    let (stream, response) = tokio::time::timeout(
        upstream_websocket_connect_timeout(),
        connect_async_tls_with_config(request, Some(upstream_websocket_config()), false, connector),
    )
    .await
    .map_err(|_| {
        UpstreamWsError::Io(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "upstream websocket connect timed out",
        ))
    })??;

    Ok((stream, response.headers().clone()))
}

fn upstream_websocket_connect_timeout() -> std::time::Duration {
    std::time::Duration::from_millis(15_000)
}

fn upstream_websocket_config() -> WebSocketConfig {
    let mut extensions = ExtensionsConfig::default();
    extensions.permessage_deflate = Some(DeflateConfig::default());

    let mut config = WebSocketConfig::default();
    config.extensions = extensions;
    config
}

fn upstream_ws_http_failure(response: http::Response<Option<Vec<u8>>>) -> UpstreamWsHttpFailure {
    let (parts, body) = response.into_parts();
    UpstreamWsHttpFailure {
        status: parts.status,
        headers: parts.headers,
        body: body.unwrap_or_default(),
    }
}

async fn map_upstream_ws_message(
    state: &AppState,
    credential_id: &str,
    observation: Option<&mut RequestObservation>,
    message: UpstreamWsMessage,
) -> Result<Option<ClientWsMessage>, AppError> {
    match message {
        UpstreamWsMessage::Text(text) => {
            sync_rate_limits_from_ws_text(state, credential_id, text.as_str()).await?;
            if let Some(observation) = observation
                && let Ok(value) = serde_json::from_str::<Value>(text.as_str())
            {
                observation.observe_json_value(&value);
            }
            Ok(Some(ClientWsMessage::Text(text.to_string().into())))
        }
        UpstreamWsMessage::Binary(data) => Ok(Some(ClientWsMessage::Binary(data))),
        UpstreamWsMessage::Ping(data) => Ok(Some(ClientWsMessage::Ping(data))),
        UpstreamWsMessage::Pong(data) => Ok(Some(ClientWsMessage::Pong(data))),
        UpstreamWsMessage::Close(close) => Ok(Some(ClientWsMessage::Close(
            close.map(map_upstream_close_frame),
        ))),
        UpstreamWsMessage::Frame(_) => Ok(None),
    }
}

fn map_client_ws_message(
    message: ClientWsMessage,
    prompt_cache_key: Option<&str>,
) -> Option<UpstreamWsMessage> {
    match message {
        ClientWsMessage::Text(text) => {
            if let Some(prompt_cache_key) = prompt_cache_key
                && let Some((normalized, _)) =
                    normalize_prompt_cache_key(text.as_bytes(), prompt_cache_key)
            {
                return Some(UpstreamWsMessage::Text(
                    String::from_utf8(normalized)
                        .expect("normalized prompt cache key body should remain UTF-8")
                        .into(),
                ));
            }
            Some(UpstreamWsMessage::Text(text.to_string().into()))
        }
        ClientWsMessage::Binary(data) => Some(UpstreamWsMessage::Binary(data)),
        ClientWsMessage::Ping(data) => Some(UpstreamWsMessage::Ping(data)),
        ClientWsMessage::Pong(data) => Some(UpstreamWsMessage::Pong(data)),
        ClientWsMessage::Close(close) => {
            Some(UpstreamWsMessage::Close(close.map(map_client_close_frame)))
        }
    }
}

fn map_upstream_close_frame(
    close: tungstenite::protocol::CloseFrame,
) -> axum::extract::ws::CloseFrame {
    axum::extract::ws::CloseFrame {
        code: u16::from(close.code),
        reason: close.reason.to_string().into(),
    }
}

fn map_client_close_frame(
    close: axum::extract::ws::CloseFrame,
) -> tungstenite::protocol::CloseFrame {
    tungstenite::protocol::CloseFrame {
        code: close.code.into(),
        reason: close.reason.to_string().into(),
    }
}

async fn sync_rate_limits_from_ws_text(
    state: &AppState,
    credential_id: &str,
    text: &str,
) -> Result<(), AppError> {
    if let Some(snapshot) = parse_rate_limit_event(text) {
        state
            .update_rate_limit_snapshot(credential_id, snapshot)
            .await?;
    }

    let json = match serde_json::from_str::<WsEnvelope>(text) {
        Ok(json) => json,
        Err(_) => return Ok(()),
    };

    if let Some(header_map) = maybe_extract_header_map_for_rate_limits(json.headers) {
        state
            .update_rate_limits_from_headers(credential_id, &header_map)
            .await?;
    }
    if let Some(header_map) = maybe_extract_header_map_for_rate_limits(
        json.response.and_then(|response| response.headers),
    ) {
        state
            .update_rate_limits_from_headers(credential_id, &header_map)
            .await?;
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct WsEnvelope {
    headers: Option<Value>,
    response: Option<WsResponseEnvelope>,
}

#[derive(Debug, Deserialize)]
struct WsResponseEnvelope {
    headers: Option<Value>,
}

fn maybe_extract_header_map_for_rate_limits(headers: Option<Value>) -> Option<HeaderMap> {
    let header_map = header_map_from_json(headers?);
    let snapshots = parse_all_rate_limits(&header_map);
    if snapshots.is_empty() {
        None
    } else {
        Some(header_map)
    }
}

fn header_map_from_json(value: Value) -> HeaderMap {
    let Some(object) = value.as_object() else {
        return HeaderMap::new();
    };
    let mut headers = HeaderMap::new();
    for (name, value) in object {
        let Some(text) = json_header_value(value) else {
            continue;
        };
        let Ok(header_name) = http::header::HeaderName::try_from(name.as_str()) else {
            continue;
        };
        let Ok(header_value) = http::HeaderValue::from_str(text.as_ref()) else {
            continue;
        };
        let _ = headers.insert(header_name, header_value);
    }
    headers
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

fn insert_header_value(headers: &mut HeaderMap, name: String, value: String) {
    if let (Ok(name), Ok(value)) = (
        http::header::HeaderName::try_from(name),
        http::HeaderValue::from_str(&value),
    ) {
        let _ = headers.insert(name, value);
    }
}

fn codex_prompt_cache_key(seed: &str) -> String {
    let digest = Sha256::digest(format!("codex-proxy:prompt-cache:{seed}"));
    let mut key = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        write!(&mut key, "{byte:02x}").expect("writing to a string should not fail");
    }
    key
}

fn normalize_prompt_cache_key(raw_json: &[u8], fallback_key: &str) -> Option<(Vec<u8>, String)> {
    if raw_json.is_empty() {
        return None;
    }

    let mut value: Value = serde_json::from_slice(raw_json).ok()?;
    if let Some(existing_key) = value
        .get("prompt_cache_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|key| !key.is_empty())
    {
        return Some((raw_json.to_vec(), existing_key.to_string()));
    }

    let object = value.as_object_mut()?;
    object.insert(
        "prompt_cache_key".to_string(),
        Value::String(fallback_key.to_string()),
    );
    let bytes = serde_json::to_vec(&value).ok()?;
    Some((bytes, fallback_key.to_string()))
}

fn normalize_prompt_cache_key_for_request(
    client_headers: &HeaderMap,
    body: Bytes,
    fallback_key: &str,
) -> Result<(Bytes, String), AppError> {
    if body.is_empty() {
        return Ok((body, fallback_key.to_string()));
    }

    let content_encoding = client_headers
        .get(reqwest::header::CONTENT_ENCODING)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .unwrap_or("");

    if content_encoding.eq_ignore_ascii_case("zstd") {
        let decoded = zstd::stream::decode_all(std::io::Cursor::new(body.as_ref()))
            .map_err(|err| AppError::bad_request(err.to_string()))?;
        if let Some((normalized, prompt_cache_key)) =
            normalize_prompt_cache_key(&decoded, fallback_key)
        {
            let encoded = zstd::stream::encode_all(std::io::Cursor::new(normalized.as_slice()), 3)
                .map_err(|err| AppError::internal(err.to_string()))?;
            return Ok((Bytes::from(encoded), prompt_cache_key));
        }
        return Ok((body, fallback_key.to_string()));
    }

    if let Some((normalized, prompt_cache_key)) =
        normalize_prompt_cache_key(body.as_ref(), fallback_key)
    {
        return Ok((Bytes::from(normalized), prompt_cache_key));
    }

    Ok((body, fallback_key.to_string()))
}

fn resolve_session_key_from_headers(headers: &HeaderMap) -> Option<String> {
    const CANDIDATE_HEADERS: &[&str] = &[
        "x-client-request-id",
        "session_id",
        "conversation_id",
        "x-session-id",
        "session-id",
        "conversation-id",
        "conversationid",
        "sessionid",
    ];

    CANDIDATE_HEADERS.iter().find_map(|name| {
        headers
            .get(*name)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

fn set_prompt_cache_headers(headers: &mut HeaderMap, prompt_cache_key: &str) {
    if prompt_cache_key.is_empty() {
        return;
    }

    insert_header_value(
        headers,
        "x-client-request-id".to_string(),
        prompt_cache_key.to_string(),
    );
    insert_header_value(
        headers,
        "session_id".to_string(),
        prompt_cache_key.to_string(),
    );
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
    let retry_policy = upstream_http_retry_policy();
    let method = reqwest::Method::from_bytes(method.as_str().as_bytes())
        .map_err(|err| AppError::bad_request(err.to_string()))?;

    'auth: loop {
        let auth = manager
            .auth()
            .await
            .ok_or_else(|| AppError::service_unavailable("credential auth is unavailable"))?;
        let url = upstream_http_url(state, credential, normalized_path);
        let prepared = prepare_upstream_http_request(
            state,
            credential,
            client_headers,
            &auth,
            normalized_path,
            body.clone(),
        )?;
        let mut attempt = 0;

        loop {
            let mut request = state
                .http_client()
                .request(method.clone(), url.clone())
                .headers(prepared.headers.clone());
            if !prepared.body.is_empty() {
                request = request.body(prepared.body.clone());
            }

            match request.send().await {
                Ok(response) => {
                    if response.status() == StatusCode::UNAUTHORIZED {
                        if let Some(recovery) = recovery.as_mut()
                            && recovery.has_next()
                        {
                            recovery
                                .next()
                                .await
                                .map_err(|err| AppError::bad_gateway(err.to_string()))?;
                            state.sync_credential_from_auth(&credential.id).await?;
                            continue 'auth;
                        }
                        return Ok(response);
                    }

                    if should_retry_upstream_http_status(&retry_policy, response.status(), attempt)
                    {
                        attempt += 1;
                        drop(response);
                        tokio::time::sleep(backoff(retry_policy.base_delay, attempt)).await;
                        continue;
                    }

                    return Ok(response);
                }
                Err(err) => {
                    if should_retry_upstream_http_error(&retry_policy, &err, attempt) {
                        attempt += 1;
                        tokio::time::sleep(backoff(retry_policy.base_delay, attempt)).await;
                        continue;
                    }

                    return Err(AppError::bad_gateway(err.to_string()));
                }
            }
        }
    }
}

fn upstream_http_retry_policy() -> RetryPolicy {
    RetryPolicy {
        max_attempts: 4,
        base_delay: std::time::Duration::from_millis(200),
        retry_on: RetryOn {
            retry_429: false,
            retry_5xx: true,
            retry_transport: true,
        },
    }
}

fn should_retry_upstream_http_error(
    policy: &RetryPolicy,
    err: &reqwest::Error,
    attempt: u64,
) -> bool {
    policy.retry_on.should_retry(
        &if err.is_timeout() {
            TransportError::Timeout
        } else {
            TransportError::Network(err.to_string())
        },
        attempt,
        policy.max_attempts,
    )
}

fn should_retry_upstream_http_status(
    policy: &RetryPolicy,
    status: StatusCode,
    attempt: u64,
) -> bool {
    policy.retry_on.should_retry(
        &TransportError::Http {
            status,
            url: None,
            headers: None,
            body: None,
        },
        attempt,
        policy.max_attempts,
    )
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

fn prepare_upstream_http_request(
    state: &AppState,
    credential: &credential::Model,
    client_headers: &HeaderMap,
    auth: &CodexAuth,
    normalized_path: &str,
    body: Bytes,
) -> Result<PreparedUpstreamHttpRequest, AppError> {
    let mut headers = build_upstream_http_headers(client_headers, &auth)?;
    let fallback_prompt_cache_key = resolve_session_key_from_headers(client_headers)
        .unwrap_or_else(|| codex_prompt_cache_key(&credential.id));
    let (body, prompt_cache_key) =
        normalize_prompt_cache_key_for_request(client_headers, body, &fallback_prompt_cache_key)?;
    set_prompt_cache_headers(&mut headers, &prompt_cache_key);
    let base_url = state.provider_base_url(credential);
    let body = maybe_compress_upstream_request_body(
        &mut headers,
        &base_url,
        auth,
        normalized_path,
        client_headers,
        body,
    )?;

    Ok(PreparedUpstreamHttpRequest { headers, body })
}

fn maybe_compress_upstream_request_body(
    headers: &mut reqwest::header::HeaderMap,
    base_url: &str,
    auth: &CodexAuth,
    normalized_path: &str,
    client_headers: &HeaderMap,
    body: Bytes,
) -> Result<Bytes, AppError> {
    if !should_use_upstream_request_compression(
        base_url,
        auth,
        normalized_path,
        client_headers,
        &body,
    ) {
        return Ok(body);
    }

    headers.insert(
        reqwest::header::CONTENT_ENCODING,
        reqwest::header::HeaderValue::from_static("zstd"),
    );
    if !headers.contains_key(reqwest::header::CONTENT_TYPE) {
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            reqwest::header::HeaderValue::from_static("application/json"),
        );
    }

    let compressed = zstd::stream::encode_all(std::io::Cursor::new(body.as_ref()), 3)
        .map_err(|err| AppError::internal(err.to_string()))?;
    Ok(Bytes::from(compressed))
}

fn should_use_upstream_request_compression(
    base_url: &str,
    auth: &CodexAuth,
    normalized_path: &str,
    client_headers: &HeaderMap,
    body: &Bytes,
) -> bool {
    normalized_path == "/responses"
        && !body.is_empty()
        && auth.is_chatgpt_auth()
        && !client_headers.contains_key(http::header::CONTENT_ENCODING)
        && upstream_base_url_supports_request_compression(base_url)
}

fn upstream_base_url_supports_request_compression(base_url: &str) -> bool {
    let Some(host) = Url::parse(base_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_ascii_lowercase))
    else {
        return false;
    };

    host == "chatgpt.com"
        || host.ends_with(".chatgpt.com")
        || host == "openai.com"
        || host.ends_with(".openai.com")
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
    insert_missing_default_reqwest_headers(&mut headers);

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
    const OPENAI_BETA_HEADER: &str = "OpenAI-Beta";
    const RESPONSES_WEBSOCKETS_V2_BETA_HEADER_VALUE: &str = "responses_websockets=2026-02-06";

    let url = upstream_websocket_url(&upstream_http_url(state, credential, "/responses"))?;
    let mut request = url
        .into_client_request()
        .map_err(|err| AppError::bad_request(err.to_string()))?;
    let prompt_cache_key = resolve_session_key_from_headers(client_headers)
        .unwrap_or_else(|| codex_prompt_cache_key(&credential.id));
    {
        let headers = request.headers_mut();
        for (name, value) in client_headers {
            if should_skip_ws_request_header(name.as_str()) {
                continue;
            }
            let _ = headers.insert(name, value.clone());
        }
        insert_missing_default_http_headers(headers);
        let _ = headers.insert(
            OPENAI_BETA_HEADER,
            http::HeaderValue::from_static(RESPONSES_WEBSOCKETS_V2_BETA_HEADER_VALUE),
        );
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
        set_prompt_cache_headers(headers, &prompt_cache_key);
    }
    Ok(request)
}

fn upstream_websocket_url(upstream_http_url: &str) -> Result<String, AppError> {
    let mut url =
        Url::parse(upstream_http_url).map_err(|err| AppError::bad_request(err.to_string()))?;

    let scheme = match url.scheme() {
        "http" => "ws",
        "https" => "wss",
        "ws" | "wss" => return Ok(url.to_string()),
        _ => return Ok(url.to_string()),
    };
    let _ = url.set_scheme(scheme);
    Ok(url.to_string())
}

fn insert_missing_default_reqwest_headers(headers: &mut reqwest::header::HeaderMap) {
    for (name, value) in &codex_default_headers() {
        if !headers.contains_key(name) {
            let _ = headers.insert(name, value.clone());
        }
    }
}

fn insert_missing_default_http_headers(headers: &mut HeaderMap) {
    for (name, value) in &codex_default_headers() {
        if !headers.contains_key(name) {
            let _ = headers.insert(name, value.clone());
        }
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
            | "x-codex-proxy-credential-id"
    )
}

fn should_skip_ws_request_header(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    should_skip_request_header(name.as_str())
        || (name.starts_with("sec-websocket-") && name != "sec-websocket-protocol")
}

fn should_skip_ws_handshake_response_header(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    should_skip_response_header(name.as_str())
        || matches!(
            name.as_str(),
            "sec-websocket-accept" | "sec-websocket-extensions"
        )
}

fn copy_upstream_ws_handshake_headers(target: &mut HeaderMap, upstream_headers: &HeaderMap) {
    for (name, value) in upstream_headers {
        if should_skip_ws_handshake_response_header(name.as_str()) {
            continue;
        }
        let _ = target.insert(name, value.clone());
    }
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

async fn require_admin(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<AuthenticatedPrincipal, AppError> {
    let bearer = extract_bearer(headers)?;
    state.authenticate_bearer(&bearer, true, true).await
}

async fn require_client(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<AuthenticatedPrincipal, AppError> {
    let bearer = extract_bearer(headers)?;
    state.authenticate_bearer(&bearer, false, false).await
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

fn admin_session_view(principal: &AuthenticatedPrincipal) -> AdminSessionView {
    AdminSessionView {
        principal_kind: principal.principal_kind.as_str().to_string(),
        api_key_id: principal.api_key_id.clone(),
        api_key_name: principal.api_key_name.clone(),
        created_at: principal.admin_session_created_at,
        last_used_at: principal.admin_session_last_used_at,
        expires_at: principal.admin_session_expires_at,
    }
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

async fn find_pending_browser_auth_session_by_state(
    state: &AppState,
    oauth_state: &str,
) -> Result<auth_session::Model, AppError> {
    auth_session::Entity::find()
        .filter(auth_session::Column::Method.eq(AuthMethod::Browser.as_str()))
        .filter(auth_session::Column::Status.eq(AuthStatus::Pending.as_str()))
        .filter(auth_session::Column::OauthState.eq(oauth_state.to_string()))
        .order_by_desc(auth_session::Column::CreatedAt)
        .one(state.db())
        .await
        .map_err(|err| AppError::internal(err.to_string()))?
        .ok_or_else(|| AppError::not_found("browser auth session not found for callback state"))
}

fn header_string(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn forwarded_pair(forwarded: &str, key: &str) -> Option<String> {
    forwarded
        .split(',')
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .split(';')
        .find_map(|part| {
            let mut pieces = part.trim().splitn(2, '=');
            let name = pieces.next()?.trim();
            let value = pieces.next()?.trim().trim_matches('"');
            if name.eq_ignore_ascii_case(key) && !value.is_empty() {
                Some(value.to_string())
            } else {
                None
            }
        })
}

fn origin_from_url(url_value: &str) -> Option<String> {
    let url = Url::parse(url_value).ok()?;
    let host = url.host_str()?;
    let mut origin = format!("{}://{}", url.scheme(), host);
    if let Some(port) = url.port() {
        origin.push(':');
        origin.push_str(&port.to_string());
    }
    Some(origin)
}

fn default_scheme_for_host(host: &str) -> &'static str {
    let host = host.trim();
    if host.starts_with("localhost") || host.starts_with("127.0.0.1") || host.starts_with("[::1]") {
        "http"
    } else {
        "https"
    }
}

fn public_base_url(
    headers: &HeaderMap,
    config: &crate::config::AppConfig,
) -> Result<String, AppError> {
    if let Some(origin) = config.public_base_url.as_deref().and_then(origin_from_url) {
        return Ok(origin);
    }

    if let Some(origin) = header_string(headers, "origin").and_then(|value| origin_from_url(&value))
    {
        return Ok(origin);
    }

    let forwarded_header = header_string(headers, "forwarded");
    let forwarded_host = header_string(headers, "x-forwarded-host")
        .or_else(|| {
            forwarded_header
                .as_deref()
                .and_then(|value| forwarded_pair(value, "host"))
        })
        .map(|value| {
            value
                .split(',')
                .next()
                .unwrap_or_default()
                .trim()
                .to_string()
        })
        .filter(|value| !value.is_empty());
    let host = forwarded_host
        .or_else(|| header_string(headers, "host"))
        .map(|value| {
            value
                .split(',')
                .next()
                .unwrap_or_default()
                .trim()
                .to_string()
        })
        .filter(|value| !value.is_empty());
    if let Some(host) = host {
        let scheme = header_string(headers, "x-forwarded-proto")
            .or_else(|| {
                forwarded_header
                    .as_deref()
                    .and_then(|value| forwarded_pair(value, "proto"))
            })
            .map(|value| {
                value
                    .split(',')
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .to_string()
            })
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| default_scheme_for_host(&host).to_string());
        return Ok(format!("{scheme}://{host}"));
    }

    Err(AppError::bad_request(
        "unable to determine public base url for browser auth callback; provide Host/X-Forwarded-* headers or configure CODEX_PROXY_PUBLIC_BASE_URL",
    ))
}

fn browser_auth_api_callback_url(
    headers: &HeaderMap,
    config: &crate::config::AppConfig,
) -> Result<String, AppError> {
    Ok(format!(
        "{}{}",
        public_base_url(headers, config)?,
        "/admin/auth/browser/callback"
    ))
}

fn browser_auth_callback_request_url(
    headers: &HeaderMap,
    config: &crate::config::AppConfig,
    query: &BrowserAuthCallbackQuery,
) -> Result<String, AppError> {
    let mut url = Url::parse(&browser_auth_api_callback_url(headers, config)?)
        .map_err(|err| AppError::internal(err.to_string()))?;
    {
        let mut pairs = url.query_pairs_mut();
        if let Some(value) = query.code.as_deref() {
            pairs.append_pair("code", value);
        }
        if let Some(value) = query.state.as_deref() {
            pairs.append_pair("state", value);
        }
        if let Some(value) = query.error.as_deref() {
            pairs.append_pair("error", value);
        }
        if let Some(value) = query.error_description.as_deref() {
            pairs.append_pair("error_description", value);
        }
    }
    Ok(url.to_string())
}

fn browser_auth_result_redirect_path(
    auth_session_id: Option<&str>,
    credential_id: Option<&str>,
    auth_status: &str,
    error: Option<String>,
) -> String {
    let mut query = vec![format!("auth_status={}", urlencoding::encode(auth_status))];
    if let Some(auth_session_id) = auth_session_id {
        query.push(format!(
            "auth_session_id={}",
            urlencoding::encode(auth_session_id)
        ));
    }
    if let Some(credential_id) = credential_id {
        query.push(format!(
            "credential_id={}",
            urlencoding::encode(credential_id)
        ));
    }
    if let Some(error) = error {
        query.push(format!("error={}", urlencoding::encode(&error)));
    }
    format!("/auth/callback?{}", query.join("&"))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_header_filter_removes_proxy_internal_header() {
        assert!(should_skip_request_header("x-codex-proxy-credential-id"));
        assert!(!should_skip_request_header("x-codex-turn-state"));
        assert!(should_skip_ws_request_header("sec-websocket-key"));
        assert!(!should_skip_ws_request_header("sec-websocket-protocol"));
    }

    #[test]
    fn codex_prompt_cache_key_is_stable_and_distinct() {
        let first = codex_prompt_cache_key("credential-1");
        let second = codex_prompt_cache_key("credential-1");
        let other = codex_prompt_cache_key("credential-2");

        assert_eq!(first, second);
        assert_ne!(first, other);
    }

    #[test]
    fn normalize_prompt_cache_key_injects_fallback_for_missing_and_blank_values() {
        let fallback = codex_prompt_cache_key("credential-3");

        let missing = br#"{"model":"gpt-5","stream":true}"#;
        let (normalized_missing, key_missing) =
            normalize_prompt_cache_key(missing, &fallback).expect("missing key should normalize");
        assert_eq!(key_missing, fallback);
        let missing_json: Value = serde_json::from_slice(&normalized_missing)
            .expect("normalized body should remain valid JSON");
        assert_eq!(missing_json["prompt_cache_key"], fallback);

        let blank = br#"{"model":"gpt-5","prompt_cache_key":""}"#;
        let (normalized_blank, key_blank) =
            normalize_prompt_cache_key(blank, &fallback).expect("blank key should normalize");
        assert_eq!(key_blank, fallback);
        let blank_json: Value = serde_json::from_slice(&normalized_blank)
            .expect("normalized body should remain valid JSON");
        assert_eq!(blank_json["prompt_cache_key"], fallback);
    }

    #[test]
    fn normalize_prompt_cache_key_for_request_reencodes_zstd_bodies() {
        let fallback = codex_prompt_cache_key("credential-5");
        let mut headers = HeaderMap::new();
        headers.insert(
            reqwest::header::CONTENT_ENCODING,
            http::HeaderValue::from_static("zstd"),
        );
        let original = br#"{"model":"gpt-5","stream":true}"#;
        let compressed = zstd::stream::encode_all(std::io::Cursor::new(original.as_slice()), 3)
            .expect("zstd compression should succeed");

        let (normalized, key) = normalize_prompt_cache_key_for_request(
            &headers,
            Bytes::from(compressed.clone()),
            &fallback,
        )
        .expect("zstd body should normalize");

        assert_eq!(key, fallback);
        assert_ne!(normalized.as_ref(), compressed.as_slice());

        let decoded = zstd::stream::decode_all(std::io::Cursor::new(normalized.as_ref()))
            .expect("normalized zstd body should decode");
        let normalized_json: Value =
            serde_json::from_slice(&decoded).expect("normalized body should remain valid JSON");
        assert_eq!(normalized_json["prompt_cache_key"], fallback);
    }

    #[test]
    fn normalize_prompt_cache_key_preserves_existing_value_and_headers_follow_it() {
        let fallback = codex_prompt_cache_key("credential-4");
        let existing = br#"{"model":"gpt-5","prompt_cache_key":"custom-cache-key"}"#;

        let (normalized, key) =
            normalize_prompt_cache_key(existing, &fallback).expect("existing key should normalize");
        assert_eq!(normalized, existing);
        assert_eq!(key, "custom-cache-key");

        let mut headers = HeaderMap::new();
        headers.insert(
            "x-client-request-id",
            http::HeaderValue::from_static("old-request-id"),
        );
        headers.insert(
            "session_id",
            http::HeaderValue::from_static("old-session-id"),
        );
        set_prompt_cache_headers(&mut headers, &key);

        assert_eq!(
            headers
                .get("x-client-request-id")
                .and_then(|value| value.to_str().ok()),
            Some("custom-cache-key")
        );
        assert_eq!(
            headers
                .get("session_id")
                .and_then(|value| value.to_str().ok()),
            Some("custom-cache-key")
        );
    }

    #[test]
    fn session_key_resolution_prefers_client_request_id_then_session_aliases() {
        let mut headers = HeaderMap::new();
        headers.insert("session_id", http::HeaderValue::from_static("session-only"));
        assert_eq!(
            resolve_session_key_from_headers(&headers).as_deref(),
            Some("session-only")
        );

        headers.insert(
            "x-client-request-id",
            http::HeaderValue::from_static("client-priority"),
        );
        assert_eq!(
            resolve_session_key_from_headers(&headers).as_deref(),
            Some("client-priority")
        );

        headers.remove("x-client-request-id");
        headers.remove("session_id");
        headers.insert(
            "conversation_id",
            http::HeaderValue::from_static("conversation-fallback"),
        );
        assert_eq!(
            resolve_session_key_from_headers(&headers).as_deref(),
            Some("conversation-fallback")
        );
    }

    #[test]
    fn ws_handshake_header_copy_preserves_codex_metadata_headers() {
        let mut upstream = HeaderMap::new();
        upstream.insert(
            "x-codex-turn-state",
            http::HeaderValue::from_static("turn-state-1"),
        );
        upstream.insert("x-models-etag", http::HeaderValue::from_static("etag-1"));
        upstream.insert(
            "x-reasoning-included",
            http::HeaderValue::from_static("true"),
        );
        upstream.insert("openai-model", http::HeaderValue::from_static("o4-mini"));
        upstream.insert(
            "sec-websocket-protocol",
            http::HeaderValue::from_static("realtime"),
        );
        upstream.insert(
            "sec-websocket-extensions",
            http::HeaderValue::from_static("permessage-deflate"),
        );

        let mut downstream = HeaderMap::new();
        copy_upstream_ws_handshake_headers(&mut downstream, &upstream);

        assert_eq!(
            downstream
                .get("x-codex-turn-state")
                .and_then(|value| value.to_str().ok()),
            Some("turn-state-1")
        );
        assert_eq!(
            downstream
                .get("x-models-etag")
                .and_then(|value| value.to_str().ok()),
            Some("etag-1")
        );
        assert_eq!(
            downstream
                .get("x-reasoning-included")
                .and_then(|value| value.to_str().ok()),
            Some("true")
        );
        assert_eq!(
            downstream
                .get("openai-model")
                .and_then(|value| value.to_str().ok()),
            Some("o4-mini")
        );
        assert_eq!(
            downstream
                .get("sec-websocket-protocol")
                .and_then(|value| value.to_str().ok()),
            Some("realtime")
        );
        assert!(!downstream.contains_key("sec-websocket-extensions"));
    }

    #[test]
    fn websocket_close_frame_mapping_preserves_code_and_reason() {
        let client_close = axum::extract::ws::CloseFrame {
            code: 4001,
            reason: "proxy-transparent".into(),
        };

        let upstream_close = map_client_close_frame(client_close.clone());
        assert_eq!(u16::from(upstream_close.code), 4001);
        assert_eq!(upstream_close.reason.as_str(), "proxy-transparent");

        let roundtrip_close = map_upstream_close_frame(upstream_close);
        assert_eq!(roundtrip_close.code, 4001);
        assert_eq!(roundtrip_close.reason.as_str(), "proxy-transparent");
    }

    #[test]
    fn upstream_ws_http_failure_extracts_status_headers_and_body() {
        let response = http::Response::builder()
            .status(StatusCode::UPGRADE_REQUIRED)
            .header("x-models-etag", "etag-2")
            .body(Some(br#"{"error":"upgrade_required"}"#.to_vec()))
            .expect("response should build");

        let failure = upstream_ws_http_failure(response);

        assert_eq!(failure.status, StatusCode::UPGRADE_REQUIRED);
        assert_eq!(
            failure
                .headers
                .get("x-models-etag")
                .and_then(|value| value.to_str().ok()),
            Some("etag-2")
        );
        assert_eq!(failure.body, br#"{"error":"upgrade_required"}"#);
    }

    #[test]
    fn websocket_metadata_headers_ignore_invalid_entries() {
        let header_map = header_map_from_json(serde_json::json!({
            "x-codex-turn-state": "turn-2",
            "bad header": "nope",
            "x-bool": true,
            "x-array": ["ignored"]
        }));

        assert_eq!(
            header_map
                .get("x-codex-turn-state")
                .and_then(|value| value.to_str().ok()),
            Some("turn-2")
        );
        assert_eq!(
            header_map
                .get("x-bool")
                .and_then(|value| value.to_str().ok()),
            Some("true")
        );
        assert!(!header_map.contains_key("bad header"));
        assert!(!header_map.contains_key("x-array"));
    }

    #[test]
    fn websocket_requests_receive_missing_codex_default_headers() {
        let mut headers = HeaderMap::new();

        insert_missing_default_http_headers(&mut headers);

        let defaults = codex_default_headers();
        for (name, value) in &defaults {
            assert_eq!(headers.get(name), Some(value));
        }
    }

    #[test]
    fn responses_request_compression_matches_codex_openai_chatgpt_behavior() {
        let auth = CodexAuth::create_dummy_chatgpt_auth_for_testing();

        assert!(should_use_upstream_request_compression(
            "https://chatgpt.com/backend-api/codex",
            &auth,
            "/responses",
            &HeaderMap::new(),
            &Bytes::from_static(br#"{"model":"gpt-5"}"#),
        ));
        assert!(!should_use_upstream_request_compression(
            "https://chatgpt.com/backend-api/codex",
            &auth,
            "/models",
            &HeaderMap::new(),
            &Bytes::from_static(br#"{"model":"gpt-5"}"#),
        ));
    }

    #[test]
    fn upstream_request_body_is_zstd_encoded_for_openai_responses() {
        let mut headers = reqwest::header::HeaderMap::new();
        let auth = CodexAuth::create_dummy_chatgpt_auth_for_testing();
        let original = Bytes::from_static(br#"{"model":"gpt-5","stream":true}"#);

        let compressed = maybe_compress_upstream_request_body(
            &mut headers,
            "https://chatgpt.com/backend-api/codex",
            &auth,
            "/responses",
            &HeaderMap::new(),
            original.clone(),
        )
        .expect("compression should succeed");

        assert_ne!(compressed, original);
        assert_eq!(
            headers
                .get(reqwest::header::CONTENT_ENCODING)
                .and_then(|value| value.to_str().ok()),
            Some("zstd")
        );
        let decoded = zstd::stream::decode_all(std::io::Cursor::new(compressed.as_ref()))
            .expect("zstd payload should decode");
        assert_eq!(decoded, original.as_ref());
    }

    #[test]
    fn public_base_url_prefers_configured_origin() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "origin",
            http::HeaderValue::from_static("https://ui.internal.example"),
        );
        headers.insert(
            "host",
            http::HeaderValue::from_static("proxy.internal.example"),
        );

        let config = crate::config::AppConfig {
            bind: "127.0.0.1:8787".parse().expect("bind should parse"),
            data_dir: PathBuf::from("/tmp/codex-proxy"),
            database_url: "sqlite::memory:".to_string(),
            admin_password_hash: "hash".to_string(),
            chatgpt_base_url: "https://chatgpt.com/backend-api/codex".to_string(),
            auth_issuer: "https://auth.openai.com".to_string(),
            auth_client_id: "client".to_string(),
            public_base_url: Some("https://public.example/app".to_string()),
            forced_chatgpt_workspace_id: None,
        };

        assert_eq!(
            public_base_url(&headers, &config).expect("public base url should resolve"),
            "https://public.example"
        );
    }

    #[test]
    fn upstream_websocket_url_rewrites_only_scheme() {
        assert_eq!(
            upstream_websocket_url("https://chatgpt.com/backend-api/codex/responses?foo=bar")
                .expect("websocket url should build"),
            "wss://chatgpt.com/backend-api/codex/responses?foo=bar"
        );
        assert_eq!(
            upstream_websocket_url("http://localhost:8787/responses")
                .expect("websocket url should build"),
            "ws://localhost:8787/responses"
        );
    }
}
