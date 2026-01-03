## ADDED Requirements

### Requirement: 错误类型定义

The system SHALL 使用 thiserror 定义统一的错误类型。

**文件：** `src/error.rs`

#### Scenario: 请求转发错误
- **当** 转发请求到上游失败时
- **则** The system SHALL 返回 HTTP 502 Bad Gateway

#### Scenario: 请求体解析错误
- **当** 无法解析请求体为 JSON 时
- **则** The system SHALL 返回 HTTP 400 Bad Request

#### Scenario: 未知模型错误
- **当** 模型带有思考后缀但不在注册表中时
- **则** The system SHALL 返回 HTTP 400 Bad Request
- **且** 错误信息应说明模型未知

> **说明：** 未知模型错误的具体逻辑在 `thinking-injector/spec.md` 中定义。

### Requirement: 错误响应格式

The system SHALL 返回 JSON 格式的错误响应。

**文件：** `src/error.rs`

#### Scenario: 代理错误响应
- **当** 代理自身产生错误时
- **则** The system SHALL 返回以下格式：
```json
{
  "error": {
    "message": "error description",
    "type": "proxy_error"
  }
}
```

#### Scenario: 上游错误透传
- **当** 上游返回错误响应时
- **则** The system SHALL 原样透传上游的错误响应
- **且** 保留原始状态码

### Requirement: 结构化日志

The system SHALL 使用 tracing 输出结构化日志。

**文件：** `src/main.rs`

#### Scenario: 启动日志
- **当** 代理启动时
- **则** The system SHALL 记录监听地址和上游 URL

#### Scenario: 请求日志
- **当** 处理请求时
- **则** The system SHALL 记录：
  - 请求方法和路径
  - 检测到的协议
  - 模型名称（如有）
  - 响应状态码
  - 处理耗时

#### Scenario: 错误日志
- **当** 发生错误时
- **则** The system SHALL 记录错误详情

### 实现说明

```rust
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ProxyError {
    #[error("request forwarding failed: {0}")]
    ForwardingFailed(#[from] reqwest::Error),

    #[error("invalid request body: {0}")]
    InvalidBody(#[from] serde_json::Error),

    #[error("unknown model with thinking suffix: {0}")]
    UnknownModel(String),

    #[error("internal error: {0}")]
    Internal(String),
}

impl IntoResponse for ProxyError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            ProxyError::ForwardingFailed(_) => (StatusCode::BAD_GATEWAY, self.to_string()),
            ProxyError::InvalidBody(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            ProxyError::UnknownModel(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            ProxyError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };

        let body = json!({
            "error": {
                "message": message,
                "type": "proxy_error"
            }
        });

        (status, Json(body)).into_response()
    }
}
```

### 日志配置

```rust
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

fn init_tracing() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rs_proxy=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();
}
```

### 日志示例

```
INFO rs_proxy: Starting proxy server
INFO rs_proxy: Listening on 0.0.0.0:6356
INFO rs_proxy: Upstream URL: https://cpa.1percentsync.games

INFO tower_http::trace: started processing request method=POST path=/v1/chat/completions
INFO rs_proxy: protocol=OpenAI model=gpt-5.1(high) thinking_suffix=high
INFO tower_http::trace: finished processing request status=200 latency=1234ms
```
