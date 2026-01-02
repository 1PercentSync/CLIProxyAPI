## ADDED Requirements

### Requirement: Model Name Suffix Parsing
The system SHALL parse thinking configuration from model name suffixes aligned with CLIProxyAPI's `NormalizeThinkingModel()` logic.

**File:** `src/thinking/parser.rs`

#### Scenario: Numeric budget suffix
- **WHEN** a request contains model name `claude-sonnet-4(16384)`
- **THEN** the system SHALL extract base model `claude-sonnet-4`
- **AND** extract thinking budget `16384` (provider-native tokens, clamped to model's supported range)

#### Scenario: String effort level suffix
- **WHEN** a request contains model name `gpt-5.1(high)`
- **THEN** the system SHALL extract base model `gpt-5.1`
- **AND** extract reasoning effort `high` (case-insensitive)

#### Scenario: No suffix
- **WHEN** a request contains model name `claude-sonnet-4` without parentheses
- **THEN** the system SHALL use the model name as-is
- **AND** not inject any thinking configuration

#### Scenario: Empty parentheses
- **WHEN** a request contains model name `model-name()`
- **THEN** the system SHALL ignore the empty parentheses
- **AND** use `model-name` as the model name (brackets stripped)

#### Scenario: Provider prefix format
- **WHEN** a request contains model name `openrouter://gemini-3-pro-preview(high)`
- **THEN** the system SHALL extract base model `openrouter://gemini-3-pro-preview`
- **AND** extract reasoning effort `high`

### Implementation Notes

Parser returns a struct like:
```rust
pub enum ThinkingValue {
    None,                    // No suffix or empty ()
    Budget(i32),             // Numeric value like 16384
    Level(String),           // Level like "high", "auto", "none"
}

pub struct ParsedModel {
    pub base_name: String,
    pub thinking: ThinkingValue,
}
```
