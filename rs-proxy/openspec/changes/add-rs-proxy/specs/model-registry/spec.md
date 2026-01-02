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

### 模型定义示例

对照 CLIProxyAPI 的 `internal/registry/model_definitions.go`，以下仅为结构示例，实现时应完整复制所有模型定义：

```rust
use std::sync::LazyLock;

static MODELS: LazyLock<Vec<ModelInfo>> = LazyLock::new(|| vec![
    // 示例 1：使用数值预算的模型
    ModelInfo {
        id: "claude-sonnet-4-5-20250929",
        max_completion_tokens: 64000,
        thinking: Some(ThinkingSupport {
            min: 1024,
            max: 100000,
            zero_allowed: false,
            dynamic_allowed: true,
            levels: None,
        }),
    },

    // 示例 2：使用离散等级的模型
    ModelInfo {
        id: "gemini-3-pro-preview",
        max_completion_tokens: 65536,
        thinking: Some(ThinkingSupport {
            min: 128,
            max: 32768,
            zero_allowed: false,
            dynamic_allowed: true,
            levels: Some(&["low", "high"]),
        }),
    },

    // 实现时需包含 CLIProxyAPI 中的所有模型...
]);

pub fn get_model_info(id: &str) -> Option<&'static ModelInfo> {
    MODELS.iter().find(|m| m.id == id)
}

pub fn model_supports_thinking(id: &str) -> bool {
    get_model_info(id).map(|m| m.thinking.is_some()).unwrap_or(false)
}
```

### 维护说明

模型定义需定期对照 CLIProxyAPI 源码更新：
- **参考文件：** `../internal/registry/model_definitions.go`
- **结构体定义：** `../internal/registry/model_registry.go`
- **更新时机：** 当 CLIProxyAPI 添加新模型或修改思考支持配置时

**关键点：**
- 使用 `std::sync::LazyLock` 实现静态初始化（Rust 1.80+）
- 模型 ID 必须与 CLIProxyAPI 完全匹配
- `ThinkingSupport` 字段含义与 Go 版本一致
