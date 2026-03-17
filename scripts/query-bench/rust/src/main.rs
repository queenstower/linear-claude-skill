use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process;

const LINEAR_API_URL: &str = "https://api.linear.app/graphql";
const LINEAR_TOKEN_URL: &str = "https://api.linear.app/oauth/token";

#[derive(Serialize)]
struct GraphQLRequest {
    query: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    variables: Option<Value>,
}

#[derive(Deserialize)]
struct GraphQLResponse {
    data: Option<Value>,
    errors: Option<Vec<GraphQLError>>,
}

#[derive(Deserialize, Serialize, Debug)]
struct GraphQLError {
    message: String,
    #[serde(default)]
    locations: Vec<ErrorLocation>,
    #[serde(default)]
    path: Vec<Value>,
}

#[derive(Deserialize, Serialize, Debug)]
struct ErrorLocation {
    line: u32,
    column: u32,
}

/// Response from Linear's OAuth token refresh endpoint.
#[derive(Deserialize, Debug)]
struct OAuthTokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    scope: Option<String>,
}

/// Distinguishes recoverable 401 errors from other failures.
enum QueryError {
    Unauthorized(String),
    Other(String),
}

fn get_token() -> Result<String, String> {
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

fn parse_args() -> Result<(String, Option<Value>), String> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        return Err(
            "Error: Query argument is required\n\nUsage:\n  query \"query { viewer { id name } }\""
                .to_string(),
        );
    }

    let query = args[1].clone();
    let variables = if args.len() > 2 {
        let parsed: Value =
            serde_json::from_str(&args[2]).map_err(|e| {
                format!("Error: Variables must be valid JSON\nReceived: {}\nParse error: {}", args[2], e)
            })?;
        Some(parsed)
    } else {
        None
    };

    Ok((query, variables))
}

async fn execute_query(
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

    // Linear returns 200 with auth errors in the GraphQL body
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
async fn refresh_oauth_token(client: &Client) -> Result<OAuthTokenResponse, String> {
    let refresh_token = env::var("LINEAR_REFRESH_TOKEN")
        .map_err(|_| "LINEAR_REFRESH_TOKEN not set — cannot refresh expired token".to_string())?;
    let client_id = env::var("LINEAR_OAUTH_CLIENT_ID")
        .map_err(|_| "LINEAR_OAUTH_CLIENT_ID not set — cannot refresh expired token".to_string())?;
    let client_secret = env::var("LINEAR_OAUTH_CLIENT_SECRET")
        .map_err(|_| "LINEAR_OAUTH_CLIENT_SECRET not set — cannot refresh expired token".to_string())?;

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

/// Find the .env file by walking up from the binary or from SCRIPT_DIR.
fn find_env_file() -> Option<PathBuf> {
    // Try relative to the binary's grandparent (rust/target/release/ -> rust/ -> query-bench/ -> scripts/ -> .env)
    if let Ok(exe) = env::current_exe() {
        // Walk up from the binary to find the linear-claude-skill root
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
    // Try from CWD
    let cwd_env = PathBuf::from(".env");
    if cwd_env.exists() {
        return Some(cwd_env);
    }
    None
}

/// Update the .env file with new token values.
fn update_env_file(new_access_token: &str, new_refresh_token: Option<&str>) {
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

#[tokio::main]
async fn main() {
    let (query, variables) = match parse_args() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{}", e);
            process::exit(1);
        }
    };

    let token = match get_token() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{}", e);
            process::exit(1);
        }
    };

    let client = Client::new();

    // First attempt
    match execute_query(&client, &token, &query, variables.clone()).await {
        Ok(data) => {
            let output = serde_json::to_string_pretty(&data).unwrap();
            println!("{}", output);
            return;
        }
        Err(QueryError::Unauthorized(_reason)) => {
            // Try to refresh the token
            match refresh_oauth_token(&client).await {
                Ok(token_resp) => {
                    let new_token = &token_resp.access_token;

                    // Update env var for this process
                    env::set_var("LINEAR_AGENT_TOKEN", new_token);

                    // Persist to .env file
                    update_env_file(
                        new_token,
                        token_resp.refresh_token.as_deref(),
                    );

                    // Retry the query with the new token
                    match execute_query(&client, new_token, &query, variables).await {
                        Ok(data) => {
                            let output = serde_json::to_string_pretty(&data).unwrap();
                            println!("{}", output);
                        }
                        Err(QueryError::Unauthorized(e)) => {
                            eprintln!("Error executing query (after token refresh):\n{}", e);
                            process::exit(1);
                        }
                        Err(QueryError::Other(e)) => {
                            eprintln!("Error executing query:\n{}", e);
                            process::exit(1);
                        }
                    }
                }
                Err(refresh_err) => {
                    eprintln!("Error executing query:\nToken expired and refresh failed: {}", refresh_err);
                    process::exit(1);
                }
            }
        }
        Err(QueryError::Other(e)) => {
            eprintln!("Error executing query:\n{}", e);
            process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_token_prefers_agent_token() {
        env::set_var("LINEAR_AGENT_TOKEN", "agent_tok");
        env::set_var("LINEAR_API_KEY", "api_key");
        let result = get_token().unwrap();
        assert_eq!(result, "agent_tok");
        env::remove_var("LINEAR_AGENT_TOKEN");
        env::remove_var("LINEAR_API_KEY");
    }

    #[test]
    fn test_get_token_falls_back_to_api_key() {
        env::remove_var("LINEAR_AGENT_TOKEN");
        env::set_var("LINEAR_API_KEY", "lin_api_test");
        let result = get_token().unwrap();
        assert_eq!(result, "lin_api_test");
        env::remove_var("LINEAR_API_KEY");
    }

    #[test]
    fn test_get_token_fails_without_credentials() {
        env::remove_var("LINEAR_AGENT_TOKEN");
        env::remove_var("LINEAR_API_KEY");
        let result = get_token();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("LINEAR_AGENT_TOKEN"));
    }

    #[test]
    fn test_graphql_request_serialization() {
        let req = GraphQLRequest {
            query: "query { viewer { id } }".to_string(),
            variables: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("viewer"));
        assert!(!json.contains("variables"));
    }

    #[test]
    fn test_graphql_request_with_variables() {
        let vars = serde_json::json!({"first": 10});
        let req = GraphQLRequest {
            query: "query($first: Int) { users(first: $first) { nodes { id } } }".to_string(),
            variables: Some(vars),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("variables"));
        assert!(json.contains("10"));
    }

    #[test]
    fn test_graphql_response_with_data() {
        let json = r#"{"data": {"viewer": {"id": "123"}}}"#;
        let resp: GraphQLResponse = serde_json::from_str(json).unwrap();
        assert!(resp.data.is_some());
        assert!(resp.errors.is_none());
    }

    #[test]
    fn test_graphql_response_with_errors() {
        let json = r#"{"errors": [{"message": "bad query", "locations": [{"line": 1, "column": 1}]}]}"#;
        let resp: GraphQLResponse = serde_json::from_str(json).unwrap();
        assert!(resp.errors.is_some());
        assert_eq!(resp.errors.unwrap()[0].message, "bad query");
    }

    #[test]
    fn test_empty_token_treated_as_missing() {
        env::set_var("LINEAR_AGENT_TOKEN", "");
        env::remove_var("LINEAR_API_KEY");
        let result = get_token();
        assert!(result.is_err());
        env::remove_var("LINEAR_AGENT_TOKEN");
    }

    #[test]
    fn test_oauth_token_response_parsing() {
        let json = r#"{"access_token":"lin_oauth_new","refresh_token":"lin_refresh_new","token_type":"Bearer","expires_in":315576000,"scope":"read write"}"#;
        let resp: OAuthTokenResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.access_token, "lin_oauth_new");
        assert_eq!(resp.refresh_token.as_deref(), Some("lin_refresh_new"));
        assert_eq!(resp.expires_in, Some(315576000));
    }

    #[test]
    fn test_oauth_token_response_minimal() {
        let json = r#"{"access_token":"lin_oauth_min"}"#;
        let resp: OAuthTokenResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.access_token, "lin_oauth_min");
        assert!(resp.refresh_token.is_none());
    }
}
