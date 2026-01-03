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

> **说明：** `base_url` 参数不包含协议前缀，系统在构造实际请求 URL 时自动添加 `https://`。

### 实现说明

```rust
use argh::FromArgs;

fn default_port() -> u16 {
    6356
}

fn default_base_url() -> String {
    String::from("cpa.1percentsync.games")
}

#[derive(FromArgs)]
/// RS-Proxy：思考配置注入代理
struct Args {
    #[argh(option, short = 'p', default = "default_port()")]
    /// 监听端口
    port: u16,

    #[argh(option, short = 'b', default = "default_base_url()")]
    /// 上游基础 URL
    base_url: String,
}
```
