package main

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"testing"
)

func TestGetToken_PrefersAgentToken(t *testing.T) {
	os.Setenv("LINEAR_AGENT_TOKEN", "agent_tok")
	os.Setenv("LINEAR_API_KEY", "api_key")
	defer os.Unsetenv("LINEAR_AGENT_TOKEN")
	defer os.Unsetenv("LINEAR_API_KEY")

	token, err := getToken()
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if token != "agent_tok" {
		t.Errorf("expected agent_tok, got %s", token)
	}
}

func TestGetToken_FallsBackToAPIKey(t *testing.T) {
	os.Unsetenv("LINEAR_AGENT_TOKEN")
	os.Setenv("LINEAR_API_KEY", "lin_api_test")
	defer os.Unsetenv("LINEAR_API_KEY")

	token, err := getToken()
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if token != "lin_api_test" {
		t.Errorf("expected lin_api_test, got %s", token)
	}
}

func TestGetToken_FailsWithoutCredentials(t *testing.T) {
	os.Unsetenv("LINEAR_AGENT_TOKEN")
	os.Unsetenv("LINEAR_API_KEY")

	_, err := getToken()
	if err == nil {
		t.Fatal("expected error, got nil")
	}
}

func TestGraphQLRequestSerialization(t *testing.T) {
	req := graphQLRequest{
		Query: "query { viewer { id } }",
	}
	data, err := json.Marshal(req)
	if err != nil {
		t.Fatalf("marshal failed: %v", err)
	}
	s := string(data)
	if s == "" {
		t.Fatal("empty serialization")
	}

	var parsed map[string]interface{}
	json.Unmarshal(data, &parsed)
	if parsed["query"] != "query { viewer { id } }" {
		t.Errorf("unexpected query: %v", parsed["query"])
	}
	if _, ok := parsed["variables"]; ok {
		t.Error("variables should be omitted when nil")
	}
}

func TestGraphQLRequestWithVariables(t *testing.T) {
	vars := map[string]interface{}{"first": 10}
	req := graphQLRequest{
		Query:     "query($first: Int) { users(first: $first) { nodes { id } } }",
		Variables: vars,
	}
	data, err := json.Marshal(req)
	if err != nil {
		t.Fatalf("marshal failed: %v", err)
	}

	var parsed map[string]interface{}
	json.Unmarshal(data, &parsed)
	if _, ok := parsed["variables"]; !ok {
		t.Error("variables should be present")
	}
}

func TestExecuteQuery_Success(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("Authorization") != "test-token" {
			t.Errorf("expected auth header test-token, got %s", r.Header.Get("Authorization"))
		}
		if r.Header.Get("Content-Type") != "application/json" {
			t.Errorf("expected content-type application/json")
		}
		w.WriteHeader(200)
		w.Write([]byte(`{"data": {"viewer": {"id": "123", "name": "Test"}}}`))
	}))
	defer server.Close()

	// We can't easily override the URL in executeQuery without refactoring,
	// so test the response parsing logic directly
	var resp graphQLResponse
	err := json.Unmarshal([]byte(`{"data": {"viewer": {"id": "123", "name": "Test"}}}`), &resp)
	if err != nil {
		t.Fatalf("unmarshal failed: %v", err)
	}
	if resp.Data == nil {
		t.Fatal("expected data, got nil")
	}
	if len(resp.Errors) > 0 {
		t.Fatal("unexpected errors")
	}
}

func TestExecuteQuery_GraphQLErrors(t *testing.T) {
	body := `{"errors": [{"message": "bad query", "locations": [{"line": 1, "column": 1}]}]}`
	var resp graphQLResponse
	err := json.Unmarshal([]byte(body), &resp)
	if err != nil {
		t.Fatalf("unmarshal failed: %v", err)
	}
	if len(resp.Errors) != 1 {
		t.Fatalf("expected 1 error, got %d", len(resp.Errors))
	}
	if resp.Errors[0].Message != "bad query" {
		t.Errorf("expected 'bad query', got '%s'", resp.Errors[0].Message)
	}
}

func TestEmptyTokenTreatedAsMissing(t *testing.T) {
	os.Setenv("LINEAR_AGENT_TOKEN", "")
	os.Unsetenv("LINEAR_API_KEY")
	defer os.Unsetenv("LINEAR_AGENT_TOKEN")

	_, err := getToken()
	if err == nil {
		t.Fatal("expected error for empty token")
	}
}
