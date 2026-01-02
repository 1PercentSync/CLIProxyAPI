## 1. 项目设置

- [ ] 1.1 创建 `Cargo.toml` 并添加依赖（使用最新兼容版本）：
  - tokio (rt-multi-thread, macros, net, io-util, sync)
  - axum
  - reqwest (json, stream)
  - argh
  - serde (derive)
  - serde_json
  - tower-http (cors, trace)
  - futures
  - tokio-stream
  - tracing, tracing-subscriber
  - thiserror

## 2. 核心基础设施

- [ ] 2.1 使用 argh 实现 CLI 参数解析（main.rs、config.rs）
  - 使用 `#[argh(option, default = "...")]` 设置默认值
  - `-b/--base-url`（默认：`"cpa.1percentsync.games"`）
  - `-p/--port`（默认：6356）
- [ ] 2.2 使用 thiserror 定义自定义错误类型（error.rs）
  - 包装 reqwest::Error、serde_json::Error、std::io::Error
  - 使用 `#[from]` 实现自动转换
- [ ] 2.3 实现代理核心（proxy/client.rs）
  - HTTP 客户端封装与连接池
  - 头部转发
  - SSE 流处理（将上游字节转发到下游）

## 3. 模型注册表

- [ ] 3.1 定义 Rust 数据结构（models/registry.rs）
  - `ThinkingSupport` 结构体（min, max, zero_allowed, dynamic_allowed, levels）
  - `ModelInfo` 结构体（id, max_completion_tokens, thinking）
- [ ] 3.2 对照 CLIProxyAPI 的 `internal/registry/model_definitions.go` 编写模型定义
  - Claude 模型（claude-sonnet-4-5, claude-opus-4-5, 等）
  - Gemini 模型（gemini-2.5-pro, gemini-3-pro-preview, 等）
  - OpenAI 模型（gpt-5, gpt-5.1, gpt-5.2, 等）
  - iFlow 模型（glm-4.6, glm-4.7, minimax-m2.1）
- [ ] 3.3 实现模型查找函数
  - `get_model_info(id: &str) -> Option<&ModelInfo>`
  - `model_supports_thinking(id: &str) -> bool`

## 4. 思考配置

- [ ] 4.1 实现模型后缀解析器（thinking/parser.rs）
  - 解析 `model(value)` 模式
  - 检测数值与字符串值
- [ ] 4.2 实现努力等级到预算的映射（thinking/models.rs）
  - none→0, auto→-1, minimal→512, low→1024, medium→8192, high→24576, xhigh→32768
- [ ] 4.3 实现思考注入器（thinking/injector.rs）
  - 协议特定的注入逻辑

## 5. 协议处理器

- [ ] 5.1 实现 OpenAI 处理器（protocol/openai.rs）
  - `/v1/chat/completions`、`/v1/responses`
  - 设置 `reasoning_effort` 字段（仅等级，非数值）
- [ ] 5.2 实现 Anthropic 处理器（protocol/anthropic.rs）
  - `/v1/messages`
  - 设置 `thinking.type` + `thinking.budget_tokens`
- [ ] 5.3 实现 Gemini 处理器（protocol/gemini.rs）
  - `/v1beta/models/*`
  - Gemini 2.5: 设置 `thinkingBudget`
  - Gemini 3: 设置 `thinkingLevel`

## 6. 模型列表增强

- [ ] 6.1 实现基于请求头的协议检测（protocol/mod.rs，仅用于 /v1/models）
  - `x-api-key` → Anthropic
  - `Authorization: Bearer` → OpenAI
- [ ] 6.2 实现模型列表增强器（models/enhancer.rs）
  - 为支持的模型添加思考等级变体

## 7. 请求路由

- [ ] 7.1 设置包含所有端点的 axum 路由器
- [ ] 7.2 添加 CORS 和追踪中间件

## 8. 测试与完善

- [ ] 8.1 使用 tracing 添加结构化日志
- [ ] 8.2 优雅处理错误情况
- [ ] 8.3 使用真实 API 调用进行测试

