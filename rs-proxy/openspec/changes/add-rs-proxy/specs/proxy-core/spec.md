## ADDED Requirements

### Requirement: HTTP 客户端封装

The system SHALL 使用 reqwest 提供 HTTP 客户端封装。

**文件：** `src/proxy/client.rs`

#### Scenario: 客户端配置
- **当** 代理启动时
- **则** The system SHALL 创建共享的 reqwest::Client
- **且** 配置合理的超时和连接池

#### Scenario: 请求转发
- **当** 收到需要转发的请求时
- **则** The system SHALL 构建上游请求
- **且** 转发请求体和头部
- **且** 返回上游响应

#### 实现说明

```rust
use reqwest::Client;
use std::time::Duration;

/// 创建共享的 HTTP 客户端
pub fn create_client() -> Client {
    Client::builder()
        .timeout(Duration::from_secs(120))
        .pool_max_idle_per_host(10)
        .build()
        .expect("Failed to create HTTP client")
}

/// 转发请求到上游
pub async fn forward_request(
    client: &Client,
    base_url: &str,
    method: reqwest::Method,
    path: &str,
    headers: axum::http::HeaderMap,
    body: bytes::Bytes,
) -> Result<reqwest::Response, reqwest::Error> {
    let url = format!("https://{}{}", base_url, path);
    let forwarded_headers = forward_headers(&headers);

    client
        .request(method, &url)
        .headers(forwarded_headers)
        .body(body)
        .send()
        .await
}
```

**关键点：**
- 使用共享的 `Client` 实例以复用连接池
- 超时设置为 120 秒以支持长时间的 API 调用
- 上游 URL 始终使用 HTTPS 协议

---

### Requirement: 头部转发

The system SHALL透明转发认证和其他请求头。

**文件：** `src/proxy/client.rs`

#### Scenario: Authorization 头
- **当** 请求包含 `Authorization` 头时
- **则** The system SHALL原样转发到上游

#### Scenario: API key 头
- **当** 请求包含 `x-api-key` 头时
- **则** The system SHALL原样转发到上游

#### 实现说明

```rust
fn forward_headers(incoming: &HeaderMap) -> HeaderMap {
    let mut outgoing = HeaderMap::new();

    // 转发所有头部，除了主机特定的
    for (key, value) in incoming.iter() {
        if key != "host" && key != "content-length" {
            outgoing.insert(key.clone(), value.clone());
        }
    }

    outgoing
}
```

**关键点：**
- 所有认证头必须原样转发
- 不要修改、去除或重写认证头
- 仅排除 `Host` 和 `Content-Length`（代理会重新计算）

---

### Requirement: SSE 流式支持

The system SHALL正确处理 SSE 流式响应。

**文件：** `src/proxy/client.rs`

#### Scenario: 流式响应透传
- **当** 上游返回 `Content-Type: text/event-stream` 的流式响应时
- **则** The system SHALL立即将每个数据块转发给客户端
- **且** 保持正确的 SSE 帧格式

#### 实现说明

```rust
use futures::StreamExt;
use reqwest::Response;

async fn forward_stream(response: Response) -> impl axum::body::Body {
    let stream = response.bytes_stream();
    axum::body::Body::from_stream(stream)
}
```

**关键点：**
- 不缓冲——立即转发数据块
- 使用 reqwest 的 `bytes_stream()`
- 在响应中保留 `Content-Type: text/event-stream` 头
