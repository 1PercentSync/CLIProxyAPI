## ADDED Requirements

### Requirement: Gemini Protocol Thinking Injection
The system SHALL inject thinking configuration for Gemini protocol aligned with CLIProxyAPI.

**File:** `src/protocol/gemini.rs`

#### Scenario: Gemini 2.5 with thinking budget
- **WHEN** model is Gemini 2.5 variant (e.g., `gemini-2.5-pro`, `gemini-2.5-flash`)
- **AND** has thinking suffix
- **THEN** the system SHALL set `generationConfig.thinkingConfig.thinkingBudget` to the numeric value
- **AND** set `generationConfig.thinkingConfig.include_thoughts` to `true` if not explicitly set

#### Scenario: Gemini 3 with thinking level
- **WHEN** model is Gemini 3 variant (e.g., `gemini-3-pro-preview`, `gemini-3-flash-preview`)
- **AND** has thinking suffix
- **THEN** the system SHALL convert budget to level string and set `generationConfig.thinkingConfig.thinkingLevel`
- **AND** set `generationConfig.thinkingConfig.includeThoughts` to `true` if not explicitly set
- **AND** remove `thinkingBudget` field if present (Gemini 3 uses level, not budget)

#### Scenario: Gemini 3 budget to level conversion
- **WHEN** model is Gemini 3 and has numeric budget
- **THEN** the system SHALL convert using these rules:
  - For Gemini 3 Pro: only `"low"`, `"high"` supported
    - budget <= 1024 → `"low"`
    - budget > 1024 → `"high"`
  - For Gemini 3 Flash: `"minimal"`, `"low"`, `"medium"`, `"high"` supported
    - budget <= 512 → `"minimal"`
    - budget <= 1024 → `"low"`
    - budget <= 8192 → `"medium"`
    - budget > 8192 → `"high"`
  - budget == -1 (auto) → `"high"`

#### Scenario: Gemini with auto level
- **WHEN** model is Gemini and has suffix `(auto)`
- **THEN** for Gemini 2.5: set `thinkingBudget` to `-1`
- **AND** for Gemini 3: set `thinkingLevel` to `"high"`

#### Scenario: Gemini model with default thinking
- **WHEN** model has default thinking enabled (e.g., `gemini-3-pro-preview`)
- **AND** no suffix is provided
- **THEN** the system SHALL NOT disable thinking
- **AND** let model use its default behavior

#### Scenario: Unknown model with thinking suffix
- **WHEN** model has thinking suffix (e.g., `(high)`, `(16384)`)
- **AND** model does NOT exist in registry
- **THEN** the system SHALL return HTTP 400 error with message indicating unknown model

> **⚠️ DESIGN DECISION - DIFFERS FROM CLIProxyAPI:**
> RS-Proxy requires models to be in the registry to apply thinking configuration.
> See thinking-mapping/spec.md for full details on this design decision.

### Requirement: include_thoughts Auto-Setting
The system SHALL automatically set include_thoughts when thinking is configured.

#### Scenario: Auto-set include_thoughts for Gemini 2.5
- **WHEN** thinking budget is set and budget != 0
- **AND** user did not explicitly set `include_thoughts`
- **THEN** the system SHALL set `generationConfig.thinkingConfig.include_thoughts` to `true`

#### Scenario: Auto-set includeThoughts for Gemini 3
- **WHEN** thinking level is set
- **AND** user did not explicitly set `includeThoughts`
- **THEN** the system SHALL set `generationConfig.thinkingConfig.includeThoughts` to `true`
- **AND** remove legacy `include_thoughts` field if present (Gemini 3 uses camelCase)

### Implementation Notes

```rust
fn is_gemini_3(model: &str) -> bool {
    model.contains("gemini-3")
}

fn is_gemini_3_flash(model: &str) -> bool {
    model.contains("gemini-3") && model.contains("flash")
}

fn budget_to_gemini3_level(model: &str, budget: i32) -> &'static str {
    if budget == -1 {
        return "high";  // auto -> high
    }
    if is_gemini_3_flash(model) {
        match budget {
            ..=512 => "minimal",
            ..=1024 => "low",
            ..=8192 => "medium",
            _ => "high",
        }
    } else {
        // Gemini 3 Pro: only low/high
        if budget <= 1024 { "low" } else { "high" }
    }
}

// For Gemini 2.5
if !is_gemini_3(model) {
    body["generationConfig"]["thinkingConfig"]["thinkingBudget"] = budget;
    if !has_explicit_include_thoughts {
        body["generationConfig"]["thinkingConfig"]["include_thoughts"] = true;
    }
}

// For Gemini 3
if is_gemini_3(model) {
    let level = budget_to_gemini3_level(model, budget);
    body["generationConfig"]["thinkingConfig"]["thinkingLevel"] = level;
    // Remove thinkingBudget if present
    body["generationConfig"]["thinkingConfig"].remove("thinkingBudget");
    if !has_explicit_include_thoughts {
        body["generationConfig"]["thinkingConfig"]["includeThoughts"] = true;
    }
    // Clean up legacy snake_case field
    body["generationConfig"]["thinkingConfig"].remove("include_thoughts");
}
```

**Critical:**
- Gemini 2.5 uses `thinkingBudget` (numeric) + `include_thoughts` (snake_case)
- Gemini 3 uses `thinkingLevel` (string) + `includeThoughts` (camelCase)
- When setting thinking config, auto-set include_thoughts/includeThoughts to `true` unless user explicitly specified otherwise
- Models like `gemini-3-pro-preview` have default thinking enabled; only override if suffix provided
