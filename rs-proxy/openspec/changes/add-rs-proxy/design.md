## 背景

RS-Proxy 是一个独立的轻量级 Rust 反向代理，透明转发 API 请求，同时解析并应用来自模型名称后缀的思考配置。它可转发到任意上游 API 服务器。

**重要说明：** RS-Proxy 不是专门为 CLIProxyAPI 设计的中间件。它是通用代理，其思考配置逻辑与 CLIProxyAPI 保持一致以确保兼容性。

**约束条件：**
- 必须与 CLIProxyAPI 的思考后缀解析和注入逻辑保持一致
- 必须支持 SSE 流式传输且不缓冲
- 模型定义需手动维护，对照 CLIProxyAPI 源码
- 不提供协议转换——仅注入思考配置

**利益相关方：**
- 希望通过模型名称简化思考配置的 API 客户端
- 需要支持思考注入的轻量级本地代理的用户

## 目标 / 非目标

**目标：**
- 解析模型后缀如 `model(high)` 或 `model(16384)`
- 注入协议适配的思考配置
- 支持 SSE 的透明代理
- 为模型列表添加思考变体

**非目标：**
- 认证/授权（仅透明透传）
- 请求缓存
- 负载均衡
- 模型特定的提示词转换

## 决策

### 决策 1：使用 axum 作为 HTTP 框架
- **原因：** 轻量级、异步优先、与 tower 生态系统完美集成
- **备选方案：** actix-web（较重）、warp（不够人性化）、hyper（过于底层）

### 决策 2：静态模型注册表
- **做法：** 在开发时对照 CLIProxyAPI 的 `internal/registry/model_definitions.go` 手动编写 Rust 模型定义
- **原因：** 简化构建流程，无需网络依赖；类型安全；更容易调试和维护
- **备选方案：** 编译时自动获取（增加构建复杂度，需要网络访问，解析可能失败）

### 决策 3：协议检测策略
- **主要方式：** URL 路径决定协议（如 `/v1/messages` → Anthropic，`/v1beta/models/*` → Gemini，`/v1/chat/completions` → OpenAI）
- **例外情况：** `/v1/models` 端点被 OpenAI 和 Anthropic 共用，因此使用请求头判断：`x-api-key` → Anthropic，`Authorization: Bearer` → OpenAI
- **原因：** 最小化请求头检查开销；大多数端点有唯一路径

### 决策 4：透明 SSE 转发
- **原因：** 最小化延迟和复杂度
- **实现方式：** 使用 reqwest 的 `bytes_stream()` 直接转发数据块

### 决策 5：思考注入规则（与 CLIProxyAPI 部分对齐）
协议特定的注入行为与 CLIProxyAPI 实现匹配

**重要说明：**
- 预算值会被钳制到模型支持的范围内
- 使用离散等级的模型，若等级不在支持列表中，向上 clamp 到最近的支持等级（与 CLIProxyAPI 不同，详见 `thinking-injector/spec.md`）

### 决策 6：模型列表增强（与 CLIProxyAPI 不同）
- RS-Proxy 通过添加思考等级变体（如 `model(low)`、`model(high)`）增强 `/v1/models` 响应
- CLIProxyAPI 的模型列表响应不包含这些变体
- 这是有意为之的差异，帮助客户端发现可用的思考配置

### 决策 7：简化 auto 等级钳制逻辑（与 CLIProxyAPI 不同）
- **CLIProxyAPI 行为：** 当模型不支持动态预算时，计算中点 `mid = (min + max) / 2`，并在 `mid <= 0` 时有额外回退逻辑（返回 0 或 min）
- **RS-Proxy 行为：** 直接返回中点，省略 `mid <= 0` 分支
- **原因：**
  1. 当前所有模型定义中 `min + max > 0`，`mid` 永远不会 <= 0
  2. RS-Proxy 要求模型必须在注册表中（未知模型返回 400），可保证模型定义的合理性
  3. 简化实现，避免不可达代码

### 决策 8：空括号处理（与 CLIProxyAPI 不同）
- RS-Proxy 对空括号 `model()` 去除括号，使用 `model` 作为基础模型名
- CLIProxyAPI 对空括号返回原始模型名（含括号）
- 这是有意为之的差异，提供更干净的模型名称

### 决策 9：透传策略与覆盖行为（与 CLIProxyAPI 部分不同）
- **无后缀时：** 完全透传用户请求中已有的思考配置，不做任何处理
- **有后缀时：** 后缀解析的值**统一覆盖**用户设置的值（所有协议）
- **与 CLIProxyAPI 的差异：**
  - CLIProxyAPI 的 `ApplyClaudeThinkingConfig` 在用户已设置 `thinking` 时不覆盖（用户优先）
  - RS-Proxy 统一采用"后缀覆盖用户值"策略
- **原因：**
  1. 简化实现，所有协议采用一致的行为
  2. 后缀是用户明确指定的，应优先于请求体中可能是默认值的设置
  3. 避免复杂的协议特定逻辑

### 决策 10：最小干预原则（与 CLIProxyAPI 不同）
- **CLIProxyAPI 行为：** 主动清理不适用的思考字段（如对不支持思考的模型移除 `reasoning_effort`）
- **RS-Proxy 行为：** 不清理用户请求中的任何字段，仅在有后缀时注入/覆盖
- **原因：**
  1. 用户请求中已有思考配置，说明用户有意设置，应尊重用户意图
  2. RS-Proxy 是透明代理，不应过度干预请求内容
  3. 简化实现，避免复杂的清理逻辑

## 风险 / 权衡

| 风险 | 缓解措施 |
|------|----------|
| 大型流式响应 | 不缓冲，直接透传最小化内存占用 |
| 协议检测歧义 | 明确的基于请求头的规则 |
| 模型定义过时 | 定期对照 CLIProxyAPI 源码更新，记录同步日期 |

## 迁移计划

不适用——新项目，无需迁移。

## 模块架构

### 目录结构

```
src/
├── main.rs                 # 入口点
├── config.rs               # CLI 参数（argh）
├── error.rs                # 错误类型（thiserror）
├── protocol/
│   ├── mod.rs              # Protocol 枚举 + detect_protocol()
│   ├── openai.rs           # inject_openai()
│   ├── anthropic.rs        # inject_anthropic()
│   └── gemini.rs           # inject_gemini()
├── thinking/
│   ├── mod.rs              # ThinkingConfig 枚举 + 模块导出
│   ├── parser.rs           # parse_model_suffix()
│   ├── models.rs           # 等级/预算映射函数
│   └── injector.rs         # inject_thinking_config()
├── models/
│   ├── registry.rs         # 静态模型注册表
│   └── enhancer.rs         # 模型列表增强
└── proxy/
    └── client.rs           # HTTP 客户端 + SSE 转发
```

### 模块依赖关系

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              main.rs                                        │
│                         (axum 路由 + 服务器)                                 │
└─────────────────────────────────┬───────────────────────────────────────────┘
                                  │
                                  ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                           proxy/client.rs                                   │
│                    (HTTP 请求转发 + SSE 流处理)                              │
└─────────────────────────────────┬───────────────────────────────────────────┘
                                  │
          ┌───────────────────────┼───────────────────────┐
          │                       │                       │
          ▼                       ▼                       ▼
┌─────────────────┐   ┌─────────────────────┐   ┌─────────────────┐
│ protocol/mod.rs │   │ thinking/injector.rs│   │models/enhancer.rs│
│                 │   │                     │   │                 │
│ • Protocol 枚举 │   │ • ThinkingConfig    │   │ • 模型列表增强  │
│ • detect_       │   │ • inject_thinking_  │   │                 │
│   protocol()    │   │   config()          │   │                 │
└────────┬────────┘   └──────────┬──────────┘   └────────┬────────┘
         │                       │                       │
         │            ┌──────────┼──────────┐            │
         │            │          │          │            │
         ▼            ▼          ▼          ▼            ▼
┌─────────────────────────────────────────────────────────────────┐
│                     models/registry.rs                          │
│              (ModelInfo, ThinkingSupport, 静态注册表)            │
└─────────────────────────────────────────────────────────────────┘
```

### 公共类型定义位置

| 类型 | 定义位置 | 使用者 |
|------|----------|--------|
| `Protocol` | `protocol/mod.rs` | `thinking/injector.rs`, `models/enhancer.rs`, `proxy/client.rs` |
| `ThinkingConfig` | `thinking/mod.rs` | `thinking/injector.rs`, `protocol/*.rs` |
| `ModelInfo` | `models/registry.rs` | `thinking/injector.rs`, `models/enhancer.rs` |
| `ThinkingSupport` | `models/registry.rs` | `thinking/injector.rs`, `thinking/models.rs` |

### 请求处理流程

```
客户端请求
    │
    ▼
┌─────────────────────────────────────────────────────────────────┐
│ 1. 协议检测 (protocol/mod.rs)                                   │
│    detect_protocol(path, headers) → Protocol                    │
└─────────────────────────────────┬───────────────────────────────┘
                                  │
                                  ▼
┌─────────────────────────────────────────────────────────────────┐
│ 2. 思考注入 (thinking/injector.rs)                              │
│    inject_thinking_config(body, model, protocol, path)          │
│    → InjectionResult::Injected / PassThrough / Error            │
│                                                                 │
│    内部流程：                                                    │
│    ├── parse_model_suffix() → 解析后缀                          │
│    ├── get_model_info() → 查询注册表                            │
│    ├── resolve_thinking_config() → 转换 + 钳制                  │
│    └── inject_{openai,anthropic,gemini}() → 协议特定注入        │
└─────────────────────────────────┬───────────────────────────────┘
                                  │
                                  ▼
┌─────────────────────────────────────────────────────────────────┐
│ 3. 代理转发 (proxy/client.rs)                                   │
│    forward_request(modified_body) → 上游响应                    │
│    forward_stream() → SSE 流式转发                              │
└─────────────────────────────────────────────────────────────────┘
                                  │
                                  ▼
                             客户端响应
```

