## ADDED Requirements

### Requirement: OpenAI Protocol Thinking Injection
The system SHALL inject thinking configuration for OpenAI protocol aligned with CLIProxyAPI.

**File:** `src/protocol/openai.rs`

#### Scenario: Chat completions with level suffix
- **WHEN** model has suffix `(high)` and protocol is OpenAI chat (`/v1/chat/completions`)
- **AND** model exists in registry with thinking support
- **THEN** the system SHALL set `reasoning_effort` to `high`
- **AND** set `model` field to the base model name

#### Scenario: Responses endpoint with level suffix
- **WHEN** model has suffix `(high)` and protocol is OpenAI Responses (`/v1/responses`)
- **AND** model exists in registry with thinking support
- **THEN** the system SHALL set `reasoning.effort` to `high`
- **AND** set `model` field to the base model name

#### Scenario: Numeric budget for OpenAI
- **WHEN** model has numeric suffix `(16384)` and protocol is OpenAI
- **AND** model exists in registry with thinking support
- **THEN** the system SHALL convert the numeric budget to a level string using reverse mapping:
  - 1 - 1024 → `"low"`
  - 1025 - 8192 → `"medium"`
  - 8193 - 24576 → `"high"`
  - 24577+ → highest supported level (default `"xhigh"`)
- **AND** set `reasoning_effort` (chat) or `reasoning.effort` (responses) to the converted level
- **AND** set `model` field to the base model name

#### Scenario: Unknown model with thinking suffix
- **WHEN** model has thinking suffix (e.g., `(high)`, `(16384)`)
- **AND** model does NOT exist in registry
- **THEN** the system SHALL return HTTP 400 error with message indicating unknown model

> **⚠️ DESIGN DECISION - DIFFERS FROM CLIProxyAPI:**
> RS-Proxy requires models to be in the registry to apply thinking configuration.
> See thinking-mapping/spec.md for full details on this design decision.

### Implementation Notes

**Critical:** OpenAI protocol only accepts level strings for `reasoning_effort`, NOT numeric budgets. Numeric budgets must be converted to levels first.

```rust
// Budget to level conversion (reverse mapping)
fn budget_to_effort(budget: i32) -> &'static str {
    match budget {
        1..=1024 => "low",
        1025..=8192 => "medium",
        8193..=24576 => "high",
        _ if budget > 24576 => "xhigh",
        _ => "medium", // fallback
    }
}

// For /v1/chat/completions
body["model"] = base_model;
let level = match thinking {
    ThinkingValue::Level(l) => l,
    ThinkingValue::Budget(b) => budget_to_effort(b),
};
body["reasoning_effort"] = level;

// For /v1/responses
body["model"] = base_model;
body["reasoning"]["effort"] = level;
```
