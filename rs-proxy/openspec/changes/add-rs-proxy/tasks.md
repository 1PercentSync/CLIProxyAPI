## 1. Project Setup

- [ ] 1.1 Create `Cargo.toml` with dependencies:
  - tokio 1.48.0 (rt-multi-thread, macros, net, io-util, sync)
  - axum 0.8.8
  - reqwest 0.13.1 (json, stream)
  - argh 0.1.13
  - serde 1.0.228 (derive)
  - serde_json 1.0.148
  - tower-http 0.6.8 (cors, trace)
  - futures 0.3.31
  - tokio-stream 0.1.17
  - regex 1.12.2
  - tracing, tracing-subscriber
  - thiserror 2.0.17
- [ ] 1.2 Create `Cargo.toml` [build-dependencies]:
  - reqwest 0.13.1 (blocking)
- [ ] 1.3 Create `build.rs` for compile-time data fetching from CLIProxyAPI

## 2. Core Infrastructure

- [ ] 2.1 Implement CLI argument parsing with argh (main.rs, config.rs)
  - Use `#[argh(option, default = "...")]` for defaults
  - `-b/--base-url` (default: "cpa.1percentsync.games")
  - `-p/--port` (default: 6356)
- [ ] 2.2 Define custom error types with thiserror (error.rs)
  - Wrap reqwest::Error, serde_json::Error, std::io::Error
  - Use `#[from]` for automatic conversion
- [ ] 2.3 Implement HTTP client wrapper (proxy/client.rs)
  - Connection pooling
  - Header forwarding
- [ ] 2.4 Implement SSE stream handling (proxy/streaming.rs)
  - Forward upstream bytes to downstream

## 3. Thinking Configuration

- [ ] 3.1 Implement model suffix parser (thinking/parser.rs)
  - Parse `model(value)` pattern
  - Detect numeric vs string value
- [ ] 3.2 Implement effort-to-budget mapping (thinking/models.rs)
  - none→0, auto→-1, minimal→512, low→1024, medium→8192, high→24576, xhigh→32768
- [ ] 3.3 Implement thinking injector (thinking/injector.rs)
  - Protocol-specific injection logic

## 4. Protocol Handlers

- [ ] 4.1 Implement OpenAI handler (protocol/openai.rs)
  - `/v1/chat/completions`, `/v1/responses`
  - Set `reasoning_effort` field (level only, not numeric)
- [ ] 4.2 Implement Anthropic handler (protocol/anthropic.rs)
  - `/v1/messages`
  - Set `thinking.type` + `thinking.budget_tokens`
- [ ] 4.3 Implement Gemini handler (protocol/gemini.rs)
  - `/v1beta/models/*`
  - Set `thinkingBudget` (not `thinkingLevel`)

## 5. Model List Enhancement

- [ ] 5.1 Implement protocol detection from headers (for /v1/models only)
  - `x-api-key` → Anthropic
  - `Authorization: Bearer` → OpenAI
- [ ] 5.2 Implement model list enhancer (models/enhancer.rs)
  - Add thinking level variants for supported models

## 6. Request Routing

- [ ] 6.1 Set up axum router with all endpoints
- [ ] 6.2 Add CORS and tracing middleware

## 7. Build-time Data Generation

- [ ] 7.1 Fetch CLIProxyAPI source files in build.rs using reqwest::blocking
- [ ] 7.2 Parse Go source to extract model support data
- [ ] 7.3 Generate Rust code for model registry
- [ ] 7.4 Use std::sync::LazyLock for static model registry (not once_cell)

## 8. Testing & Polish

- [ ] 8.1 Add structured logging with tracing
- [ ] 8.2 Handle error cases gracefully
- [ ] 8.3 Test with real API calls
