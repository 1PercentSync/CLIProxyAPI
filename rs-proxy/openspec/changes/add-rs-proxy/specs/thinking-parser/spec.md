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

### 实现说明

解析器返回如下结构体：
```rust
/// 思考配置值类型
#[derive(Debug, Clone, PartialEq)]
pub enum ThinkingValue {
    /// 无后缀或空括号 ()
    None,
    /// 数值预算（如 16384、-1）
    Budget(i32),
    /// 等级字符串（如 "high"、"auto"、"none"）
    Level(String),
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
    // 查找最后一个 '(' 和 ')'
    let open_paren = match model.rfind('(') {
        Some(idx) => idx,
        None => {
            // 无括号，原样返回
            return ParsedModel {
                base_name: model.to_string(),
                thinking: ThinkingValue::None,
            };
        }
    };

    let close_paren = match model.rfind(')') {
        Some(idx) => idx,
        None => {
            // 只有 '(' 没有 ')'，不完整括号，原样透传
            return ParsedModel {
                base_name: model.to_string(),
                thinking: ThinkingValue::None,
            };
        }
    };

    // 检查括号顺序：')' 必须在 '(' 之后且在末尾
    if close_paren <= open_paren || close_paren != model.len() - 1 {
        // 括号顺序错误或 ')' 不在末尾，原样透传
        return ParsedModel {
            base_name: model.to_string(),
            thinking: ThinkingValue::None,
        };
    }

    // 提取基础模型名和后缀内容
    let base_name = model[..open_paren].to_string();
    let suffix = &model[open_paren + 1..close_paren];

    // 空后缀
    if suffix.is_empty() {
        return ParsedModel {
            base_name,
            thinking: ThinkingValue::None,
        };
    }

    // 尝试解析为数值
    if let Ok(budget) = suffix.parse::<i32>() {
        return ParsedModel {
            base_name,
            thinking: ThinkingValue::Budget(budget),
        };
    }

    // 等级字符串
    ParsedModel {
        base_name,
        thinking: ThinkingValue::Level(suffix.to_string()),
    }
}
```
