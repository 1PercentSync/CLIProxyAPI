## ADDED Requirements

### Requirement: 静态模型注册表

The system SHALL维护一个静态模型注册表，包含支持思考功能的模型定义。

**文件：** `src/models/registry.rs`

#### Scenario: 模型查找
- **当** 需要查找模型信息时
- **则** The system SHALL从静态注册表中查找模型
- **且** 返回模型的思考支持配置（若有）

#### Scenario: 模型不存在
- **当** 查找的模型 ID 不在注册表中时
- **则** The system SHALL返回 None

### 数据结构

对照 CLIProxyAPI 的 `internal/registry/model_registry.go` 定义 Rust 结构体：

```rust
/// 描述模型的思考/推理预算范围
/// 值以提供商原生 token 单位解释
pub struct ThinkingSupport {
    /// 最小允许思考预算（含）
    pub min: i32,
    /// 最大允许思考预算（含）
    pub max: i32,
    /// 是否允许 0 值（禁用思考）
    pub zero_allowed: bool,
    /// 是否允许 -1 值（动态思考预算）
    pub dynamic_allowed: bool,
    /// 当用户请求 (auto) 或 (-1) 但协议不支持动态时使用的预算值
    /// 设置此值后，(auto) 和 (-1) 会使用此预算而非 (min+max)/2
    pub auto_budget: Option<i32>,
    /// 离散推理等级（如 "low", "medium", "high"）
    /// 设置时，模型使用等级而非 token 预算
    pub levels: Option<&'static [&'static str]>,
}

/// 模型信息
pub struct ModelInfo {
    /// 模型唯一标识符
    pub id: &'static str,
    /// 最大完成 tokens（用于 max_tokens 调整）
    pub max_completion_tokens: i32,
    /// 思考支持配置，None 表示不支持思考
    pub thinking: Option<ThinkingSupport>,
}
```

> **⚠️ 设计决策 - `auto_budget` 字段：**
>
> 新增 `auto_budget: Option<i32>` 字段用于处理以下情况：
>
> 1. **Claude 模型**：API 不支持 `budget_tokens: -1`，需要使用固定预算
>    - 设置 `auto_budget: Some(16384)`
>    - 当用户使用 `(auto)` 或 `(-1)` 时返回 16384
>
> 2. **跨协议模型**（如 `gemini-claude-*`）：
>    - 通过 Gemini API 访问 Claude 模型
>    - Gemini 协议支持 `-1`，但 Anthropic 协议不支持
>    - 设置 `auto_budget: Some(16384)` 供 Anthropic 协议使用
>
> 当 `auto_budget` 未设置时，使用 `(min + max) / 2` 作为回退。

### 模型定义示例

```rust
use std::sync::LazyLock;

static MODELS: LazyLock<Vec<ModelInfo>> = LazyLock::new(|| vec![
    // 示例 1：Claude 模型（不支持动态预算）
    ModelInfo {
        id: "claude-sonnet-4-5-20250929",
        max_completion_tokens: 64000,
        thinking: Some(ThinkingSupport {
            min: 1024,
            max: 100000,
            zero_allowed: false,
            dynamic_allowed: false, // Claude API 不支持 budget_tokens=-1
            auto_budget: Some(16384), // (auto) 和 (-1) 使用此预算
            levels: None,
        }),
    },

    // 示例 2：Gemini 3 模型（支持动态预算，有离散等级）
    ModelInfo {
        id: "gemini-3-pro-preview",
        max_completion_tokens: 65536,
        thinking: Some(ThinkingSupport {
            min: 128,
            max: 32768,
            zero_allowed: false,
            dynamic_allowed: true,
            auto_budget: None, // 不需要，Gemini 协议支持 -1
            levels: Some(&["low", "high"]),
        }),
    },

    // 示例 3：跨协议模型（Claude via Gemini API）
    ModelInfo {
        id: "gemini-claude-opus-4-5-thinking",
        max_completion_tokens: 64000,
        thinking: Some(ThinkingSupport {
            min: 1024,
            max: 200000,
            zero_allowed: false,
            dynamic_allowed: true, // Gemini 协议支持 -1
            auto_budget: Some(16384), // Anthropic 协议使用此值
            levels: None,
        }),
    },
]);

pub fn get_model_info(id: &str) -> Option<&'static ModelInfo> {
    MODELS.iter().find(|m| m.id == id)
}
```

### 跨协议模型说明

部分模型通过非原生协议访问（如 Claude 模型通过 Gemini API）。这些模型的 ID 可能以 `gemini-` 开头但实际是其他供应商的模型。

| 模型 ID | 实际模型 | 访问方式 |
|---------|----------|----------|
| `gemini-claude-sonnet-4-5-thinking` | Claude Sonnet 4.5 | Gemini API |
| `gemini-claude-opus-4-5-thinking` | Claude Opus 4.5 | Gemini API |

这些跨协议模型的特点：
- `dynamic_allowed: true`（Gemini 协议支持 `-1`）
- `auto_budget: Some(16384)`（Anthropic 协议回退值）
- 不被识别为"原生 Gemini 模型"（用于 Disabled 意图处理）

> **注意：原生 Gemini 模型判断**
>
> `thinking/injector.rs` 使用白名单判断模型是否为原生 Gemini：
> ```rust
> const NATIVE_GEMINI_PREFIXES: &[&str] = &[
>     "gemini-2.5-",
>     "gemini-3-",
>     "gemini-pro",
>     "gemini-flash",
> ];
> ```
>
> 这确保 `gemini-claude-*` 等跨协议模型不会被误判。

### 维护说明

模型定义需定期对照 CLIProxyAPI 源码更新：
- **参考文件：** `../internal/registry/model_definitions.go`
- **结构体定义：** `../internal/registry/model_registry.go`
- **更新时机：** 当 CLIProxyAPI 添加新模型或修改思考支持配置时

**关键点：**
- 使用 `std::sync::LazyLock` 实现静态初始化（Rust 1.80+）
- 模型 ID 必须与 CLIProxyAPI 完全匹配
- `ThinkingSupport` 字段含义与 Go 版本一致
- **新增：** `auto_budget` 字段用于动态预算回退

### 模型去重与合并规则

CLIProxyAPI 中的 Gemini 模型分为多个函数，同一模型 ID 可能出现在多个函数中。

**合并规则：**

RS-Proxy 只需保留一份模型定义，按以下优先级合并：

1. **官方 API 为权威来源** - `GetGeminiModels()` 最权威
2. **Vertex 次之** - `GetGeminiVertexModels()`
3. **CLI/AIStudio 补充** - `GetGeminiCLIModels()`、`GetAIStudioModels()` 用于补充缺失字段
