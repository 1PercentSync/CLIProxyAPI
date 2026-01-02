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
- **则** The system SHALL忽略空括号
- **且** 使用 `model-name` 作为模型名称（去除括号）

#### Scenario: 提供商前缀格式
- **当** 请求包含模型名称 `openrouter://gemini-3-pro-preview(high)` 时
- **则** The system SHALL提取基础模型 `openrouter://gemini-3-pro-preview`
- **且** 提取推理努力等级 `high`

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
