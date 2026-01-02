## ADDED Requirements

### Requirement: Anthropic 协议思考注入

The system SHALL为 Anthropic 协议注入思考配置。

**文件：** `src/protocol/anthropic.rs`

> **注意：** 此模块只负责注入逻辑。模型验证、后缀解析、预算钳制由 `thinking/injector.rs` 完成。
> 此模块接收已处理好的 `ThinkingConfig::Budget(i32)` 并注入到请求体中。

#### Scenario: 正预算注入
- **当** 收到 `ThinkingConfig::Budget(budget)` 且 `budget > 0`
- **则** The system SHALL 设置 `thinking.type` 为 `"enabled"`
- **且** 设置 `thinking.budget_tokens` 为预算值
- **且** 将 `model` 字段设为基础模型名称
- **且** 调整 `max_tokens`（见下方）

#### Scenario: 零或负预算
- **当** 收到 `ThinkingConfig::Budget(budget)` 且 `budget <= 0`
- **则** The system SHALL 不设置任何思考配置
- **且** 仅将 `model` 字段设为基础模型名称

#### Scenario: 覆盖用户已设置的值
- **当** 用户请求中已包含 `thinking.type` 或 `thinking.budget_tokens`
- **且** 模型名称包含思考后缀
- **则** The system SHALL 用后缀解析的值**覆盖**用户设置的值

> **⚠️ 设计决策 - 与 CLIProxyAPI 不同：**
> CLIProxyAPI 的 `ApplyClaudeThinkingConfig` 在用户已设置 `thinking` 时不覆盖（用户优先）。
> RS-Proxy 统一采用"后缀覆盖用户值"策略，简化处理逻辑。

### Requirement: 思考启用时的 max_tokens 调整

The system SHALL确保启用思考时 `max_tokens` 足够。

**文件：** `src/protocol/anthropic.rs`

#### Scenario: max_tokens 调整
- **当** 思考启用且 `budget_tokens > 0` 时
- **则** The system SHALL 将 `max_tokens` 设为模型的 `MaxCompletionTokens`（如当前值较低或未设置）

> **说明：** Anthropic API 要求 `max_tokens > thinking.budget_tokens`。

### 实现说明

```rust
use crate::thinking::ThinkingConfig;
use crate::models::registry::ModelInfo;

/// 注入 Anthropic 思考配置
/// 前置条件：thinking_config 已由 injector 处理为 Budget 类型
pub fn inject_anthropic(
    mut body: serde_json::Value,
    base_model: &str,
    thinking_config: ThinkingConfig,
    model_info: &ModelInfo,
) -> serde_json::Value {
    // 更新模型名称（去除后缀）
    body["model"] = serde_json::Value::String(base_model.to_string());

    // 提取预算值
    let budget = match thinking_config {
        ThinkingConfig::Budget(b) => b,
        ThinkingConfig::Effort(_) => {
            // Anthropic 协议不应收到 Effort 类型，injector 应已转换
            unreachable!("Anthropic protocol should receive Budget, not Effort")
        }
    };

    // 仅当 budget > 0 时应用思考配置
    if budget > 0 {
        // 设置思考配置
        if body.get("thinking").is_none() {
            body["thinking"] = serde_json::json!({});
        }
        body["thinking"]["type"] = serde_json::Value::String("enabled".to_string());
        body["thinking"]["budget_tokens"] = serde_json::Value::Number(budget.into());

        // 调整 max_tokens
        let current_max = body.get("max_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let required_max = model_info.max_completion_tokens as i64;

        if current_max < required_max {
            body["max_tokens"] = serde_json::Value::Number(required_max.into());
        }
    }
    // budget <= 0：不设置任何思考配置，仅返回更新了 model 的 body

    body
}
```
