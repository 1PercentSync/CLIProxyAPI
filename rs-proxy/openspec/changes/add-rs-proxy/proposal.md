# 变更：添加支持思考配置的 Rust 反向代理

## 原因

用户需要一个轻量级、独立的反向代理，能够从模型名称后缀（如 `model(high)` 或 `model(16384)`）解析思考配置并注入到 API 请求中。这使客户端无需修改请求体即可控制思考/推理行为。

## 变更内容

- 新增 Rust 项目 `rs-proxy` 作为独立反向代理（可转发到任意上游，不仅限于 CLIProxyAPI）
- 实现模型名称后缀解析，与 CLIProxyAPI 的逻辑保持一致
- 手动维护模型注册表，对照 CLIProxyAPI 的 `internal/registry/model_definitions.go` 编写 Rust 模型定义
- 支持 OpenAI、Anthropic 和 Gemini API 协议（协议由 URL 路径决定，`/v1/models` 除外，使用请求头判断）
- 思考配置注入逻辑与 CLIProxyAPI 保持一致（参见 CLIProxyAPI 的 `internal/util/thinking*.go` 和 `internal/runtime/executor/*_executor.go`）
- 为模型列表响应添加思考等级变体（此功能与 CLIProxyAPI 不同，后者不包含变体）
- 正确处理 SSE 流式响应

**注意：** RS-Proxy 不提供协议转换——仅向原始协议格式注入思考配置。

## 设计决策（与 CLIProxyAPI 不同）

RS-Proxy 在以下方面与 CLIProxyAPI 有意不同：

1. **未知模型带思考后缀** → 返回 HTTP 400 错误（非静默回退）
2. **数值预算转等级时的 Clamp** → 向上 clamp 到最近的支持等级
3. **空括号处理** → 去除括号，使用干净的模型名
4. **简化 auto 等级钳制** → 省略 `mid <= 0` 分支
5. **透传策略与覆盖行为** → 统一采用"后缀覆盖用户值"策略
6. **最小干预原则** → 不清理用户请求中的任何字段

详细说明和理由见 `design.md`。

## 影响

- 受影响的规格（新功能）：
  - `cli` → `src/config.rs`
  - `thinking-parser` → `src/thinking/parser.rs`
  - `thinking-mapping` → `src/thinking/models.rs`
  - `thinking-injector` → `src/thinking/injector.rs`
  - `model-registry` → `src/models/registry.rs`
  - `protocol-common` → `src/protocol/mod.rs`
  - `protocol-openai` → `src/protocol/openai.rs`
  - `protocol-anthropic` → `src/protocol/anthropic.rs`
  - `protocol-gemini` → `src/protocol/gemini.rs`
  - `proxy-core` → `src/proxy/client.rs`（头部转发 + SSE 流处理）
  - `models-enhancer` → `src/models/enhancer.rs`
- 受影响的代码：`/rs-proxy` 目录下的新项目
- 依赖：tokio, axum, reqwest, argh, serde_json, tower-http, futures, thiserror
