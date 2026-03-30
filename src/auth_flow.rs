use crate::config::AppConfig;
use crate::error::AppError;
use base64::Engine;
use chrono::Utc;
use codex_client::build_reqwest_client_with_custom_ca;
use codex_login::AuthCredentialsStoreMode;
use codex_login::AuthDotJson;
use codex_login::AuthMode;
use codex_login::TokenData;
use codex_login::default_client::originator;
use codex_login::save_auth;
use codex_login::token_data::parse_chatgpt_jwt_claims;
use rand::Rng;
use serde_json::Value as JsonValue;
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

pub fn start_browser_auth(config: &AppConfig, redirect_uri: String) -> BrowserAuthStart {
    let oauth_state = generate_state();
    let pkce_code_verifier = generate_code_verifier();
    let pkce_code_challenge = code_challenge(&pkce_code_verifier);
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
                "Login is restricted to a specific workspace, but the token did not include a chatgpt_account_id claim.",
            )
        })?;
        if actual_workspace_id != expected_workspace_id {
            return Err(AppError::forbidden(format!(
                "Login is restricted to workspace id {expected_workspace_id}.",
            )));
        }
    }

    // Mirror Codex's current browser-login persistence behavior: if the legacy
    // token-exchange endpoint is available, persist the exchanged API key
    // alongside the ChatGPT tokens. The exchange is best effort upstream.
    let openai_api_key = exchange_id_token_for_api_key(
        &config.auth_issuer,
        &config.auth_client_id,
        &exchanged.id_token,
    )
    .await
    .ok();

    let token_data = TokenData {
        account_id: id_token.chatgpt_account_id.clone(),
        id_token,
        access_token: exchanged.access_token,
        refresh_token: exchanged.refresh_token,
    };
    let auth_payload = AuthDotJson {
        auth_mode: Some(AuthMode::Chatgpt),
        openai_api_key,
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

    let client = build_reqwest_client_with_custom_ca(reqwest::Client::builder())
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
        .map_err(|err| AppError::bad_gateway(redact_sensitive_error_url(err).to_string()))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let detail = parse_token_endpoint_error(&body);
        return Err(AppError::bad_gateway(format!(
            "token endpoint returned status {status}: {detail}"
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

async fn exchange_id_token_for_api_key(
    issuer: &str,
    client_id: &str,
    id_token: &str,
) -> Result<String, AppError> {
    #[derive(serde::Deserialize)]
    struct TokenExchangeResponse {
        access_token: String,
    }

    let client = build_reqwest_client_with_custom_ca(reqwest::Client::builder())
        .map_err(|err| AppError::internal(err.to_string()))?;
    let response = client
        .post(format!("{}/oauth/token", issuer.trim_end_matches('/')))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!(
            "grant_type={}&client_id={}&requested_token={}&subject_token={}&subject_token_type={}",
            urlencoding::encode("urn:ietf:params:oauth:grant-type:token-exchange"),
            urlencoding::encode(client_id),
            urlencoding::encode("openai-api-key"),
            urlencoding::encode(id_token),
            urlencoding::encode("urn:ietf:params:oauth:token-type:id_token"),
        ))
        .send()
        .await
        .map_err(|err| AppError::bad_gateway(err.to_string()))?;

    if !response.status().is_success() {
        return Err(AppError::bad_gateway(format!(
            "oauth api key exchange failed with status {}",
            response.status()
        )));
    }

    let token_response = response
        .json::<TokenExchangeResponse>()
        .await
        .map_err(|err| AppError::bad_gateway(err.to_string()))?;
    Ok(token_response.access_token)
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
        return format!("Sign-in failed: {error_description}");
    }

    format!("Sign-in failed: {error_code}")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TokenEndpointErrorDetail {
    display_message: String,
}

impl std::fmt::Display for TokenEndpointErrorDetail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.display_message.fmt(f)
    }
}

const REDACTED_URL_VALUE: &str = "<redacted>";
const SENSITIVE_URL_QUERY_KEYS: &[&str] = &[
    "access_token",
    "api_key",
    "client_secret",
    "code",
    "code_verifier",
    "id_token",
    "key",
    "refresh_token",
    "requested_token",
    "state",
    "subject_token",
    "token",
];

fn redact_sensitive_query_value(key: &str, value: &str) -> String {
    if SENSITIVE_URL_QUERY_KEYS
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(key))
    {
        REDACTED_URL_VALUE.to_string()
    } else {
        value.to_string()
    }
}

fn redact_sensitive_url_parts(url: &mut Url) {
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_fragment(None);

    let query_pairs = url
        .query_pairs()
        .map(|(key, value)| {
            let key = key.into_owned();
            let value = value.into_owned();
            (key.clone(), redact_sensitive_query_value(&key, &value))
        })
        .collect::<Vec<_>>();

    if query_pairs.is_empty() {
        url.set_query(None);
        return;
    }

    let redacted_query = query_pairs
        .into_iter()
        .fold(
            url::form_urlencoded::Serializer::new(String::new()),
            |mut serializer, (key, value)| {
                serializer.append_pair(&key, &value);
                serializer
            },
        )
        .finish();
    url.set_query(Some(&redacted_query));
}

fn redact_sensitive_error_url(mut err: reqwest::Error) -> reqwest::Error {
    if let Some(url) = err.url_mut() {
        redact_sensitive_url_parts(url);
    }
    err
}

fn parse_token_endpoint_error(body: &str) -> TokenEndpointErrorDetail {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return TokenEndpointErrorDetail {
            display_message: "unknown error".to_string(),
        };
    }

    let parsed = serde_json::from_str::<JsonValue>(trimmed).ok();
    if let Some(json) = parsed {
        if let Some(description) = json.get("error_description").and_then(JsonValue::as_str)
            && !description.trim().is_empty()
        {
            return TokenEndpointErrorDetail {
                display_message: description.to_string(),
            };
        }
        if let Some(error_obj) = json.get("error")
            && let Some(message) = error_obj.get("message").and_then(JsonValue::as_str)
            && !message.trim().is_empty()
        {
            return TokenEndpointErrorDetail {
                display_message: message.to_string(),
            };
        }
        if let Some(error_code) = json.get("error").and_then(JsonValue::as_str)
            && !error_code.trim().is_empty()
        {
            return TokenEndpointErrorDetail {
                display_message: error_code.to_string(),
            };
        }
        if let Some(error_code) = json
            .get("error")
            .and_then(JsonValue::as_object)
            .and_then(|error_obj| error_obj.get("code"))
            .and_then(JsonValue::as_str)
            && !error_code.trim().is_empty()
        {
            return TokenEndpointErrorDetail {
                display_message: error_code.to_string(),
            };
        }
    }

    TokenEndpointErrorDetail {
        display_message: trimmed.to_string(),
    }
}

struct ExchangedTokens {
    id_token: String,
    access_token: String,
    refresh_token: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_token_endpoint_error_prefers_error_description() {
        let detail = parse_token_endpoint_error(
            r#"{"error":"invalid_grant","error_description":"refresh token expired"}"#,
        );

        assert_eq!(detail.to_string(), "refresh token expired");
    }

    #[test]
    fn callback_error_message_matches_codex_wording() {
        assert_eq!(
            callback_error_message("access_denied", Some("missing_codex_entitlement")),
            "Codex is not enabled for your workspace. Contact your workspace administrator to request access to Codex."
        );
        assert_eq!(
            callback_error_message("access_denied", Some("user denied")),
            "Sign-in failed: user denied"
        );
        assert_eq!(
            callback_error_message("invalid_request", None),
            "Sign-in failed: invalid_request"
        );
    }

    #[test]
    fn redact_sensitive_url_parts_hides_oauth_secrets() {
        let mut url = Url::parse(
            "https://user:pass@auth.openai.com/oauth/token?code=abc123&redirect_uri=http%3A%2F%2Flocalhost%3A1455%2Fauth%2Fcallback#frag",
        )
        .expect("url should parse");

        redact_sensitive_url_parts(&mut url);

        assert_eq!(
            url.as_str(),
            "https://auth.openai.com/oauth/token?code=%3Credacted%3E&redirect_uri=http%3A%2F%2Flocalhost%3A1455%2Fauth%2Fcallback"
        );
    }
}
