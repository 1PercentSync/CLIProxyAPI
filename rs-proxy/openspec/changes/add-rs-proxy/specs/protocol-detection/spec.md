## ADDED Requirements

### Requirement: 协议检测

The system SHALL从 URL 路径检测 API 协议，仅对歧义端点使用请求头判断。

**文件：** `src/protocol/mod.rs`

#### Scenario: 通过路径检测 OpenAI
- **当** 请求路径为 `/v1/chat/completions` 或 `/v1/responses` 时
- **则** The system SHALL将其视为 OpenAI 协议

#### Scenario: 通过路径检测 Anthropic
- **当** 请求路径为 `/v1/messages` 时
- **则** The system SHALL将其视为 Anthropic 协议

#### Scenario: 通过路径检测 Gemini
- **当** 请求路径匹配 `/v1beta/models/*` 时
- **则** The system SHALL将其视为 Gemini 协议

#### Scenario: 通过请求头区分共享端点
- **当** 请求路径为 `/v1/models` 时
- **且** 请求包含 `x-api-key` 头
- **则** The system SHALL将其视为 Anthropic 协议

#### Scenario: 共享端点回退到 OpenAI
- **当** 请求路径为 `/v1/models` 时
- **且** 请求包含 `Authorization: Bearer` 头但不含 `x-api-key`
- **则** The system SHALL将其视为 OpenAI 协议

### 实现说明

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
        _ => Protocol::OpenAI, // 回退
    }
}
```

**重要说明：** 请求头仅用于 `/v1/models` 端点的协议区分。其他所有端点仅使用 URL 路径。
