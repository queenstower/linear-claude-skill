package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
)

const linearAPIURL = "https://api.linear.app/graphql"

type graphQLRequest struct {
	Query     string      `json:"query"`
	Variables interface{} `json:"variables,omitempty"`
}

type graphQLError struct {
	Message   string          `json:"message"`
	Locations []errorLocation `json:"locations,omitempty"`
	Path      []interface{}   `json:"path,omitempty"`
}

type errorLocation struct {
	Line   int `json:"line"`
	Column int `json:"column"`
}

type graphQLResponse struct {
	Data   json.RawMessage `json:"data"`
	Errors []graphQLError  `json:"errors,omitempty"`
}

func getToken() (string, error) {
	if token := os.Getenv("LINEAR_AGENT_TOKEN"); token != "" {
		return token, nil
	}
	if key := os.Getenv("LINEAR_API_KEY"); key != "" {
		return key, nil
	}
	return "", fmt.Errorf("No Linear credentials found. Set LINEAR_AGENT_TOKEN (preferred) or LINEAR_API_KEY")
}

func parseArgs() (string, interface{}, error) {
	args := os.Args[1:]

	if len(args) < 1 {
		return "", nil, fmt.Errorf("Error: Query argument is required\n\nUsage:\n  query \"query { viewer { id name } }\"")
	}

	query := args[0]
	var variables interface{}

	if len(args) > 1 {
		if err := json.Unmarshal([]byte(args[1]), &variables); err != nil {
			return "", nil, fmt.Errorf("Error: Variables must be valid JSON\nReceived: %s\nParse error: %v", args[1], err)
		}
	}

	return query, variables, nil
}

func executeQuery(token, query string, variables interface{}) (json.RawMessage, error) {
	reqBody := graphQLRequest{
		Query:     query,
		Variables: variables,
	}

	body, err := json.Marshal(reqBody)
	if err != nil {
		return nil, fmt.Errorf("failed to marshal request: %v", err)
	}

	req, err := http.NewRequest("POST", linearAPIURL, bytes.NewReader(body))
	if err != nil {
		return nil, fmt.Errorf("failed to create request: %v", err)
	}

	req.Header.Set("Authorization", token)
	req.Header.Set("Content-Type", "application/json")

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("network error: %v", err)
	}
	defer resp.Body.Close()

	respBody, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, fmt.Errorf("failed to read response: %v", err)
	}

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return nil, fmt.Errorf("HTTP %d: %s", resp.StatusCode, string(respBody))
	}

	var gqlResp graphQLResponse
	if err := json.Unmarshal(respBody, &gqlResp); err != nil {
		return nil, fmt.Errorf("failed to parse response: %v", err)
	}

	if len(gqlResp.Errors) > 0 {
		errJSON, _ := json.MarshalIndent(gqlResp.Errors, "", "  ")
		return nil, fmt.Errorf("GraphQL Errors:\n%s", string(errJSON))
	}

	if gqlResp.Data == nil {
		return nil, fmt.Errorf("no data in response")
	}

	return gqlResp.Data, nil
}

func run() int {
	query, variables, err := parseArgs()
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		return 1
	}

	token, err := getToken()
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		return 1
	}

	data, err := executeQuery(token, query, variables)
	if err != nil {
		fmt.Fprintf(os.Stderr, "Error executing query:\n%v\n", err)
		return 1
	}

	var pretty bytes.Buffer
	if err := json.Indent(&pretty, data, "", "  "); err != nil {
		fmt.Fprintln(os.Stderr, "Failed to format output:", err)
		return 1
	}

	fmt.Println(pretty.String())
	return 0
}

func main() {
	os.Exit(run())
}
