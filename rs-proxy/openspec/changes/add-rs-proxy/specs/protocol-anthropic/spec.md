## ADDED Requirements

### Requirement: Anthropic Protocol Thinking Injection
The system SHALL inject thinking configuration for Anthropic protocol aligned with CLIProxyAPI.

**File:** `src/protocol/anthropic.rs`

#### Scenario: Anthropic with effort level
- **WHEN** model has suffix `(high)` and protocol is Anthropic
- **AND** model exists in registry with thinking support
- **THEN** the system SHALL set `thinking.type` to `enabled`
- **AND** set `thinking.budget_tokens` to `24576`
- **AND** set `model` field to the base model name
- **AND** ensure `max_tokens` is sufficient (see max_tokens adjustment below)

#### Scenario: Anthropic with numeric budget
- **WHEN** model has suffix `(16384)` and protocol is Anthropic
- **AND** model exists in registry with thinking support
- **THEN** the system SHALL set `thinking.type` to `enabled`
- **AND** set `thinking.budget_tokens` to `16384` (clamped to model range)

#### Scenario: Anthropic with none level
- **WHEN** model has suffix `(none)` and protocol is Anthropic
- **THEN** the system SHALL NOT set any thinking configuration
- **AND** return the request body unchanged (no `thinking.type`, no `thinking.budget_tokens`)

#### Scenario: Anthropic with zero or negative budget
- **WHEN** model has budget <= 0 after processing
- **THEN** the system SHALL NOT set any thinking configuration

#### Scenario: Unknown model with thinking suffix
- **WHEN** model has thinking suffix (e.g., `(high)`, `(16384)`)
- **AND** model does NOT exist in registry
- **THEN** the system SHALL return HTTP 400 error with message indicating unknown model

> **⚠️ DESIGN DECISION - DIFFERS FROM CLIProxyAPI:**
> RS-Proxy requires models to be in the registry to apply thinking configuration.
> CLIProxyAPI falls back to `budget + 4000` for max_tokens when registry lookup fails.
> RS-Proxy instead returns an error for unknown models with thinking suffixes.
> This ensures predictable behavior and prevents silent failures with incorrect configurations.

### Requirement: max_tokens Adjustment for Thinking
The system SHALL ensure `max_tokens` is sufficient when thinking is enabled.

**File:** `src/protocol/anthropic.rs`

#### Scenario: max_tokens adjustment
- **WHEN** thinking is enabled with `budget_tokens > 0`
- **AND** model has `MaxCompletionTokens` in registry
- **THEN** the system SHALL set `max_tokens` to `MaxCompletionTokens` if current value is lower

> **Note:** Unlike CLIProxyAPI which has a fallback of `budget + 4000`, RS-Proxy does not need
> this fallback because unknown models are rejected before reaching this point.

### Implementation Notes

```rust
// First, check if model exists in registry
let model_info = registry.get_model_info(&base_model)
    .ok_or_else(|| Error::UnknownModel(base_model.clone()))?;

// Check if model supports thinking
if model_info.thinking.is_none() {
    // Model doesn't support thinking, just strip brackets and forward
    body["model"] = base_model;
    return Ok(body);
}

// Only apply thinking config if budget > 0
if budget > 0 {
    body["model"] = base_model;
    body["thinking"]["type"] = "enabled";
    body["thinking"]["budget_tokens"] = budget;

    // Set max_tokens to model's MaxCompletionTokens
    let current_max = body["max_tokens"].as_i64().unwrap_or(0);
    let required_max = model_info.max_completion_tokens as i64;  // e.g., 64000 for Claude 4.5

    if current_max < required_max {
        body["max_tokens"] = required_max;
    }
} else {
    // budget <= 0 (including "none" level which maps to 0)
    // Do NOT set any thinking configuration, return body unchanged
    body["model"] = base_model;
}
```

**Critical:**
- Level strings are first converted to budgets via the mapping table
- `(none)` maps to budget 0, which means NO thinking config is set (not `budget_tokens = 0`)
- Anthropic API requires `max_tokens > thinking.budget_tokens`; violating this returns HTTP 400
- **RS-Proxy rejects unknown models with thinking suffixes (differs from CLIProxyAPI)**
