use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::env;
use std::process;

const LINEAR_API_URL: &str = "https://api.linear.app/graphql";

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

async fn execute_query(token: &str, query: &str, variables: Option<Value>) -> Result<Value, String> {
    let client = Client::new();

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
        .map_err(|e| format!("Network error: {}", e))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| format!("Failed to read response: {}", e))?;

    if !status.is_success() {
        return Err(format!("HTTP {}: {}", status, body));
    }

    let gql_response: GraphQLResponse =
        serde_json::from_str(&body).map_err(|e| format!("Failed to parse response: {}", e))?;

    if let Some(errors) = gql_response.errors {
        let error_json = serde_json::to_string_pretty(&errors)
            .unwrap_or_else(|_| format!("{:?}", errors));
        return Err(format!(
            "GraphQL Errors:\n{}",
            error_json
        ));
    }

    gql_response
        .data
        .ok_or_else(|| "No data in response".to_string())
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

    match execute_query(&token, &query, variables).await {
        Ok(data) => {
            let output = serde_json::to_string_pretty(&data).unwrap();
            println!("{}", output);
        }
        Err(e) => {
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
}
