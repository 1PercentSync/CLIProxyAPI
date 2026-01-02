## ADDED Requirements

### Requirement: Model List Enhancement
The system SHALL enhance model list responses with thinking level variants. This differs from CLIProxyAPI which does not include variants.

**File:** `src/models/enhancer.rs`

#### Scenario: OpenAI models endpoint
- **WHEN** a GET request is made to `/v1/models` with OpenAI headers
- **THEN** the system SHALL forward to upstream
- **AND** for each model supporting thinking, add variants like `model(low)`, `model(medium)`, `model(high)`

#### Scenario: Anthropic models endpoint
- **WHEN** a GET request is made to `/v1/models` with Anthropic headers
- **THEN** the system SHALL forward to upstream
- **AND** enhance response with thinking variants

#### Scenario: Gemini models endpoint
- **WHEN** a GET request is made to `/v1beta/models`
- **THEN** the system SHALL forward to upstream
- **AND** enhance response with thinking variants for supported models

### Implementation Notes

```rust
fn enhance_model_list(models: Vec<Model>, protocol: Protocol) -> Vec<Model> {
    let mut enhanced = models.clone();

    for model in &models {
        if supports_thinking(&model.id) {
            for level in ["low", "medium", "high"] {
                enhanced.push(Model {
                    id: format!("{}({})", model.id, level),
                    ..model.clone()
                });
            }
        }
    }

    enhanced
}
```

**Important:** This is an RS-Proxy-specific feature. CLIProxyAPI does NOT enhance model lists with variants.
