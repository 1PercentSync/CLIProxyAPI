## ADDED Requirements

### Requirement: Protocol 枚举定义

The system SHALL 定义 `Protocol` 枚举，作为整个项目中协议类型的统一表示。

**文件：** `src/protocol/mod.rs`

> **说明：** `Protocol` 枚举是公共类型，被以下模块使用：
> - `thinking/injector.rs`：根据协议类型决定注入格式
> - `protocol/*.rs`：协议特定的注入实现
> - `models/enhancer.rs`：根据协议类型增强模型列表

```rust
/// API 协议类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    OpenAI,
    Anthropic,
    Gemini,
}
```

### Requirement: 协议检测

The system SHALL 从 URL 路径检测 API 协议，仅对歧义端点使用请求头判断。

**文件：** `src/protocol/mod.rs`

#### Scenario: 通过路径检测 OpenAI
- **当** 请求路径为 `/v1/chat/completions` 或 `/v1/responses` 时
- **则** The system SHALL 将其视为 OpenAI 协议

#### Scenario: 通过路径检测 Anthropic
- **当** 请求路径为 `/v1/messages` 时
- **则** The system SHALL 将其视为 Anthropic 协议

#### Scenario: 通过路径检测 Gemini
- **当** 请求路径匹配 `/v1beta/models/*` 时
- **则** The system SHALL 将其视为 Gemini 协议

#### Scenario: 通过请求头区分共享端点
- **当** 请求路径为 `/v1/models` 时
- **且** 请求包含 `x-api-key` 头
- **则** The system SHALL 将其视为 Anthropic 协议

#### Scenario: 共享端点回退到 OpenAI
- **当** 请求路径为 `/v1/models` 时
- **且** 请求包含 `Authorization: Bearer` 头但不含 `x-api-key`
- **则** The system SHALL 将其视为 OpenAI 协议

### 实现说明

```rust
use axum::http::HeaderMap;

/// API 协议类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    OpenAI,
    Anthropic,
    Gemini,
}

/// 从请求路径和头部检测协议类型
///
/// 检测优先级：
/// 1. 路径精确匹配（大部分端点）
/// 2. 路径前缀匹配（Gemini）
/// 3. 请求头判断（仅 /v1/models）
/// 4. 回退到 OpenAI
pub fn detect_protocol(path: &str, headers: &HeaderMap) -> Protocol {
    match path {
        "/v1/chat/completions" | "/v1/responses" => Protocol::OpenAI,
        "/v1/messages" => Protocol::Anthropic,
        p if p.starts_with("/v1beta/models") => Protocol::Gemini,
        "/v1/models" => {
            // 共享端点：使用请求头区分
            if headers.contains_key("x-api-key") {
                Protocol::Anthropic
            } else {
                Protocol::OpenAI
            }
        }
        _ => Protocol::OpenAI, // 未知路径回退到 OpenAI
    }
}
```

**重要说明：** 请求头仅用于 `/v1/models` 端点的协议区分。其他所有端点仅使用 URL 路径。

### 模块导出

`src/protocol/mod.rs` 应导出以下内容供其他模块使用：

```rust
// src/protocol/mod.rs

mod openai;
mod anthropic;
mod gemini;

pub use self::openai::inject_openai;
pub use self::anthropic::inject_anthropic;
pub use self::gemini::inject_gemini;

// Protocol 枚举和检测函数（定义在本文件中）
// pub enum Protocol { ... }
// pub fn detect_protocol(...) { ... }
```
