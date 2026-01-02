## ADDED Requirements

### Requirement: 模型列表增强

The system SHALL为模型列表响应添加思考等级变体。此功能与 CLIProxyAPI 不同，后者不包含变体。

**文件：** `src/models/enhancer.rs`

#### Scenario: OpenAI 模型端点
- **当** 以 OpenAI 请求头向 `/v1/models` 发起 GET 请求时
- **则** The system SHALL转发到上游
- **且** 为每个支持思考的模型添加变体，如 `model(low)`、`model(medium)`、`model(high)`

#### Scenario: Anthropic 模型端点
- **当** 以 Anthropic 请求头向 `/v1/models` 发起 GET 请求时
- **则** The system SHALL转发到上游
- **且** 为响应添加思考变体

#### Scenario: Gemini 模型端点
- **当** 向 `/v1beta/models` 发起 GET 请求时
- **则** The system SHALL转发到上游
- **且** 为支持的模型添加思考变体

### 实现说明

```rust
fn enhance_model_list(models: Vec<Model>, protocol: Protocol) -> Vec<Model> {
    let mut enhanced = models.clone();

    for model in &models {
        if supports_thinking(&model.id) {
            for level in ["low", "medium", "high"] {
                enhanced.push(Model {
                    id: format!("{}({})", model.id, level),
                    ..model.clone()
                });
            }
        }
    }

    enhanced
}
```

**重要说明：** 这是 RS-Proxy 特有的功能。CLIProxyAPI 不会为模型列表添加变体。
