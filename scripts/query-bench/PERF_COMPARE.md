# Linear GraphQL Query — Performance Comparison

Benchmark comparing four implementations of the Linear GraphQL query tool (`query.ts`), which POSTs GraphQL queries to `https://api.linear.app/graphql` and returns JSON.

## Environment

- **OS**: Linux 6.8.0-94-generic (x86_64)
- **Date**: 2026-03-16
- **Iterations**: 5 per test
- **Auth**: OAuth agent token (`LINEAR_AGENT_TOKEN`)
- **Rust**: 1.94.0 (`reqwest` + `tokio`, release build)
- **Go**: 1.26.1 (stdlib `net/http` only, no external deps)
- **Zig**: 0.16.0-dev.2923 (self-built from source, `std.http.Client`)
- **TypeScript**: Node.js + `npx tsx` + `@linear/sdk`

## Results

### Cold Start (startup + exit with usage error, no network)

| Language | avg | min | max |
|---|---|---|---|
| Zig | **2ms** | 2ms | 2ms |
| Go | 3ms | 3ms | 3ms |
| Rust | 4ms | 3ms | 5ms |
| TypeScript | 1029ms | 968ms | 1150ms |

### Live API Query: `query { viewer { id name } }`

| Language | avg | min | max |
|---|---|---|---|
| Rust | **114ms** | 87ms | 161ms |
| Go | 116ms | 95ms | 129ms |
| Zig | 339ms | 316ms | 404ms |
| TypeScript | 1090ms | 983ms | 1269ms |

### Live API Query: `viewer.assignedIssues (first:10)`

| Language | avg | min | max |
|---|---|---|---|
| Go | **124ms** | 117ms | 144ms |
| Rust | 125ms | 88ms | 218ms |
| Zig | 487ms | 321ms | 1114ms |
| TypeScript | 1082ms | 999ms | 1223ms |

## Analysis

### TypeScript (`npx tsx`)

The dominant cost is **Node.js startup + TypeScript JIT compilation** via `npx tsx`, adding ~1 second to every invocation regardless of query complexity. The actual API round-trip is only ~90-120ms (visible when subtracting cold-start overhead from live query times).

### Rust

Consistently fast across all benchmarks. The `reqwest` + `rustls` HTTP stack is mature and well-optimised. Smallest binary at **4.8MB** (release). Best min-latency on live queries (87-88ms).

### Go

Essentially tied with Rust on live queries, with slightly more consistent results (lower variance). Uses only the standard library — no external dependencies. Simple build toolchain (`go build`). Binary size is moderate.

### Zig (0.16.0-dev)

Fastest cold-start at **2ms** thanks to minimal runtime overhead. However, the HTTP/TLS client in this pre-release 0.16-dev build adds significant overhead (~200-350ms) compared to Rust and Go. The `std.Io` subsystem was recently rewritten and is not yet optimised. Largest binary at **25MB** (ReleaseFast with libc). Expected to improve significantly once Zig 0.16 stabilises.

## Recommendation

**Rust or Go** are both strong candidates to replace `npx tsx query.ts` in production:

- Both save **~1 second per invocation** (primarily by eliminating Node.js startup)
- For a typical lingbot workflow making 4-6 queries, that's **4-6 seconds saved per run**
- Rust has a slight edge in raw performance; Go has simpler builds and no external deps
- Zig is promising but not ready for this use case until 0.16 stabilises

## Reproducing

```bash
# Build all implementations
cd linear-claude-skill/scripts/query-bench
PATH="/home/qtame/.cargo/bin:/usr/local/go/bin:/usr/local/bin:$PATH"

(cd rust && cargo build --release)
(cd go && go build -o query .)
(cd zig && zig build -Doptimize=ReleaseFast)

# Run benchmark
LINEAR_API_KEY=lin_api_xxx ./benchmark.sh 10

# Run unit tests
(cd rust && cargo test)
(cd go && go test -v ./...)
(cd zig && zig build test)

# Run contract tests (all implementations must behave identically)
cd tests
./test_contract.sh "npx tsx ../../query.ts" "TypeScript"
./test_contract.sh "../rust/target/release/query" "Rust"
./test_contract.sh "../go/query" "Go"
./test_contract.sh "../zig/zig-out/bin/query" "Zig"
```
