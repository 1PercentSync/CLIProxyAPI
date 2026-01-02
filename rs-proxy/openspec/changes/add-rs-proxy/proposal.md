# 变更：添加支持思考配置的 Rust 反向代理

## 原因

用户需要一个轻量级、独立的反向代理，能够从模型名称后缀（如 `model(high)` 或 `model(16384)`）解析思考配置并注入到 API 请求中。这使客户端无需修改请求体即可控制思考/推理行为。

## 变更内容

- 新增 Rust 项目 `rs-proxy` 作为独立反向代理（可转发到任意上游，不仅限于 CLIProxyAPI）
- 实现模型名称后缀解析，与 CLIProxyAPI 的逻辑保持一致
- 手动维护模型注册表，对照 CLIProxyAPI 的 `internal/registry/model_definitions.go` 编写 Rust 模型定义
- 支持 OpenAI、Anthropic 和 Gemini API 协议（协议由 URL 路径决定，`/v1/models` 除外，使用请求头判断）
- 思考配置注入与 CLIProxyAPI 行为对齐：
  - **OpenAI/Codex/Qwen/iFlow/OpenRouter：** `reasoning_effort`（chat）或 `reasoning.effort`（Responses）；数值预算转换为等级字符串
  - **Anthropic：** `thinking.type=enabled` + `thinking.budget_tokens`；`max_tokens` 设为模型的 `MaxCompletionTokens`；`(none)` 不设置思考配置
  - **Gemini 2.5：** `thinkingBudget`（数值）+ 自动设置 `include_thoughts=true`
  - **Gemini 3：** `thinkingLevel`（字符串：low/medium/high）+ 自动设置 `includeThoughts=true`
- 为模型列表响应添加思考等级变体（此功能与 CLIProxyAPI 不同，后者不包含变体）
- 正确处理 SSE 流式响应

**注意：** RS-Proxy 不提供协议转换——仅向原始协议格式注入思考配置。

## 设计决策（与 CLIProxyAPI 不同）

### 未知模型带思考后缀的处理

- **CLIProxyAPI：** 允许未知模型使用思考后缀，采用回退行为
- **RS-Proxy：** 对未知模型带思考后缀返回 HTTP 400 错误

**理由：** 确保行为可预测，防止错误配置导致的静默失败。

## 影响

- 受影响的规格（新功能）：
  - `cli` → `src/config.rs`
  - `thinking-parser` → `src/thinking/parser.rs`
  - `thinking-mapping` → `src/thinking/models.rs`
  - `model-registry` → `src/models/registry.rs`
  - `protocol-detection` → `src/protocol/mod.rs`
  - `protocol-openai` → `src/protocol/openai.rs`
  - `protocol-anthropic` → `src/protocol/anthropic.rs`
  - `protocol-gemini` → `src/protocol/gemini.rs`
  - `proxy-streaming` → `src/proxy/streaming.rs`
  - `proxy-headers` → `src/proxy/client.rs`
  - `models-enhancer` → `src/models/enhancer.rs`
- 受影响的代码：`/rs-proxy` 目录下的新项目
- 依赖：tokio, axum, reqwest, argh, serde_json, tower-http, futures, thiserror
