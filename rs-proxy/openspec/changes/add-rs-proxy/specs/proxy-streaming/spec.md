## ADDED Requirements

### Requirement: SSE 流式支持

The system SHALL正确处理 SSE 流式响应。

**文件：** `src/proxy/streaming.rs`

#### Scenario: 流式响应透传
- **当** 上游返回 `Content-Type: text/event-stream` 的流式响应时
- **则** The system SHALL立即将每个数据块转发给客户端
- **且** 保持正确的 SSE 帧格式

### 实现说明

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
