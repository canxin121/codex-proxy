use crate::config::AppConfig;
use crate::error::AppError;
use base64::Engine;
use chrono::Utc;
use codex_login::AuthCredentialsStoreMode;
use codex_login::AuthDotJson;
use codex_login::AuthMode;
use codex_login::TokenData;
use codex_login::default_client::originator;
use codex_login::save_auth;
use codex_login::token_data::parse_chatgpt_jwt_claims;
use rand::Rng;
use reqwest::StatusCode;
use sha2::Digest;
use sha2::Sha256;
use std::path::Path;
use url::Url;

pub struct BrowserAuthStart {
    pub oauth_state: String,
    pub pkce_code_verifier: String,
    pub authorization_url: String,
    pub redirect_uri: String,
}

pub struct BrowserAuthCompletion {
    pub oauth_code: String,
}

pub fn start_browser_auth(config: &AppConfig) -> BrowserAuthStart {
    let oauth_state = generate_state();
    let pkce_code_verifier = generate_code_verifier();
    let pkce_code_challenge = code_challenge(&pkce_code_verifier);
    let redirect_uri = config.auth_callback_url.clone();
    let authorization_url = build_authorization_url(
        &config.auth_issuer,
        &config.auth_client_id,
        &redirect_uri,
        &pkce_code_challenge,
        &oauth_state,
        config.forced_chatgpt_workspace_id.as_deref(),
    );

    BrowserAuthStart {
        oauth_state,
        pkce_code_verifier,
        authorization_url,
        redirect_uri,
    }
}

pub fn parse_browser_callback(
    callback_url: &str,
    expected_state: &str,
) -> Result<BrowserAuthCompletion, AppError> {
    let url = Url::parse(callback_url)
        .map_err(|err| AppError::bad_request(format!("invalid callback_url: {err}")))?;
    let query = url.query_pairs().collect::<Vec<_>>();

    let error_code = query
        .iter()
        .find(|(key, _)| key == "error")
        .map(|(_, value)| value.to_string());
    let error_description = query
        .iter()
        .find(|(key, _)| key == "error_description")
        .map(|(_, value)| value.to_string());
    if let Some(error_code) = error_code {
        return Err(AppError::bad_request(callback_error_message(
            &error_code,
            error_description.as_deref(),
        )));
    }

    let callback_state = query
        .iter()
        .find(|(key, _)| key == "state")
        .map(|(_, value)| value.to_string())
        .ok_or_else(|| AppError::bad_request("callback_url is missing state"))?;
    if callback_state != expected_state {
        return Err(AppError::bad_request(
            "callback_url state did not match auth session",
        ));
    }

    let oauth_code = query
        .iter()
        .find(|(key, _)| key == "code")
        .map(|(_, value)| value.to_string())
        .ok_or_else(|| AppError::bad_request("callback_url is missing code"))?;

    Ok(BrowserAuthCompletion { oauth_code })
}

pub async fn complete_browser_auth(
    config: &AppConfig,
    credential_home: &Path,
    redirect_uri: &str,
    pkce_code_verifier: &str,
    oauth_code: &str,
) -> Result<(), AppError> {
    let exchanged = exchange_code_for_tokens(
        &config.auth_issuer,
        &config.auth_client_id,
        redirect_uri,
        pkce_code_verifier,
        oauth_code,
    )
    .await?;

    std::fs::create_dir_all(credential_home).map_err(|err| AppError::internal(err.to_string()))?;

    let id_token = parse_chatgpt_jwt_claims(&exchanged.id_token)
        .map_err(|err| AppError::bad_gateway(err.to_string()))?;
    if let Some(expected_workspace_id) = config.forced_chatgpt_workspace_id.as_deref() {
        let actual_workspace_id = id_token.chatgpt_account_id.as_deref().ok_or_else(|| {
            AppError::forbidden(
                "auth token did not include a chatgpt_account_id claim for workspace validation",
            )
        })?;
        if actual_workspace_id != expected_workspace_id {
            return Err(AppError::forbidden(format!(
                "auth is restricted to workspace id {expected_workspace_id}"
            )));
        }
    }

    let token_data = TokenData {
        account_id: id_token.chatgpt_account_id.clone(),
        id_token,
        access_token: exchanged.access_token,
        refresh_token: exchanged.refresh_token,
    };
    let auth_payload = AuthDotJson {
        auth_mode: Some(AuthMode::Chatgpt),
        openai_api_key: None,
        tokens: Some(token_data),
        last_refresh: Some(Utc::now()),
    };
    save_auth(
        credential_home,
        &auth_payload,
        AuthCredentialsStoreMode::File,
    )
    .map_err(|err| AppError::internal(err.to_string()))
}

fn generate_state() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn generate_code_verifier() -> String {
    let mut bytes = [0u8; 64];
    rand::rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn code_challenge(code_verifier: &str) -> String {
    let digest = Sha256::digest(code_verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

fn build_authorization_url(
    issuer: &str,
    client_id: &str,
    redirect_uri: &str,
    code_challenge: &str,
    oauth_state: &str,
    forced_chatgpt_workspace_id: Option<&str>,
) -> String {
    let mut query = vec![
        ("response_type".to_string(), "code".to_string()),
        ("client_id".to_string(), client_id.to_string()),
        ("redirect_uri".to_string(), redirect_uri.to_string()),
        (
            "scope".to_string(),
            "openid profile email offline_access api.connectors.read api.connectors.invoke"
                .to_string(),
        ),
        ("code_challenge".to_string(), code_challenge.to_string()),
        ("code_challenge_method".to_string(), "S256".to_string()),
        ("id_token_add_organizations".to_string(), "true".to_string()),
        ("codex_cli_simplified_flow".to_string(), "true".to_string()),
        ("state".to_string(), oauth_state.to_string()),
        ("originator".to_string(), originator().value),
    ];
    if let Some(workspace_id) = forced_chatgpt_workspace_id {
        query.push(("allowed_workspace_id".to_string(), workspace_id.to_string()));
    }

    let encoded_query = query
        .into_iter()
        .map(|(key, value)| format!("{key}={}", urlencoding::encode(&value)))
        .collect::<Vec<_>>()
        .join("&");
    format!(
        "{}/oauth/authorize?{}",
        issuer.trim_end_matches('/'),
        encoded_query
    )
}

async fn exchange_code_for_tokens(
    issuer: &str,
    client_id: &str,
    redirect_uri: &str,
    code_verifier: &str,
    oauth_code: &str,
) -> Result<ExchangedTokens, AppError> {
    #[derive(serde::Deserialize)]
    struct TokenResponse {
        id_token: String,
        access_token: String,
        refresh_token: String,
    }

    let client = reqwest::Client::builder()
        .build()
        .map_err(|err| AppError::internal(err.to_string()))?;
    let response = client
        .post(format!("{}/oauth/token", issuer.trim_end_matches('/')))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!(
            "grant_type=authorization_code&code={}&redirect_uri={}&client_id={}&code_verifier={}",
            urlencoding::encode(oauth_code),
            urlencoding::encode(redirect_uri),
            urlencoding::encode(client_id),
            urlencoding::encode(code_verifier),
        ))
        .send()
        .await
        .map_err(|err| AppError::bad_gateway(err.to_string()))?;

    if response.status() != StatusCode::OK {
        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<failed to read error body>".to_string());
        return Err(AppError::bad_gateway(format!(
            "oauth token exchange failed with status {status}: {body}"
        )));
    }

    let token_response = response
        .json::<TokenResponse>()
        .await
        .map_err(|err| AppError::bad_gateway(err.to_string()))?;
    Ok(ExchangedTokens {
        id_token: token_response.id_token,
        access_token: token_response.access_token,
        refresh_token: token_response.refresh_token,
    })
}

fn callback_error_message(error_code: &str, error_description: Option<&str>) -> String {
    if error_code == "access_denied"
        && error_description.is_some_and(|description| {
            description
                .to_ascii_lowercase()
                .contains("missing_codex_entitlement")
        })
    {
        return "Codex is not enabled for your workspace. Contact your workspace administrator to request access to Codex.".to_string();
    }

    if let Some(error_description) = error_description
        && !error_description.trim().is_empty()
    {
        return format!("sign-in failed: {error_description}");
    }

    format!("sign-in failed with oauth error: {error_code}")
}

struct ExchangedTokens {
    id_token: String,
    access_token: String,
    refresh_token: String,
}
