## ADDED Requirements

### Requirement: 努力等级到预算映射

The system SHALL将努力等级字符串映射到 token 预算，与 CLIProxyAPI 保持一致。

**文件：** `src/thinking/models.rs`

#### Scenario: 标准努力等级
- **当** 努力等级为以下之一：none, auto, minimal, low, medium, high, xhigh
- **则** The system SHALL分别映射到预算：0, -1, 512, 1024, 8192, 24576, 32768

> **注意：** `none`/`0` 和 `auto`/`-1` 的特殊处理在意图层面完成（见 thinking-parser spec），
> 此模块仅提供基础的映射函数。

### Requirement: 预算到努力等级反向映射

The system SHALL将数值预算映射回努力等级字符串（OpenAI 协议需要）。

**文件：** `src/thinking/models.rs`

#### Scenario: 预算到努力等级转换（通用映射，双向对称）
- **当** 需要将数值预算转换为努力等级时（如 OpenAI 协议）
- **则** The system SHALL使用与 `level_to_budget` 双向对称的通用映射：
  - 0 → `"none"`
  - -1 → `"auto"`
  - 1 - 512 → `"minimal"`
  - 513 - 1024 → `"low"`
  - 1025 - 8192 → `"medium"`
  - 8193 - 24576 → `"high"`
  - 24577+ → `"xhigh"`

#### Scenario: 有离散等级列表的模型
- **当** 模型有离散等级列表
- **则** The system SHALL 先使用通用映射，然后通过 `clamp_effort_to_levels` 钳制到模型支持的等级

> **说明：跨协议调用场景**
> 当用户通过 OpenAI 兼容端点（如中转服务）调用 Claude 或 Gemini 2.5 模型时：
> - 这些模型原生使用数值预算，没有离散等级列表
> - 但 OpenAI 协议需要 `reasoning_effort` 字符串
> - 此时使用通用映射表将预算转换为等级字符串
>
> 示例：`claude-sonnet-4(512)` 通过 `/v1/chat/completions` 调用
> → budget = 512 → reasoning_effort = "minimal"

### Requirement: 预算钳制

The system SHALL 将数值预算钳制到模型支持的范围内。

**文件：** `src/thinking/models.rs`

#### Scenario: 预算超出范围
- **当** 输入预算超出模型的 `[min, max]` 范围时
- **则** The system SHALL 钳制到范围边界

#### Scenario: 特殊值处理（在意图层面）

> **⚠️ 设计变更 - 意图分流架构：**
>
> 特殊值 `0`（禁用）和 `-1`（动态）的处理已移至意图层面（`ThinkingIntent`）：
>
> | 后缀 | 意图 | 处理位置 |
> |------|------|----------|
> | `(none)` / `(0)` | `ThinkingIntent::Disabled` | `resolve_intent_to_config()` |
> | `(auto)` / `(-1)` | `ThinkingIntent::Dynamic` | `resolve_intent_to_config()` |
> | 其他 | `ThinkingIntent::Fixed` | `clamp_budget()` / `clamp_effort_to_levels()` |
>
> `clamp_budget()` 仅处理 `Fixed` 意图中的数值预算钳制，
> 不再负责 `0` 和 `-1` 的协议特定转换。

#### Scenario: 预算钳制规则
- **当** 输入预算为 0 且 `zero_allowed == false`
- **则** The system SHALL 返回 `min`
- **当** 输入预算为 -1 且 `dynamic_allowed == false`
- **则** The system SHALL 返回 `auto_budget` 或 `(min + max) / 2`
- **当** 输入预算在 `(0, min)` 范围内
- **则** The system SHALL 返回 `min`
- **当** 输入预算超过 `max`
- **则** The system SHALL 返回 `max`

### Requirement: 等级钳制

The system SHALL 将等级字符串钳制到模型支持的离散等级列表。

**文件：** `src/thinking/models.rs`

#### Scenario: 等级在支持列表中
- **当** 输入等级在模型的离散等级列表中
- **则** The system SHALL 直接返回该等级

#### Scenario: 等级不在支持列表中
- **当** 输入等级不在模型的离散等级列表中
- **则** The system SHALL 向上 clamp 到最近的支持等级
- **若** 向上没有支持的等级
- **则** The system SHALL 返回最高可用等级

#### Scenario: auto 等级处理

> **⚠️ 设计变更 - `auto` 在意图层面处理：**
>
> `auto` 等级在 `to_intent()` 阶段被识别为 `ThinkingIntent::Dynamic`，
> 不会进入 `clamp_effort_to_levels()`。
>
> 如果 `auto` 意外进入此函数（如代码错误），
> 会被当作 `medium` 处理（使用 `LEVEL_ORDER` 中的默认位置 3）。

### 实现说明

```rust
/// 等级优先级顺序（用于 clamp）
const LEVEL_ORDER: &[&str] = &["none", "minimal", "low", "medium", "high", "xhigh"];

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

/// 预算到努力等级（反向映射，与 level_to_budget 双向对称）
///
/// 对于有离散等级列表的模型，返回值应通过 clamp_effort_to_levels 进一步钳制。
pub fn budget_to_effort(budget: i32) -> &'static str {
    match budget {
        0 => "none",
        -1 => "auto",
        1..=512 => "minimal",
        513..=1024 => "low",
        1025..=8192 => "medium",
        8193..=24576 => "high",
        _ if budget > 24576 => "xhigh",
        _ => "medium",  // 负数（除 -1 外）的回退
    }
}

/// 预算钳制
///
/// 将数值预算钳制到模型支持的范围，处理特殊值 0 和 -1。
/// 当 -1 被钳制时，优先使用 `auto_budget`，否则使用 `(min + max) / 2`。
pub fn clamp_budget(
    budget: i32,
    min: i32,
    max: i32,
    zero_allowed: bool,
    dynamic_allowed: bool,
    auto_budget: Option<i32>,
) -> i32 {
    match budget {
        0 if !zero_allowed => min,
        -1 if !dynamic_allowed => auto_budget.unwrap_or((min + max) / 2),
        _ if budget < min && budget > 0 => min,
        _ if budget > max => max,
        _ => budget,
    }
}

/// 等级钳制
///
/// 将等级字符串钳制到模型支持的离散等级列表。
/// 不在列表中的等级向上 clamp 到最近的支持等级。
///
/// 注意："auto" 在意图层面处理，不应进入此函数。
/// 如果进入，会被当作 "medium" 处理（LEVEL_ORDER 默认位置）。
pub fn clamp_effort_to_levels<'a>(effort: &str, levels: &'a [&'a str]) -> &'a str {
    // 如果等级在支持列表中，直接返回
    if levels.contains(&effort) {
        return levels.iter().find(|&&l| l == effort).unwrap();
    }

    // 找到输入等级的位置
    // "auto" 不在 LEVEL_ORDER 中，会得到默认位置 3 (medium)
    let effort_idx = LEVEL_ORDER.iter().position(|&l| l == effort).unwrap_or(3);

    // 向上 clamp：找到第一个 >= 当前等级的支持等级
    for &level in LEVEL_ORDER[effort_idx..].iter() {
        if levels.contains(&level) {
            return levels.iter().find(|&&l| l == level).unwrap();
        }
    }

    // 如果没有更高的，返回最高支持等级
    levels.last().unwrap()
}
```

### 与意图分流架构的关系

```
用户输入后缀
    ↓
to_intent() → ThinkingIntent
    │
    ├─ Disabled (none/0)
    │     → 直接在 resolve_intent_to_config() 处理
    │
    ├─ Dynamic (auto/-1)
    │     → 直接在 resolve_intent_to_config() 处理
    │
    └─ Fixed (level/budget)
          ↓
    ┌─────────────────────────┐
    │ thinking/models.rs      │
    │ ├─ level_to_budget()    │
    │ ├─ budget_to_effort()   │
    │ ├─ clamp_budget()       │
    │ └─ clamp_effort_to_levels() │
    └─────────────────────────┘
          ↓
    ThinkingConfig
```

此模块的函数仅在 `Fixed` 意图下被调用，用于：
1. 验证等级字符串有效性（`level_to_budget`）
2. 数值预算转等级（`budget_to_effort`）
3. 数值预算钳制（`clamp_budget`）
4. 等级钳制（`clamp_effort_to_levels`）
