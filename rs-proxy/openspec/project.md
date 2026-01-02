# 项目上下文

## 目的

RS-Proxy 是一个独立的轻量级 Rust 反向代理，可独立运行并对接任意上游服务器。它从模型名称后缀（如 `model(high)` 或 `model(16384)`）解析思考配置，并将相应配置注入 API 请求，使客户端无需修改请求体即可控制思考/推理行为。

**重要说明：**
- RS-Proxy 不是专门为 CLIProxyAPI 设计的中间件——它是通用代理，可转发到任意上游 API 服务器
- RS-Proxy 的思考配置解析和注入逻辑必须与 CLIProxyAPI 保持一致
- RS-Proxy 不提供协议转换——仅向原始协议格式注入思考配置
- RS-Proxy 会为 `/v1/models` 响应添加思考等级变体（CLIProxyAPI 不包含变体）

## 技术栈

- **语言：** Rust（最新稳定版）
- **运行时：** Tokio（异步运行时）
- **HTTP 服务器：** Axum
- **HTTP 客户端：** Reqwest
- **命令行：** Argh
- **JSON：** Serde + Serde_json
- **中间件：** Tower-http
- **流处理：** Futures + Tokio-stream
- **正则：** Regex
- **错误处理：** Thiserror
- **日志：** Tracing + Tracing-subscriber

## 项目规范

### 代码风格

- 遵循 Rust 标准格式化（`cargo fmt`）
- 使用 `clippy` 进行代码检查，采用默认规则
- 生产代码优先显式错误处理，避免 `.unwrap()`
- 使用 `thiserror` 定义自定义错误类型
- 使用 rustdoc 注释记录公共 API

### 架构模式

- **模块化结构：** 关注点分离到 `proxy/`、`thinking/`、`protocol/`、`models/` 模块
- **静态模型注册表：** 在开发时对照 CLIProxyAPI 的 `internal/registry/model_definitions.go` 手动维护 Rust 模型定义
- **透明代理：** 最小化请求/响应修改，仅注入思考配置
- **协议特定处理器：** 每种 API 协议（OpenAI、Anthropic、Gemini）有独立的转换逻辑

### 测试策略

- 针对解析和转换逻辑的单元测试
- 使用模拟上游服务器的集成测试
- 对真实 API 端点的手动测试

### Git 工作流

- 从 `main` 分支创建功能分支
- 约定式提交：`feat:`、`fix:`、`refactor:`、`docs:`、`test:`
- 基于 PR 的合并，需代码审查

## 领域上下文

### 思考配置

思考/推理是现代大语言模型的一项特性，允许在响应前进行扩展"思考"。RS-Proxy 注入的思考配置与 CLIProxyAPI 的行为保持一致。

**协议检测：**
- URL 路径决定大多数端点的协议（如 `/v1/messages` → Anthropic，`/v1beta/models/*` → Gemini）
- 请求头（`x-api-key` vs `Authorization: Bearer`）仅用于区分共享的 `/v1/models` 端点的协议

**注入规则（与 CLIProxyAPI 对齐）：**
- 仅影响注册表中存在且支持思考的模型
- **未知模型带思考后缀：** 返回 HTTP 400 错误（见下方设计决策）
- **已知：** 采用和CLIProxyAPI同样的规则

### 模型名称后缀语法

用户在模型名后附加 `(value)`，其中 value 可以是：
- 数值预算：`claude-sonnet-4(16384)` → 16384 tokens（提供商原生 tokens，钳制到模型支持范围）
- 努力等级：`gpt-5.1(high)` → high 等级（不区分大小写）
- 空括号 `()` 会被移除并忽略

## 重要约束

- **兼容性：** 必须与 CLIProxyAPI 的 Thinking 解析逻辑完全匹配
- **性能：** SSE 流式传输不得缓冲；立即转发数据块
- **透明性：** 所有头部（尤其是认证头）必须原样转发
- **模型同步：** 需定期对照 CLIProxyAPI 源码更新模型定义

## 外部依赖

### 上游服务

- **CLIProxyAPI：** 主要上游服务器（默认：`cpa.1percentsync.games`）

### 模型定义参考

开发时需参考以下 CLIProxyAPI 源文件维护 Rust 模型定义：
- `internal/registry/model_definitions.go` - 包含所有静态模型定义及 `ThinkingSupport` 配置
- `internal/registry/model_registry.go` - 包含 `ModelInfo` 和 `ThinkingSupport` 结构体定义

## 设计决策（与 CLIProxyAPI 不同）

### 未知模型带思考后缀的处理

**CLIProxyAPI 行为：** 允许未知模型使用思考后缀

**RS-Proxy 行为：** 对未知模型带思考后缀返回 HTTP 400 错误

**理由：**
- 确保行为可预测——不会静默失败并产生错误配置
- 防止发送错误的 `max_tokens` 值导致上游错误
- 强制用户使用已知的、支持思考特性的模型
- 无需回退逻辑，简化实现

### 数值预算转等级时的 Clamp 行为

**CLIProxyAPI 行为：** 数值预算转换为等级后，不验证该等级是否在模型支持列表中（依赖后续验证返回 400）

**RS-Proxy 行为：** 数值预算转换为等级后，若该等级不在模型支持列表中，向上 clamp 到最近的支持等级

**理由：**
- 提供更好的用户体验——尽可能满足用户的推理深度意图
- 向上 clamp 确保推理能力不低于用户预期
- 避免因等级不匹配而返回 400 错误