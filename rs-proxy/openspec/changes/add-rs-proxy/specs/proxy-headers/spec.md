## ADDED Requirements

### Requirement: 头部转发

The system SHALL透明转发认证和其他请求头。

**文件：** `src/proxy/client.rs`

#### Scenario: Authorization 头
- **当** 请求包含 `Authorization` 头时
- **则** The system SHALL原样转发到上游

#### Scenario: API key 头
- **当** 请求包含 `x-api-key` 头时
- **则** The system SHALL原样转发到上游

### 实现说明

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
