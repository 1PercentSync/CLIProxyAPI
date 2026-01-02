## ADDED Requirements

### Requirement: Protocol Detection
The system SHALL detect API protocol from URL path, using headers only for ambiguous endpoints.

**File:** `src/protocol/mod.rs`

#### Scenario: OpenAI detection by path
- **WHEN** request path is `/v1/chat/completions` or `/v1/responses`
- **THEN** the system SHALL treat it as OpenAI protocol

#### Scenario: Anthropic detection by path
- **WHEN** request path is `/v1/messages`
- **THEN** the system SHALL treat it as Anthropic protocol

#### Scenario: Gemini detection by path
- **WHEN** request path matches `/v1beta/models/*`
- **THEN** the system SHALL treat it as Gemini protocol

#### Scenario: Shared endpoint disambiguation by headers
- **WHEN** request path is `/v1/models`
- **AND** request contains `x-api-key` header
- **THEN** the system SHALL treat it as Anthropic protocol

#### Scenario: Shared endpoint fallback to OpenAI
- **WHEN** request path is `/v1/models`
- **AND** request contains `Authorization: Bearer` header without `x-api-key`
- **THEN** the system SHALL treat it as OpenAI protocol

### Implementation Notes

```rust
pub enum Protocol {
    OpenAI,
    Anthropic,
    Gemini,
}

pub fn detect_protocol(path: &str, headers: &HeaderMap) -> Protocol {
    match path {
        "/v1/chat/completions" | "/v1/responses" => Protocol::OpenAI,
        "/v1/messages" => Protocol::Anthropic,
        p if p.starts_with("/v1beta/models") => Protocol::Gemini,
        "/v1/models" => {
            if headers.contains_key("x-api-key") {
                Protocol::Anthropic
            } else {
                Protocol::OpenAI
            }
        }
        _ => Protocol::OpenAI, // fallback
    }
}
```

**Important:** Headers are ONLY used for `/v1/models` endpoint disambiguation. All other endpoints use URL path only.
