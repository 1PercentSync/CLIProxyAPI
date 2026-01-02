## ADDED Requirements

### Requirement: 努力等级到预算映射

The system SHALL将努力等级字符串映射到 token 预算，与 CLIProxyAPI 保持一致。

**文件：** `src/thinking/models.rs`

#### Scenario: 标准努力等级
- **当** 努力等级为以下之一：none, auto, minimal, low, medium, high, xhigh
- **则** The system SHALL分别映射到预算：0, -1, 512, 1024, 8192, 24576, 32768
- **且** 应用钳制规则：
  - `none` → 0（若模型不允许 0 则钳制到 min）
  - `auto` → -1（若模型不支持动态则钳制到中点，见下方说明）
  - 其他等级 → 对应预算值，钳制到 [min, max] 范围

> **注意：** 未知模型的处理由 `thinking/injector.rs` 负责（返回 HTTP 400），
> 此模块仅处理已知模型的映射和钳制逻辑。

#### Scenario: auto 等级钳制（模型不支持动态预算）
- **当** 努力等级为 `auto`
- **且** 模型的 `DynamicAllowed == false`
- **则** The system SHALL 返回中点值 `(min + max) / 2`

> **⚠️ 设计决策 - 与 CLIProxyAPI 不同：**
> CLIProxyAPI 在 `mid <= 0` 时有额外的回退逻辑（返回 0 或 min）。
> RS-Proxy 省略此分支，因为：
> 1. 当前所有模型定义中 `min + max > 0`，`mid` 永远不会 <= 0
> 2. RS-Proxy 要求模型必须在注册表中，可保证模型定义的合理性
> 3. 简化实现，避免不可达代码

### Requirement: 预算到努力等级反向映射

The system SHALL将数值预算映射回努力等级字符串（OpenAI 协议需要）。

**文件：** `src/thinking/models.rs`

#### Scenario: 预算到努力等级转换
- **当** 需要将数值预算转换为努力等级时（如 OpenAI）
- **则** The system SHALL使用以下范围：
  - 0 → 若模型有离散等级则返回 `levels[0]`（最低等级）；否则返回 `"none"`
  - -1 → `"auto"`
  - 1 - 1024 → `"low"`
  - 1025 - 8192 → `"medium"`
  - 8193 - 24576 → `"high"`
  - 24577+ → 若模型有离散等级则返回 `levels[last]`（最高等级）；否则返回 `"xhigh"`

### Requirement: Gemini thinkingLevel 专用映射

The system SHALL 为 Gemini 协议提供专用的 thinkingLevel 到预算映射。

**文件：** `src/thinking/models.rs`

#### Scenario: Gemini thinkingLevel 转换
- **当** 需要将 Gemini 的 thinkingLevel 转换为预算时
- **则** The system SHALL使用以下映射（注意 high 的值与通用映射不同）：
  - `"minimal"` → 512
  - `"low"` → 1024
  - `"medium"` → 8192
  - `"high"` → **32768**（不是 24576）

> **⚠️ 注意：** Gemini 的 `high` 等级映射到 32768，与通用的 24576 不同。
> 这是因为 Gemini 3 模型的 thinkingLevel 使用不同的预算范围。

### Requirement: 支持思考的模型的思考注入

The system SHALL仅为声明支持思考的模型注入思考配置。

**文件：** `src/thinking/models.rs`

#### Scenario: 注册表中不支持思考的模型
- **当** 模型存在于注册表中但未声明思考支持
- **且** 带有后缀 `(high)`
- **则** The system SHALL去除括号并使用基础模型名称
- **且** 不注入任何思考字段

#### Scenario: 不在注册表中的模型带思考后缀
- **当** 模型带有思考后缀（如 `(high)`、`(16384)`）
- **且** 模型不存在于注册表中
- **则** The system SHALL返回 HTTP 400 错误，说明模型未知

> **⚠️ 设计决策 - 与 CLIProxyAPI 不同：**
> RS-Proxy 要求模型必须在注册表中才能应用思考配置。
> CLIProxyAPI 允许未知模型使用思考后缀并采用回退行为。
> RS-Proxy 返回错误，确保行为可预测，防止错误配置导致的静默失败
>（如错误的 max_tokens 值）。

### Requirement: 离散等级验证

The system SHALL验证使用离散等级的模型的努力等级。

**文件：** `src/thinking/models.rs`

#### Scenario: 离散模型使用无效等级
- **当** 模型使用离散等级且后缀包含不支持的等级时
- **则** The system SHALL返回 HTTP 400 错误

### 实现说明

```rust
/// 等级到预算（正向映射 - 通用）
pub fn level_to_budget(level: &str) -> Option<i32> {
    match level.to_lowercase().as_str() {
        "none" => Some(0),
        "auto" => Some(-1),
        "minimal" => Some(512),
        "low" => Some(1024),
        "medium" => Some(8192),
        "high" => Some(24576),
        "xhigh" => Some(32768),
        _ => None,
    }
}

/// Gemini thinkingLevel 到预算（Gemini 专用）
/// 注意：high 映射到 32768，与通用映射不同
pub fn gemini_level_to_budget(level: &str) -> Option<i32> {
    match level.to_lowercase().as_str() {
        "minimal" => Some(512),
        "low" => Some(1024),
        "medium" => Some(8192),
        "high" => Some(32768),  // 注意：与通用的 24576 不同
        _ => None,
    }
}

/// 预算到努力等级（反向映射，OpenAI 协议需要）
pub fn budget_to_effort(budget: i32, model_levels: Option<&[String]>) -> &'static str {
    match budget {
        0 => {
            // 若模型有离散等级，返回最低等级；否则返回 "none"
            if let Some(levels) = model_levels {
                if !levels.is_empty() {
                    return levels[0].as_str();
                }
            }
            "none"
        }
        -1 => "auto",
        1..=1024 => "low",
        1025..=8192 => "medium",
        8193..=24576 => "high",
        _ if budget > 24576 => {
            // 若模型有离散等级，返回最高等级；否则返回 "xhigh"
            if let Some(levels) = model_levels {
                if !levels.is_empty() {
                    return levels[levels.len() - 1].as_str();
                }
            }
            "xhigh"
        }
        _ => "medium",  // 意外值的回退
    }
}

/// 带模型感知的预算到努力等级
pub fn budget_to_effort_for_model(model: &str, budget: i32) -> String {
    let levels = get_model_thinking_levels(model);
    budget_to_effort(budget, levels.as_deref()).to_string()
}
```

---

## CLIProxyAPI 待跟进问题

> 以下问题需要在 CLIProxyAPI 中确认或修复，RS-Proxy 已采用不同的处理方式。

### 问题：数值预算转等级后未验证等级有效性

- **位置：** `internal/util/thinking.go` 的 `ThinkingBudgetToEffort` 函数
- **问题：** 数值预算（如 `8000`）转换为等级（如 `medium`）后，未检查该等级是否在模型支持列表中。若模型只支持 `["low", "high"]`，则 `medium` 无效，最终由 `ValidateThinkingConfig` 返回 400。
- **RS-Proxy 处理：** 向上 clamp 到最近的支持等级（如 `medium` → `high`）
- **建议：** CLIProxyAPI 可考虑采用相同的 clamp 逻辑，或明确返回更具体的错误信息
