## ADDED Requirements

### Requirement: OpenAI 协议思考注入

The system SHALL为 OpenAI 协议注入思考配置，与 CLIProxyAPI 保持一致。

**文件：** `src/protocol/openai.rs`

#### Scenario: 带等级后缀的聊天补全
- **当** 模型带有后缀 `(high)` 且协议为 OpenAI chat（`/v1/chat/completions`）时
- **且** 模型存在于注册表中并支持思考
- **则** The system SHALL将 `reasoning_effort` 设为 `high`
- **且** 将 `model` 字段设为基础模型名称

#### Scenario: 带等级后缀的 Responses 端点
- **当** 模型带有后缀 `(high)` 且协议为 OpenAI Responses（`/v1/responses`）时
- **且** 模型存在于注册表中并支持思考
- **则** The system SHALL将 `reasoning.effort` 设为 `high`
- **且** 将 `model` 字段设为基础模型名称

#### Scenario: OpenAI 数值预算
- **当** 模型带有数值后缀 `(16384)` 且协议为 OpenAI 时
- **且** 模型存在于注册表中并支持思考
- **则** The system SHALL使用反向映射将数值预算转换为等级字符串：
  - 1 - 1024 → `"low"`
  - 1025 - 8192 → `"medium"`
  - 8193 - 24576 → `"high"`
  - 24577+ → 最高支持等级（默认 `"xhigh"`）
- **且** 设置 `reasoning_effort`（chat）或 `reasoning.effort`（responses）为转换后的等级
- **且** 将 `model` 字段设为基础模型名称

#### Scenario: 未知模型带思考后缀
- **当** 模型带有思考后缀（如 `(high)`、`(16384)`）
- **且** 模型不存在于注册表中
- **则** The system SHALL返回 HTTP 400 错误，说明模型未知

> **⚠️ 设计决策 - 与 CLIProxyAPI 不同：**
> RS-Proxy 要求模型必须在注册表中才能应用思考配置。
> 详见 thinking-mapping/spec.md 了解此设计决策的完整说明。

### 实现说明

**关键点：** OpenAI 协议的 `reasoning_effort` 仅接受等级字符串，不接受数值预算。数值预算必须先转换为等级。

```rust
// 预算到等级转换（反向映射）
fn budget_to_effort(budget: i32) -> &'static str {
    match budget {
        1..=1024 => "low",
        1025..=8192 => "medium",
        8193..=24576 => "high",
        _ if budget > 24576 => "xhigh",
        _ => "medium", // 回退
    }
}

// 对于 /v1/chat/completions
body["model"] = base_model;
let level = match thinking {
    ThinkingValue::Level(l) => l,
    ThinkingValue::Budget(b) => budget_to_effort(b),
};
body["reasoning_effort"] = level;

// 对于 /v1/responses
body["model"] = base_model;
body["reasoning"]["effort"] = level;
```
