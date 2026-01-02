## ADDED Requirements

### Requirement: Anthropic 协议思考注入

The system SHALL为 Anthropic 协议注入思考配置，与 CLIProxyAPI 保持一致。

**文件：** `src/protocol/anthropic.rs`

#### Scenario: 带努力等级的 Anthropic
- **当** 模型带有后缀 `(high)` 且协议为 Anthropic 时
- **且** 模型存在于注册表中并支持思考
- **则** The system SHALL将 `thinking.type` 设为 `enabled`
- **且** 将 `thinking.budget_tokens` 设为 `24576`
- **且** 将 `model` 字段设为基础模型名称
- **且** 确保 `max_tokens` 足够（见下方 max_tokens 调整）

#### Scenario: 带数值预算的 Anthropic
- **当** 模型带有后缀 `(16384)` 且协议为 Anthropic 时
- **且** 模型存在于注册表中并支持思考
- **则** The system SHALL将 `thinking.type` 设为 `enabled`
- **且** 将 `thinking.budget_tokens` 设为 `16384`（钳制到模型范围内）

#### Scenario: 带 none 等级的 Anthropic
- **当** 模型带有后缀 `(none)` 且协议为 Anthropic 时
- **则** The system SHALL不设置任何思考配置
- **且** 原样返回请求体（无 `thinking.type`，无 `thinking.budget_tokens`）

#### Scenario: 带零或负预算的 Anthropic
- **当** 处理后模型的预算 <= 0 时
- **则** The system SHALL不设置任何思考配置

#### Scenario: 未知模型带思考后缀
- **当** 模型带有思考后缀（如 `(high)`、`(16384)`）
- **且** 模型不存在于注册表中
- **则** The system SHALL返回 HTTP 400 错误，说明模型未知

> **⚠️ 设计决策 - 与 CLIProxyAPI 不同：**
> RS-Proxy 要求模型必须在注册表中才能应用思考配置。
> CLIProxyAPI 在注册表查找失败时使用 `budget + 4000` 作为 max_tokens 的回退值。
> RS-Proxy 对未知模型带思考后缀返回错误。
> 这确保了行为可预测，防止错误配置导致的静默失败。

### Requirement: 思考启用时的 max_tokens 调整

The system SHALL确保启用思考时 `max_tokens` 足够。

**文件：** `src/protocol/anthropic.rs`

#### Scenario: max_tokens 调整
- **当** 思考启用且 `budget_tokens > 0` 时
- **且** 模型在注册表中有 `MaxCompletionTokens`
- **则** The system SHALL将 `max_tokens` 设为 `MaxCompletionTokens`（如当前值较低）

> **说明：** 与 CLIProxyAPI 使用 `budget + 4000` 回退不同，RS-Proxy 不需要此回退，
> 因为未知模型在到达此处之前就会被拒绝。

### 实现说明

```rust
// 首先，检查模型是否存在于注册表中
let model_info = registry.get_model_info(&base_model)
    .ok_or_else(|| Error::UnknownModel(base_model.clone()))?;

// 检查模型是否支持思考
if model_info.thinking.is_none() {
    // 模型不支持思考，仅去除括号并转发
    body["model"] = base_model;
    return Ok(body);
}

// 仅当 budget > 0 时应用思考配置
if budget > 0 {
    body["model"] = base_model;
    body["thinking"]["type"] = "enabled";
    body["thinking"]["budget_tokens"] = budget;

    // 将 max_tokens 设为模型的 MaxCompletionTokens
    let current_max = body["max_tokens"].as_i64().unwrap_or(0);
    let required_max = model_info.max_completion_tokens as i64;  // 如 Claude 4.5 为 64000

    if current_max < required_max {
        body["max_tokens"] = required_max;
    }
} else {
    // budget <= 0（包括映射到 0 的 "none" 等级）
    // 不设置任何思考配置，原样返回 body
    body["model"] = base_model;
}
```

**关键点：**
- 等级字符串首先通过映射表转换为预算
- `(none)` 映射到预算 0，意味着不设置思考配置（而非 `budget_tokens = 0`）
- Anthropic API 要求 `max_tokens > thinking.budget_tokens`；违反此规则返回 HTTP 400
- **RS-Proxy 拒绝未知模型带思考后缀（与 CLIProxyAPI 不同）**
