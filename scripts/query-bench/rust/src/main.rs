use query::{
    execute_query, get_token, load_env_file, refresh_oauth_token, update_env_file, QueryError,
};
use reqwest::Client;
use serde_json::Value;
use std::env;
use std::process;

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
        let parsed: Value = serde_json::from_str(&args[2]).map_err(|e| {
            format!(
                "Error: Variables must be valid JSON\nReceived: {}\nParse error: {}",
                args[2], e
            )
        })?;
        Some(parsed)
    } else {
        None
    };

    Ok((query, variables))
}

#[tokio::main]
async fn main() {
    load_env_file();

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

    match execute_query(&client, &token, &query, variables.clone()).await {
        Ok(data) => {
            let output = serde_json::to_string_pretty(&data).unwrap();
            println!("{}", output);
            return;
        }
        Err(QueryError::Unauthorized(_reason)) => {
            match refresh_oauth_token(&client).await {
                Ok(token_resp) => {
                    let new_token = &token_resp.access_token;
                    env::set_var("LINEAR_AGENT_TOKEN", new_token);
                    update_env_file(new_token, token_resp.refresh_token.as_deref());

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
                    eprintln!(
                        "Error executing query:\nToken expired and refresh failed: {}",
                        refresh_err
                    );
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
    use query::{get_token, GraphQLRequest, GraphQLResponse, OAuthTokenResponse};
    use std::env;

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
        let json =
            r#"{"errors": [{"message": "bad query", "locations": [{"line": 1, "column": 1}]}]}"#;
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
