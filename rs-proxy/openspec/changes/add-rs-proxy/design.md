## Context

RS-Proxy is a standalone lightweight Rust reverse proxy that transparently forwards API requests while parsing and applying thinking configuration from model name suffixes. It can forward to any upstream API server (default: CLIProxyAPI instance).

**Important:** RS-Proxy is NOT a middleware specifically for CLIProxyAPI. It is a general-purpose proxy that aligns its thinking configuration logic with CLIProxyAPI for consistency.

**Constraints:**
- Must align with CLIProxyAPI's thinking suffix parsing and injection logic
- Must support SSE streaming without buffering
- Must compile model support data from CLIProxyAPI source at build time
- Does NOT provide protocol conversion - only injects thinking configuration

**Stakeholders:**
- API clients wanting simplified thinking configuration via model names
- Users who need a lightweight local proxy with thinking injection support

## Goals / Non-Goals

**Goals:**
- Parse model suffixes like `model(high)` or `model(16384)`
- Inject protocol-appropriate thinking configuration
- Transparent proxying with SSE support
- Model list enhancement with thinking variants

**Non-Goals:**
- Authentication/authorization (transparent passthrough only)
- Request caching
- Load balancing
- Model-specific prompt transformations

## Decisions

### Decision 1: Use axum as HTTP framework
- **Why:** Lightweight, async-first, excellent tower ecosystem integration
- **Alternatives:** actix-web (heavier), warp (less ergonomic), hyper (too low-level)

### Decision 2: Compile-time data generation via build.rs
- **Why:** Avoids runtime config files, ensures type safety
- **Alternatives:** Runtime config parsing (adds startup cost, error handling complexity)

### Decision 3: Protocol detection strategy
- **Primary:** URL path determines protocol (e.g., `/v1/messages` → Anthropic, `/v1beta/models/*` → Gemini, `/v1/chat/completions` → OpenAI)
- **Exception:** `/v1/models` endpoint is shared by OpenAI and Anthropic, so headers are used: `x-api-key` → Anthropic, `Authorization: Bearer` → OpenAI
- **Why:** Minimizes header inspection overhead; most endpoints have unique paths

### Decision 4: Transparent SSE forwarding
- **Why:** Minimizes latency and complexity
- **Approach:** Use reqwest's `bytes_stream()` and forward chunks directly

### Decision 5: Thinking injection rules (aligned with CLIProxyAPI)
Protocol-specific injection behavior matching CLIProxyAPI's implementation:

| Protocol | Level/auto/none | Numeric budget |
|----------|-----------------|----------------|
| OpenAI (chat) | Override `reasoning_effort` | No modification |
| OpenAI (Responses) | Override `reasoning.effort` | No modification |
| Anthropic | Set `thinking.type=enabled` + `thinking.budget_tokens` | Set `thinking.type=enabled` + `thinking.budget_tokens` |
| Gemini | Set `generationConfig.thinkingConfig.thinkingBudget` | Set `generationConfig.thinkingConfig.thinkingBudget` |

**Important notes:**
- Only models declaring thinking support get injection; unsupported models just have brackets stripped
- Budget values are clamped to model's supported range
- Gemini: does NOT modify `include_thoughts`
- Claude: may increase `max_tokens` if necessary
- Models using discrete levels validate the level; unsupported values return 400

### Decision 6: Model list enhancement (differs from CLIProxyAPI)
- RS-Proxy enhances `/v1/models` response by adding thinking level variants (e.g., `model(low)`, `model(high)`)
- CLIProxyAPI does NOT include these variants in its model list response
- This is an intentional difference to help clients discover available thinking configurations

## Risks / Trade-offs

| Risk | Mitigation |
|------|------------|
| CLIProxyAPI source format changes | build.rs parsing is regex-based, may need updates |
| Large streaming responses | No buffering, direct passthrough minimizes memory |
| Protocol detection ambiguity | Clear header-based rules, fallback to OpenAI format |

## Migration Plan

N/A - New project, no migration required.

## Open Questions

1. Should we support custom thinking level names beyond the standard set?
2. Should model list caching be added for performance?
