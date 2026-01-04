## ADDED Requirements

### Requirement: 模型名称后缀解析

The system SHALL从模型名称后缀解析思考配置，与 CLIProxyAPI 的 `NormalizeThinkingModel()` 逻辑保持一致。

**文件：** `src/thinking/parser.rs`

#### Scenario: 数值预算后缀
- **当** 请求包含模型名称 `claude-sonnet-4(16384)` 时
- **则** The system SHALL提取基础模型 `claude-sonnet-4`
- **且** 提取思考预算 `16384`（提供商原生 tokens，钳制到模型支持范围）

#### Scenario: 字符串努力等级后缀
- **当** 请求包含模型名称 `gpt-5.1(high)` 时
- **则** The system SHALL提取基础模型 `gpt-5.1`
- **且** 提取推理努力等级 `high`（不区分大小写）

#### Scenario: 无后缀
- **当** 请求包含不带括号的模型名称 `claude-sonnet-4` 时
- **则** The system SHALL原样使用模型名称
- **且** 不注入任何思考配置

#### Scenario: 空括号
- **当** 请求包含模型名称 `model-name()` 时
- **则** The system SHALL 去除空括号
- **且** 使用 `model-name` 作为模型名称
- **且** 不注入任何思考配置

> **⚠️ 设计决策 - 与 CLIProxyAPI 不同：**
> CLIProxyAPI 对空括号返回原始模型名（含括号）。
> RS-Proxy 去除空括号，提供更干净的模型名称。

#### Scenario: 不完整括号
- **当** 请求包含模型名称带有不完整括号（如 `model(high` 或 `model)high`）时
- **则** The system SHALL 原样透传整个模型名称
- **且** 不做任何处理

#### Scenario: 负数预算
- **当** 请求包含模型名称 `model(-1)` 时
- **则** The system SHALL 提取基础模型 `model`
- **且** 提取思考预算 `-1`（表示动态/自动思考预算）

### Requirement: 意图分类

The system SHALL 将解析后的 `ThinkingValue` 转换为用户意图 `ThinkingIntent`。

**文件：** `src/thinking/parser.rs` 和 `src/thinking/mod.rs`

#### Scenario: 禁用思考意图
- **当** 后缀为 `(none)` 或 `(0)` 时
- **则** The system SHALL 返回 `ThinkingIntent::Disabled`

#### Scenario: 动态思考意图
- **当** 后缀为 `(auto)` 或 `(-1)` 时
- **则** The system SHALL 返回 `ThinkingIntent::Dynamic`

#### Scenario: 固定思考意图
- **当** 后缀为其他等级（如 `(high)`）或数值（如 `(16384)`）时
- **则** The system SHALL 返回 `ThinkingIntent::Fixed(FixedThinking::Level|Budget)`

> **设计决策 - 意图分离：**
>
> 将特殊值（`none`, `auto`, `0`, `-1`）的处理提升到意图层面，
> 而非在后续的 `resolve_thinking_config` 中散落多处特殊分支。
> 这使得代码结构更清晰，每种意图有独立的处理路径。

### 实现说明

```rust
/// 思考配置值类型（原始解析结果）
#[derive(Debug, Clone, PartialEq)]
pub enum ThinkingValue {
    /// 无后缀或空括号 ()
    None,
    /// 数值预算（如 16384、-1）
    Budget(i32),
    /// 等级字符串（如 "high"、"auto"、"none"）
    Level(String),
}

impl ThinkingValue {
    /// 将解析后的值转换为用户意图
    ///
    /// 分类规则：
    /// - `(none)` 或 `(0)` → Disabled（禁用思考）
    /// - `(auto)` 或 `(-1)` → Dynamic（动态思考）
    /// - 其他等级或数值 → Fixed（固定思考量）
    ///
    /// 返回 `None` 表示无后缀（`ThinkingValue::None`）
    pub fn to_intent(&self) -> Option<ThinkingIntent> {
        match self {
            ThinkingValue::None => None,
            ThinkingValue::Budget(0) => Some(ThinkingIntent::Disabled),
            ThinkingValue::Budget(-1) => Some(ThinkingIntent::Dynamic),
            ThinkingValue::Budget(b) => Some(ThinkingIntent::Fixed(FixedThinking::Budget(*b))),
            ThinkingValue::Level(level) => {
                let level_lower = level.to_lowercase();
                match level_lower.as_str() {
                    "none" => Some(ThinkingIntent::Disabled),
                    "auto" => Some(ThinkingIntent::Dynamic),
                    _ => Some(ThinkingIntent::Fixed(FixedThinking::Level(level_lower))),
                }
            }
        }
    }
}

/// 用户的思考意图，从解析后的后缀分类而来
///
/// 此中间类型分离用户意图与协议特定的输出格式。
/// 允许在意图层面处理特殊值（`none`, `auto`, `0`, `-1`），
/// 而非在协议转换时散落多处特殊分支。
#[derive(Debug, Clone, PartialEq)]
pub enum ThinkingIntent {
    /// 禁用思考：用户请求 `(none)` 或 `(0)`
    Disabled,

    /// 动态思考：用户请求 `(auto)` 或 `(-1)`
    /// 由 API 决定思考预算
    Dynamic,

    /// 固定思考：用户指定具体的等级或预算
    Fixed(FixedThinking),
}

/// 固定思考值，等级字符串或数值预算
#[derive(Debug, Clone, PartialEq)]
pub enum FixedThinking {
    /// 用户指定等级字符串：`(high)`, `(low)`, `(medium)` 等
    Level(String),

    /// 用户指定数值预算：`(8192)`, `(16384)` 等
    Budget(i32),
}

/// 解析后的模型信息
#[derive(Debug, Clone)]
pub struct ParsedModel {
    /// 基础模型名称（去除后缀）
    pub base_name: String,
    /// 思考配置值
    pub thinking: ThinkingValue,
}

/// 解析模型名称后缀
///
/// 从模型名称中提取基础模型和思考配置值。
///
/// # 示例
/// - `claude-sonnet-4(16384)` → base_name: "claude-sonnet-4", thinking: Budget(16384)
/// - `gpt-5.1(high)` → base_name: "gpt-5.1", thinking: Level("high")
/// - `claude-sonnet-4()` → base_name: "claude-sonnet-4", thinking: None
/// - `claude-sonnet-4` → base_name: "claude-sonnet-4", thinking: None
/// - `model(high` → base_name: "model(high", thinking: None (不完整括号，原样透传)
pub fn parse_model_suffix(model: &str) -> ParsedModel {
    // ... 实现逻辑同原版 ...
}
```
