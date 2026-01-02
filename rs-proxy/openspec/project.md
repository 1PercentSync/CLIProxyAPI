# Project Context

## Purpose

RS-Proxy is a standalone lightweight Rust reverse proxy that can be used independently with any upstream server. It parses thinking configuration from model name suffixes (e.g., `model(high)` or `model(16384)`) and injects the appropriate configuration into API requests, allowing clients to control thinking/reasoning behavior without modifying their request payloads.

**Important clarifications:**
- RS-Proxy is NOT a middleware specifically for CLIProxyAPI - it is a general-purpose proxy that can forward to any upstream API server
- RS-Proxy MUST align its thinking configuration parsing and injection logic with CLIProxyAPI for consistency
- RS-Proxy does NOT provide protocol conversion - it only injects thinking configuration into the original protocol format
- RS-Proxy enhances `/v1/models` response with thinking level variants (unlike CLIProxyAPI which does not include variants)

## Tech Stack

- **Language:** Rust 1.92.0
- **Runtime:** Tokio 1.48.0 (async runtime)
- **HTTP Server:** Axum 0.8.8
- **HTTP Client:** Reqwest 0.13.1
- **CLI:** Argh 0.1.13
- **JSON:** Serde 1.0.228 + Serde_json 1.0.148
- **Middleware:** Tower-http 0.6.8
- **Streams:** Futures 0.3.31 + Tokio-stream 0.1.17
- **Regex:** Regex 1.12.2
- **Errors:** Thiserror 2.0.17
- **Logging:** Tracing + Tracing-subscriber

### Build Dependencies

- **Reqwest** with `blocking` feature for synchronous HTTP in `build.rs`

### Notes on Dependency Choices

- **Argh over Clap:** Lighter weight for simple CLIs (only 2 args), faster compile times
- **Thiserror:** Derive macro for custom error types with `#[from]` for wrapping errors
- **No once_cell:** Using `std::sync::LazyLock` (stable since Rust 1.80) instead

## Project Conventions

### Code Style

- Follow Rust standard formatting (`cargo fmt`)
- Use `clippy` for linting with default rules
- Prefer explicit error handling over `.unwrap()` in production code
- Use `thiserror` for custom error types
- Document public APIs with rustdoc comments

### Architecture Patterns

- **Modular structure:** Separate concerns into `proxy/`, `thinking/`, `protocol/`, `models/` modules
- **Compile-time generation:** Use `build.rs` to fetch and parse CLIProxyAPI source files
- **Transparent proxying:** Minimal request/response modification, only inject thinking config
- **Protocol-specific handlers:** Each API protocol (OpenAI, Anthropic, Gemini) has its own transformation logic

### Testing Strategy

- Unit tests for parsing and transformation logic
- Integration tests with mock upstream server
- Manual testing with real API endpoints

### Git Workflow

- Feature branches off `main`
- Conventional commits: `feat:`, `fix:`, `refactor:`, `docs:`, `test:`
- PR-based merges with review

## Domain Context

### Thinking Configuration

Thinking/reasoning is a feature in modern LLMs that allows extended "thinking" before responding. RS-Proxy injects thinking configuration aligned with CLIProxyAPI's behavior.

**Protocol Detection:**
- URL path determines protocol for most endpoints (e.g., `/v1/messages` → Anthropic, `/v1beta/models/*` → Gemini)
- Request headers (`x-api-key` vs `Authorization: Bearer`) are ONLY used to distinguish protocol for the shared `/v1/models` endpoint

**Injection Rules (aligned with CLIProxyAPI):**
- Only affects models that exist in registry with thinking support
- **Unknown models with thinking suffix:** Return HTTP 400 error (see Design Decisions below)
- **Gemini 2.5:** Sets `generationConfig.thinkingConfig.thinkingBudget` (numeric), auto-sets `include_thoughts=true`
- **Gemini 3:** Sets `generationConfig.thinkingConfig.thinkingLevel` (string: low/medium/high), auto-sets `includeThoughts=true`
- **Claude API:** Sets `thinking.type=enabled` and `thinking.budget_tokens`, sets `max_tokens` to model's `MaxCompletionTokens`
- **OpenAI/Codex/Qwen/iFlow/OpenRouter:** Level/auto/none overrides `reasoning_effort` (chat) or `reasoning.effort` (Responses); numeric budgets are converted to level strings
- Models using discrete levels validate the level; unsupported values return 400
- `(none)` level results in NO thinking configuration being set (not `budget_tokens=0`)

### Effort Level Mapping

| Level    | Budget (tokens) | Description |
|----------|-----------------|-------------|
| none     | 0 (clamped to min if 0 not allowed) | Disable thinking |
| auto     | -1 (dynamic, or clamped if not supported) | Auto-assign by upstream |
| minimal  | 512             | Low-cost reasoning |
| low      | 1024            | Fast reasoning |
| medium   | 8192            | Default reasoning depth |
| high     | 24576           | Deep reasoning |
| xhigh    | 32768           | Deeper reasoning |

### Model Name Suffix Syntax

Users append `(value)` to model names where value can be:
- Numeric budget: `claude-sonnet-4(16384)` → 16384 tokens (provider-native tokens, clamped to model's supported range)
- Effort level: `gpt-5.1(high)` → high effort (case-insensitive)
- Empty parentheses `()` are ignored
- For `provider://model` format, append brackets after model name: `openrouter://gemini-3-pro-preview(high)`

## Important Constraints

- **Compatibility:** Must match CLIProxyAPI's `NormalizeThinkingModel()` parsing logic exactly
- **Performance:** SSE streaming must not buffer; forward chunks immediately
- **Transparency:** All headers (especially auth) must be forwarded unchanged
- **Build dependency:** Requires network access at compile time to fetch CLIProxyAPI sources

## External Dependencies

### Upstream Services

- **CLIProxyAPI:** Primary upstream server (default: `cpa.1percentsync.games`)
- **GitHub Raw:** Source files fetched at compile time from `https://raw.githubusercontent.com/1PercentSync/CLIProxyAPI/main/`

### Source Files Parsed at Build Time

- `internal/util/thinking.go` - Effort level mappings
- `internal/util/gemini_thinking.go` - Gemini model detection
- `internal/util/claude_thinking.go` - Claude model support
- `internal/util/provider.go` - Model-to-provider mapping
