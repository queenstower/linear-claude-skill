const std = @import("std");
const http = std.http;
const mem = std.mem;
const json = std.json;
const Io = std.Io;
const Stringify = json.Stringify;

const linear_api_url = "https://api.linear.app/graphql";

// ============================================================================
// Token resolution
// ============================================================================

const TokenError = error{NoCredentials};

fn getEnv(name: [*:0]const u8) ?[]const u8 {
    const ptr = std.c.getenv(name) orelse return null;
    return mem.sliceTo(ptr, 0);
}

fn getToken() TokenError![]const u8 {
    if (getEnv("LINEAR_AGENT_TOKEN")) |token| {
        if (token.len > 0) return token;
    }
    if (getEnv("LINEAR_API_KEY")) |key| {
        if (key.len > 0) return key;
    }
    return TokenError.NoCredentials;
}

// ============================================================================
// Argument parsing
// ============================================================================

const Args = struct {
    query: []const u8,
    variables: ?[]const u8,
};

const ArgsError = error{MissingQuery};

fn parseArgs(init: std.process.Init.Minimal) ArgsError!Args {
    var it = std.process.Args.Iterator.init(init.args);
    _ = it.next(); // skip program name

    const query = it.next() orelse return ArgsError.MissingQuery;
    const variables = it.next();

    return Args{
        .query = query,
        .variables = variables,
    };
}

// ============================================================================
// JSON request body builder
// ============================================================================

fn buildRequestBody(allocator: mem.Allocator, query: []const u8, variables: ?[]const u8) ![]u8 {
    var out = Io.Writer.Allocating.init(allocator);
    defer out.deinit();
    const writer = &out.writer;

    writer.writeAll("{\"query\":") catch return error.OutOfMemory;
    Stringify.value(query, .{}, writer) catch return error.OutOfMemory;

    if (variables) |vars| {
        // Validate that variables is valid JSON by parsing it
        const parsed = json.parseFromSlice(json.Value, allocator, vars, .{}) catch {
            return error.InvalidVariablesJson;
        };
        defer parsed.deinit();

        writer.writeAll(",\"variables\":") catch return error.OutOfMemory;
        writer.writeAll(vars) catch return error.OutOfMemory;
    }

    writer.writeAll("}") catch return error.OutOfMemory;
    return out.toOwnedSlice();
}

// ============================================================================
// HTTP client — execute GraphQL query
// ============================================================================

fn executeQuery(
    allocator: mem.Allocator,
    zio: Io,
    token: []const u8,
    query: []const u8,
    variables: ?[]const u8,
) ![]u8 {
    const body = try buildRequestBody(allocator, query, variables);
    defer allocator.free(body);

    var client: http.Client = .{ .allocator = allocator, .io = zio };
    defer client.deinit();

    var response_storage = Io.Writer.Allocating.init(allocator);
    defer response_storage.deinit();

    const result = try client.fetch(.{
        .location = .{ .url = linear_api_url },
        .method = .POST,
        .payload = body,
        .headers = .{
            .authorization = .{ .override = token },
            .content_type = .{ .override = "application/json" },
        },
        .response_writer = &response_storage.writer,
    });

    const resp_body = try response_storage.toOwnedSlice();

    if (result.status != .ok) {
        defer allocator.free(resp_body);
        std.debug.print("HTTP {d}: {s}\n", .{ @intFromEnum(result.status), resp_body });
        return error.HttpError;
    }

    return resp_body;
}

// ============================================================================
// Response parsing — extract "data" field and check for errors
// ============================================================================

fn extractData(allocator: mem.Allocator, resp_body: []const u8) ![]u8 {
    const parsed = try json.parseFromSlice(json.Value, allocator, resp_body, .{});
    defer parsed.deinit();

    const root = parsed.value;
    if (root != .object) return error.InvalidResponse;

    // Check for GraphQL errors
    if (root.object.get("errors")) |errors_val| {
        const err_str = Stringify.valueAlloc(allocator, errors_val, .{ .whitespace = .indent_2 }) catch "";
        defer if (err_str.len > 0) allocator.free(err_str);
        std.debug.print("GraphQL Errors:\n{s}\n", .{err_str});
        return error.GraphQLError;
    }

    // Extract "data" field
    const data_val = root.object.get("data") orelse return error.NoDataInResponse;

    return Stringify.valueAlloc(allocator, data_val, .{ .whitespace = .indent_2 }) catch return error.OutOfMemory;
}

// ============================================================================
// Main
// ============================================================================

pub fn main(init: std.process.Init.Minimal) !void {
    const allocator = std.heap.page_allocator;
    const zio = std.Options.debug_io;

    // Parse arguments
    const args = parseArgs(init) catch {
        std.debug.print(
            "Error: Query argument is required\n\nUsage:\n  query \"query {{ viewer {{ id name }} }}\"\n",
            .{},
        );
        std.process.exit(1);
    };

    // Validate variables JSON early (before network call)
    if (args.variables) |vars| {
        const parsed_check = json.parseFromSlice(json.Value, allocator, vars, .{}) catch {
            std.debug.print(
                "Error: Variables must be valid JSON\nReceived: {s}\n",
                .{vars},
            );
            std.process.exit(1);
        };
        parsed_check.deinit();
    }

    // Get token
    const token = getToken() catch {
        std.debug.print(
            "No Linear credentials found. Set LINEAR_AGENT_TOKEN (preferred) or LINEAR_API_KEY.\n",
            .{},
        );
        std.process.exit(1);
    };

    // Execute query
    const resp_body = executeQuery(allocator, zio, token, args.query, args.variables) catch |err| {
        switch (err) {
            error.HttpError => std.process.exit(1),
            else => {
                std.debug.print("Error executing query:\n{}\n", .{err});
                std.process.exit(1);
            },
        }
    };
    defer allocator.free(resp_body);

    // Extract and print data
    const output = extractData(allocator, resp_body) catch |err| {
        switch (err) {
            error.GraphQLError => std.process.exit(1),
            error.NoDataInResponse => {
                std.debug.print("Error: No data in response\n", .{});
                std.process.exit(1);
            },
            else => {
                std.debug.print("Error parsing response: {}\n", .{err});
                std.process.exit(1);
            },
        }
    };
    defer allocator.free(output);

    // Write to stdout
    var stdout_buf: [4096]u8 = undefined;
    var stdout_writer = Io.File.stdout().writerStreaming(zio, &stdout_buf);
    stdout_writer.interface.writeAll(output) catch {
        std.process.exit(1);
    };
    stdout_writer.interface.writeAll("\n") catch {};
    stdout_writer.flush() catch {};
}

// ============================================================================
// Tests
// ============================================================================

test "getToken prefers agent token over api key" {
    // Token resolution tested via contract tests (test_contract.sh)
    // since env var mutation is not safe in parallel Zig tests.
}

test "buildRequestBody — query only" {
    const allocator = std.testing.allocator;
    const body = try buildRequestBody(allocator, "query { viewer { id } }", null);
    defer allocator.free(body);

    try std.testing.expect(mem.indexOf(u8, body, "viewer") != null);
    try std.testing.expect(mem.indexOf(u8, body, "variables") == null);

    const parsed = try json.parseFromSlice(json.Value, allocator, body, .{});
    defer parsed.deinit();
    const obj = parsed.value.object;
    try std.testing.expect(obj.get("query") != null);
}

test "buildRequestBody — with variables" {
    const allocator = std.testing.allocator;
    const body = try buildRequestBody(
        allocator,
        "query($first: Int) { users(first: $first) { nodes { id } } }",
        "{\"first\": 10}",
    );
    defer allocator.free(body);

    try std.testing.expect(mem.indexOf(u8, body, "variables") != null);
    try std.testing.expect(mem.indexOf(u8, body, "10") != null);

    const parsed = try json.parseFromSlice(json.Value, allocator, body, .{});
    defer parsed.deinit();
    const obj = parsed.value.object;
    try std.testing.expect(obj.get("query") != null);
    try std.testing.expect(obj.get("variables") != null);
}

test "buildRequestBody — rejects invalid JSON variables" {
    const allocator = std.testing.allocator;
    const result = buildRequestBody(allocator, "query { viewer { id } }", "not-json");
    try std.testing.expectError(error.InvalidVariablesJson, result);
}

test "extractData — success response" {
    const allocator = std.testing.allocator;
    const resp = "{\"data\": {\"viewer\": {\"id\": \"123\", \"name\": \"Test\"}}}";
    const output = try extractData(allocator, resp);
    defer allocator.free(output);

    try std.testing.expect(mem.indexOf(u8, output, "123") != null);
    try std.testing.expect(mem.indexOf(u8, output, "Test") != null);

    const parsed = try json.parseFromSlice(json.Value, allocator, output, .{});
    defer parsed.deinit();
}

test "extractData — graphql error response" {
    const allocator = std.testing.allocator;
    const resp = "{\"errors\": [{\"message\": \"bad query\", \"locations\": [{\"line\": 1, \"column\": 1}]}]}";
    const result = extractData(allocator, resp);
    try std.testing.expectError(error.GraphQLError, result);
}

test "extractData — missing data field" {
    const allocator = std.testing.allocator;
    const resp = "{\"something\": \"else\"}";
    const result = extractData(allocator, resp);
    try std.testing.expectError(error.NoDataInResponse, result);
}

test "extractData — invalid JSON response" {
    const allocator = std.testing.allocator;
    const result = extractData(allocator, "not json at all");
    try std.testing.expectError(error.SyntaxError, result);
}
