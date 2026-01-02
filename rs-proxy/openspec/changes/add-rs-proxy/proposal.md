# Change: Add Rust Reverse Proxy with Thinking Configuration Support

## Why

Users need a lightweight, standalone reverse proxy that can parse thinking configuration from model name suffixes (e.g., `model(high)` or `model(16384)`) and inject the appropriate configuration into API requests. This allows clients to control thinking/reasoning behavior without modifying their request payloads.

## What Changes

- Add new Rust project `rs-proxy` as a standalone reverse proxy (can forward to any upstream, not just CLIProxyAPI)
- Implement model name suffix parsing aligned with CLIProxyAPI's logic for consistency
- Support OpenAI, Anthropic, and Gemini API protocols (protocol determined by URL path, except `/v1/models` which uses headers)
- Inject thinking configuration aligned with CLIProxyAPI's behavior:
  - **OpenAI/Codex/Qwen/iFlow/OpenRouter:** `reasoning_effort` (chat) or `reasoning.effort` (Responses); numeric budgets are converted to level strings
  - **Anthropic:** `thinking.type=enabled` + `thinking.budget_tokens`; `max_tokens` set to model's `MaxCompletionTokens`; `(none)` results in no thinking config
  - **Gemini 2.5:** `thinkingBudget` (numeric) + auto-set `include_thoughts=true`
  - **Gemini 3:** `thinkingLevel` (string: low/medium/high) + auto-set `includeThoughts=true`
- Enhance model list responses with thinking level variants (this differs from CLIProxyAPI which does not include variants)
- Handle SSE streaming responses correctly
- Fetch model support data from CLIProxyAPI at compile time (`internal/registry/model_definitions.go`)

**Note:** RS-Proxy does NOT provide protocol conversion - it only injects thinking configuration into the original protocol format.

## Design Decisions (Differs from CLIProxyAPI)

### Unknown Models with Thinking Suffix

- **CLIProxyAPI:** Allows thinking suffixes on unknown models, uses fallback behavior
- **RS-Proxy:** Returns HTTP 400 error for unknown models with thinking suffix

**Rationale:** Ensures predictable behavior, prevents silent failures with incorrect configurations.

## Impact

- Affected specs (new capabilities):
  - `cli` → `src/config.rs`
  - `thinking-parser` → `src/thinking/parser.rs`
  - `thinking-mapping` → `src/thinking/models.rs`
  - `protocol-detection` → `src/protocol/mod.rs`
  - `protocol-openai` → `src/protocol/openai.rs`
  - `protocol-anthropic` → `src/protocol/anthropic.rs`
  - `protocol-gemini` → `src/protocol/gemini.rs`
  - `proxy-streaming` → `src/proxy/streaming.rs`
  - `proxy-headers` → `src/proxy/client.rs`
  - `models-enhancer` → `src/models/enhancer.rs`
  - `build` → `build.rs`
- Affected code: New project in `/rs-proxy` directory
- Dependencies: tokio, axum, reqwest, argh, serde_json, tower-http, regex, futures, thiserror
