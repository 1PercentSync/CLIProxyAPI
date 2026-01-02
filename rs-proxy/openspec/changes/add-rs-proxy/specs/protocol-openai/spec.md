## ADDED Requirements

### Requirement: OpenAI 协议思考注入

The system SHALL为 OpenAI 协议注入思考配置。

**文件：** `src/protocol/openai.rs`

> **注意：** 此模块只负责注入逻辑。模型验证、后缀解析、预算/等级转换和钳制由 `thinking/injector.rs` 完成。
> 此模块接收已处理好的 `ThinkingConfig::Effort(String)` 并注入到请求体中。

#### Scenario: 聊天补全端点注入
- **当** 协议为 OpenAI chat（`/v1/chat/completions`）
- **且** 收到 `ThinkingConfig::Effort` 配置
- **则** The system SHALL 设置 `reasoning_effort` 字段
- **且** 将 `model` 字段设为基础模型名称

#### Scenario: Responses 端点注入
- **当** 协议为 OpenAI Responses（`/v1/responses`）
- **且** 收到 `ThinkingConfig::Effort` 配置
- **则** The system SHALL 设置 `reasoning.effort` 字段
- **且** 将 `model` 字段设为基础模型名称

#### Scenario: 覆盖用户已设置的值
- **当** 用户请求中已包含 `reasoning_effort` 或 `reasoning.effort`
- **且** 模型名称包含思考后缀
- **则** The system SHALL 用后缀解析的值**覆盖**用户设置的值

### 实现说明

```rust
use crate::thinking::ThinkingConfig;

/// 注入 OpenAI 思考配置
/// 前置条件：thinking_config 已由 injector 处理为 Effort 类型
pub fn inject_openai(
    mut body: serde_json::Value,
    base_model: &str,
    thinking_config: ThinkingConfig,
    is_responses_endpoint: bool,
) -> serde_json::Value {
    // 更新模型名称（去除后缀）
    body["model"] = serde_json::Value::String(base_model.to_string());

    // 提取等级字符串
    let effort = match thinking_config {
        ThinkingConfig::Effort(e) => e,
        ThinkingConfig::Budget(_) => {
            // OpenAI 协议不应收到 Budget 类型，injector 应已转换
            unreachable!("OpenAI protocol should receive Effort, not Budget")
        }
    };

    // 根据端点类型注入
    if is_responses_endpoint {
        // /v1/responses 使用嵌套结构
        if body.get("reasoning").is_none() {
            body["reasoning"] = serde_json::json!({});
        }
        body["reasoning"]["effort"] = serde_json::Value::String(effort);
    } else {
        // /v1/chat/completions 使用顶级字段
        body["reasoning_effort"] = serde_json::Value::String(effort);
    }

    body
}
```
