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
pub enum ThinkingValue {
    None,                    // 无后缀或空 ()
    Budget(i32),             // 数值如 16384
    Level(String),           // 等级如 "high"、"auto"、"none"
}

pub struct ParsedModel {
    pub base_name: String,
    pub thinking: ThinkingValue,
}
```
