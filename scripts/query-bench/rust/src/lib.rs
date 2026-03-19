use dotenvy;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::env;
use std::fs;
use std::path::PathBuf;

pub const LINEAR_API_URL: &str = "https://api.linear.app/graphql";
const LINEAR_TOKEN_URL: &str = "https://api.linear.app/oauth/token";

#[derive(Serialize)]
pub struct GraphQLRequest {
    pub query: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variables: Option<Value>,
}

#[derive(Deserialize)]
pub struct GraphQLResponse {
    pub data: Option<Value>,
    pub errors: Option<Vec<GraphQLError>>,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct GraphQLError {
    pub message: String,
    #[serde(default)]
    pub locations: Vec<ErrorLocation>,
    #[serde(default)]
    pub path: Vec<Value>,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct ErrorLocation {
    pub line: u32,
    pub column: u32,
}

/// Response from Linear's OAuth token refresh endpoint.
#[derive(Deserialize, Debug)]
pub struct OAuthTokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub token_type: Option<String>,
    #[serde(default)]
    pub expires_in: Option<u64>,
    #[serde(default)]
    pub scope: Option<String>,
}

/// Distinguishes recoverable 401 errors from other failures.
pub enum QueryError {
    Unauthorized(String),
    Other(String),
}

pub fn get_token() -> Result<String, String> {
    if let Ok(token) = env::var("LINEAR_AGENT_TOKEN") {
        if !token.is_empty() {
            return Ok(token);
        }
    }
    if let Ok(key) = env::var("LINEAR_API_KEY") {
        if !key.is_empty() {
            return Ok(key);
        }
    }
    Err(
        "No Linear credentials found. Set LINEAR_AGENT_TOKEN (preferred) or LINEAR_API_KEY."
            .to_string(),
    )
}

pub async fn execute_query(
    client: &Client,
    token: &str,
    query: &str,
    variables: Option<Value>,
) -> Result<Value, QueryError> {
    let request_body = GraphQLRequest {
        query: query.to_string(),
        variables,
    };

    let response = client
        .post(LINEAR_API_URL)
        .header("Authorization", token)
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await
        .map_err(|e| QueryError::Other(format!("Network error: {}", e)))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| QueryError::Other(format!("Failed to read response: {}", e)))?;

    if status.as_u16() == 401 {
        return Err(QueryError::Unauthorized(format!(
            "HTTP {} Unauthorized: {}",
            status, body
        )));
    }

    if !status.is_success() {
        return Err(QueryError::Other(format!("HTTP {}: {}", status, body)));
    }

    let gql_response: GraphQLResponse = serde_json::from_str(&body)
        .map_err(|e| QueryError::Other(format!("Failed to parse response: {}", e)))?;

    if let Some(ref errors) = gql_response.errors {
        let is_auth_error = errors.iter().any(|e| {
            e.message.to_lowercase().contains("authentication required")
                || e.message.to_lowercase().contains("not authenticated")
        });
        if is_auth_error {
            let error_json = serde_json::to_string_pretty(errors)
                .unwrap_or_else(|_| format!("{:?}", errors));
            return Err(QueryError::Unauthorized(format!(
                "GraphQL auth error:\n{}",
                error_json
            )));
        }
    }

    if let Some(errors) = gql_response.errors {
        let error_json = serde_json::to_string_pretty(&errors)
            .unwrap_or_else(|_| format!("{:?}", errors));
        return Err(QueryError::Other(format!(
            "GraphQL Errors:\n{}",
            error_json
        )));
    }

    gql_response
        .data
        .ok_or_else(|| QueryError::Other("No data in response".to_string()))
}

/// Refresh the OAuth token using the refresh_token grant.
pub async fn refresh_oauth_token(client: &Client) -> Result<OAuthTokenResponse, String> {
    let refresh_token = env::var("LINEAR_REFRESH_TOKEN")
        .map_err(|_| "LINEAR_REFRESH_TOKEN not set — cannot refresh expired token".to_string())?;
    let client_id = env::var("LINEAR_OAUTH_CLIENT_ID")
        .map_err(|_| "LINEAR_OAUTH_CLIENT_ID not set — cannot refresh expired token".to_string())?;
    let client_secret = env::var("LINEAR_OAUTH_CLIENT_SECRET")
        .map_err(|_| {
            "LINEAR_OAUTH_CLIENT_SECRET not set — cannot refresh expired token".to_string()
        })?;

    if refresh_token.is_empty() || client_id.is_empty() || client_secret.is_empty() {
        return Err("OAuth refresh credentials are empty".to_string());
    }

    eprintln!("[INFO] Token expired — refreshing via OAuth...");

    let params = [
        ("grant_type", "refresh_token"),
        ("refresh_token", &refresh_token),
        ("client_id", &client_id),
        ("client_secret", &client_secret),
    ];

    let response = client
        .post(LINEAR_TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&params)
        .send()
        .await
        .map_err(|e| format!("Token refresh network error: {}", e))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| format!("Failed to read token response: {}", e))?;

    if !status.is_success() {
        return Err(format!(
            "Token refresh failed (HTTP {}): {}",
            status, body
        ));
    }

    let token_resp: OAuthTokenResponse = serde_json::from_str(&body)
        .map_err(|e| format!("Failed to parse token response: {} — body: {}", e, body))?;

    eprintln!(
        "[INFO] Token refreshed successfully (expires_in: {}s)",
        token_resp.expires_in.unwrap_or(0)
    );

    Ok(token_resp)
}

/// Load variables from the .env file, overriding any existing shell env vars.
pub fn load_env_file() {
    if let Some(env_path) = find_env_file() {
        match dotenvy::from_path_override(&env_path) {
            Ok(_) => {}
            Err(e) => {
                eprintln!("[WARN] Failed to load {}: {}", env_path.display(), e);
            }
        }
    }
}

/// Find the .env file by walking up from the binary or from CWD.
pub fn find_env_file() -> Option<PathBuf> {
    if let Ok(exe) = env::current_exe() {
        let mut dir = exe.parent().map(|p| p.to_path_buf());
        for _ in 0..8 {
            if let Some(ref d) = dir {
                let candidate = d.join(".env");
                if candidate.exists() {
                    return Some(candidate);
                }
                dir = d.parent().map(|p| p.to_path_buf());
            } else {
                break;
            }
        }
    }
    let cwd_env = PathBuf::from(".env");
    if cwd_env.exists() {
        return Some(cwd_env);
    }
    None
}

/// Update the .env file with new token values.
pub fn update_env_file(new_access_token: &str, new_refresh_token: Option<&str>) {
    let env_path = match find_env_file() {
        Some(p) => p,
        None => {
            eprintln!("[WARN] Could not find .env file to persist refreshed token");
            return;
        }
    };

    let content = match fs::read_to_string(&env_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[WARN] Failed to read {}: {}", env_path.display(), e);
            return;
        }
    };

    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    let mut found_access = false;
    let mut found_refresh = false;

    for line in lines.iter_mut() {
        if line.starts_with("LINEAR_AGENT_TOKEN=") {
            *line = format!("LINEAR_AGENT_TOKEN={}", new_access_token);
            found_access = true;
        }
        if let Some(rt) = new_refresh_token {
            if line.starts_with("LINEAR_REFRESH_TOKEN=") {
                *line = format!("LINEAR_REFRESH_TOKEN={}", rt);
                found_refresh = true;
            }
        }
    }

    if !found_access {
        lines.push(format!("LINEAR_AGENT_TOKEN={}", new_access_token));
    }
    if let Some(rt) = new_refresh_token {
        if !found_refresh {
            lines.push(format!("LINEAR_REFRESH_TOKEN={}", rt));
        }
    }

    let new_content = lines.join("\n") + "\n";
    if let Err(e) = fs::write(&env_path, new_content) {
        eprintln!("[WARN] Failed to write {}: {}", env_path.display(), e);
    } else {
        eprintln!("[INFO] Updated {}", env_path.display());
    }
}

/// Execute a query with automatic token refresh on 401.
pub async fn execute_with_refresh(
    client: &Client,
    token: &str,
    query: &str,
    variables: Option<Value>,
) -> Result<Value, String> {
    match execute_query(client, token, query, variables.clone()).await {
        Ok(data) => Ok(data),
        Err(QueryError::Unauthorized(_)) => {
            let token_resp = refresh_oauth_token(client).await?;
            let new_token = &token_resp.access_token;
            env::set_var("LINEAR_AGENT_TOKEN", new_token);
            update_env_file(new_token, token_resp.refresh_token.as_deref());

            match execute_query(client, new_token, query, variables).await {
                Ok(data) => Ok(data),
                Err(QueryError::Unauthorized(e)) => {
                    Err(format!("Auth failed after token refresh: {}", e))
                }
                Err(QueryError::Other(e)) => Err(e),
            }
        }
        Err(QueryError::Other(e)) => Err(e),
    }
}
