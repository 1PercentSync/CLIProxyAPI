## ADDED Requirements

### Requirement: Effort to Budget Mapping
The system SHALL map effort level strings to token budgets matching CLIProxyAPI.

**File:** `src/thinking/models.rs`

#### Scenario: Standard effort levels
- **WHEN** effort level is one of: none, auto, minimal, low, medium, high, xhigh
- **THEN** the system SHALL map to budgets: 0, -1, 512, 1024, 8192, 24576, 32768 respectively
- **AND** apply clamping rules (none→0 clamped to min if 0 not allowed; auto→-1 or clamped if not supported)

### Requirement: Budget to Effort Reverse Mapping
The system SHALL map numeric budgets back to effort level strings (needed for OpenAI protocol).

**File:** `src/thinking/models.rs`

#### Scenario: Budget to effort conversion
- **WHEN** a numeric budget needs to be converted to an effort level (e.g., for OpenAI)
- **THEN** the system SHALL use these ranges:
  - 0 → `"none"` (or model's lowest supported level if 0 not allowed)
  - -1 → `"auto"`
  - 1 - 1024 → `"low"`
  - 1025 - 8192 → `"medium"`
  - 8193 - 24576 → `"high"`
  - 24577+ → model's highest supported level (default `"xhigh"`)

### Requirement: Thinking Injection for Models with Thinking Support
The system SHALL only inject thinking configuration for models that declare thinking support.

**File:** `src/thinking/models.rs`

#### Scenario: Model in registry without thinking support
- **WHEN** model exists in registry but does not declare thinking support
- **AND** has suffix `(high)`
- **THEN** the system SHALL strip the brackets and use base model name
- **AND** NOT inject any thinking fields

#### Scenario: Model not in registry with thinking suffix
- **WHEN** model has thinking suffix (e.g., `(high)`, `(16384)`)
- **AND** model does NOT exist in registry
- **THEN** the system SHALL return HTTP 400 error with message indicating unknown model

> **⚠️ DESIGN DECISION - DIFFERS FROM CLIProxyAPI:**
> RS-Proxy requires models to be in the registry to apply thinking configuration.
> CLIProxyAPI allows thinking suffixes on unknown models and uses fallback behavior.
> RS-Proxy instead returns an error, ensuring predictable behavior and preventing
> silent failures with incorrect configurations (e.g., wrong max_tokens values).

### Requirement: Discrete Level Validation
The system SHALL validate effort levels for models using discrete levels.

**File:** `src/thinking/models.rs`

#### Scenario: Invalid level for discrete model
- **WHEN** model uses discrete levels and suffix contains unsupported level
- **THEN** the system SHALL return HTTP 400 error

### Implementation Notes

```rust
/// Level to budget (forward mapping)
pub fn level_to_budget(level: &str) -> Option<i32> {
    match level.to_lowercase().as_str() {
        "none" => Some(0),
        "auto" => Some(-1),
        "minimal" => Some(512),
        "low" => Some(1024),
        "medium" => Some(8192),
        "high" => Some(24576),
        "xhigh" => Some(32768),
        _ => None,
    }
}

/// Budget to effort (reverse mapping, needed for OpenAI protocol)
pub fn budget_to_effort(budget: i32) -> &'static str {
    match budget {
        0 => "none",
        -1 => "auto",
        1..=1024 => "low",
        1025..=8192 => "medium",
        8193..=24576 => "high",
        _ if budget > 24576 => "xhigh",
        _ => "medium",  // fallback for unexpected values
    }
}

/// Budget to effort with model awareness (uses model's highest supported level)
pub fn budget_to_effort_for_model(model: &str, budget: i32) -> String {
    if budget > 24576 {
        // Return model's highest supported level, or "xhigh" as default
        if let Some(levels) = get_model_thinking_levels(model) {
            return levels.last().unwrap_or(&"xhigh").to_string();
        }
    }
    budget_to_effort(budget).to_string()
}
```
