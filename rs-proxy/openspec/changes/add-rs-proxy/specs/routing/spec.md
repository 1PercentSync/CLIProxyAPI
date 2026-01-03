## ADDED Requirements

### Requirement: 请求路由

The system SHALL 配置 axum 路由器处理所有 API 端点。

**文件：** `src/main.rs`

#### Scenario: 聊天补全端点
- **当** 收到 `POST /v1/chat/completions` 请求时
- **则** The system SHALL 检测为 OpenAI 协议
- **且** 应用思考注入逻辑后转发到上游

#### Scenario: Responses 端点
- **当** 收到 `POST /v1/responses` 请求时
- **则** The system SHALL 检测为 OpenAI 协议
- **且** 应用思考注入逻辑后转发到上游

#### Scenario: Messages 端点
- **当** 收到 `POST /v1/messages` 请求时
- **则** The system SHALL 检测为 Anthropic 协议
- **且** 应用思考注入逻辑后转发到上游

#### Scenario: Gemini 端点
- **当** 收到 `POST /v1beta/models/{model}:generateContent` 或 `:streamGenerateContent` 请求时
- **则** The system SHALL 检测为 Gemini 协议
- **且** 应用思考注入逻辑后转发到上游

#### Scenario: 模型列表端点
- **当** 收到 `GET /v1/models` 请求时
- **则** The system SHALL 根据请求头检测协议（OpenAI 或 Anthropic）
- **且** 转发到上游并增强响应（添加思考变体）

#### Scenario: Gemini 模型列表端点
- **当** 收到 `GET /v1beta/models` 请求时
- **则** The system SHALL 检测为 Gemini 协议
- **且** 转发到上游并增强响应

#### Scenario: 其他端点
- **当** 收到不匹配上述路径的请求时
- **则** The system SHALL 直接透传到上游，不做任何处理

### Requirement: CORS 中间件

The system SHALL 配置 CORS 允许所有来源访问。

**文件：** `src/main.rs`

#### Scenario: 跨域请求
- **当** 收到带有 `Origin` 头的请求时
- **则** The system SHALL 返回适当的 CORS 响应头
- **且** 允许所有来源（`Access-Control-Allow-Origin: *`）

> **设计原理：**
> RS-Proxy 作为本地代理运行，主要服务本地客户端。
> 允许所有来源简化配置，不影响安全性。

### Requirement: 追踪中间件

The system SHALL 使用 tower-http 追踪中间件记录请求日志。

**文件：** `src/main.rs`

#### Scenario: 请求日志
- **当** 收到任何请求时
- **则** The system SHALL 记录请求方法、路径、状态码和耗时

### 实现说明

```rust
use axum::{
    routing::{get, post, any},
    Router,
};
use tower_http::{cors::CorsLayer, trace::TraceLayer};

pub fn create_router(state: AppState) -> Router {
    Router::new()
        // OpenAI 端点
        .route("/v1/chat/completions", post(handle_chat_completions))
        .route("/v1/responses", post(handle_responses))
        // Anthropic 端点
        .route("/v1/messages", post(handle_messages))
        // 模型列表端点（OpenAI/Anthropic 共用，通过请求头区分）
        .route("/v1/models", get(handle_models))
        // Gemini 端点
        .route("/v1beta/models", get(handle_gemini_models))
        .route("/v1beta/models/*path", any(handle_gemini))
        // 其他请求透传
        .fallback(handle_passthrough)
        // 中间件
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// 应用状态
#[derive(Clone)]
pub struct AppState {
    pub client: reqwest::Client,
    pub base_url: String,
}
```

### 请求处理流程

```
请求 → CORS 中间件 → 追踪中间件 → 路由匹配 → 处理函数
                                              ↓
                                    ┌─────────────────────┐
                                    │ 1. 协议检测         │
                                    │ 2. 解析请求体       │
                                    │ 3. 思考注入         │
                                    │ 4. 转发到上游       │
                                    │ 5. 返回响应         │
                                    └─────────────────────┘
```
