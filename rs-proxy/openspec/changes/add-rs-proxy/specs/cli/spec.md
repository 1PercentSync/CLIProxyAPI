## ADDED Requirements

### Requirement: 命令行参数解析

The system SHALL使用 argh 接受命令行配置参数。

**文件：** `src/config.rs`

#### Scenario: 默认配置
- **当** rs-proxy 无参数启动时
- **则** 应监听端口 6356
- **且** 使用基础 URL `"cpa.1percentsync.games"`

#### Scenario: 自定义端口
- **当** rs-proxy 以 `-p 8080` 或 `--port 8080` 启动时
- **则** 应监听端口 8080

#### Scenario: 自定义基础 URL
- **当** rs-proxy 以 `-b example.com` 或 `--base-url example.com` 启动时
- **则** 应将请求代理到 `https://example.com`

### 实现说明

```rust
use argh::FromArgs;

#[derive(FromArgs)]
/// RS-Proxy：思考配置注入代理
struct Args {
    #[argh(option, short = 'p', default = "6356")]
    /// 监听端口
    port: u16,

    #[argh(option, short = 'b', default = "String::from(\"cpa.1percentsync.games\")")]
    /// 上游基础 URL
    base_url: String,
}
```
