## ADDED Requirements

### Requirement: Gemini 协议思考注入

The system SHALL为 Gemini 协议注入思考配置。

**文件：** `src/protocol/gemini.rs`

> **注意：** 此模块只负责注入逻辑。模型验证、后缀解析、预算钳制由 `thinking/injector.rs` 完成。
> 此模块接收已处理好的 `ThinkingConfig` 并注入到请求体中：
> - 收到 `ThinkingConfig::Budget` → 注入 Gemini 2.5 格式（thinkingBudget + include_thoughts）
> - 收到 `ThinkingConfig::Effort` → 注入 Gemini 3 格式（thinkingLevel + includeThoughts）
>
> injector 根据模型是否有 `levels` 决定传递哪种类型，本模块不需要再判断模型版本。

#### Scenario: Gemini 2.5 注入（使用数值预算）
- **当** 模型为 Gemini 2.5 变体
- **且** 收到 `ThinkingConfig::Budget(budget)`
- **则** The system SHALL 设置 `generationConfig.thinkingConfig.thinkingBudget` 为预算值
- **且** 若用户未显式设置，将 `include_thoughts` 设为 `true`

#### Scenario: Gemini 3 注入（使用离散等级）
- **当** 模型为 Gemini 3 变体
- **且** 收到 `ThinkingConfig::Effort(level)`
- **则** The system SHALL 设置 `generationConfig.thinkingConfig.thinkingLevel` 为等级字符串
- **且** 若存在 `thinkingBudget` 字段则移除
- **且** 若用户未显式设置，将 `includeThoughts` 设为 `true`（驼峰命名）
- **且** 若存在旧版 `include_thoughts` 字段则移除

#### Scenario: 覆盖用户已设置的值
- **当** 用户请求中已包含 `thinkingBudget` 或 `thinkingLevel`
- **且** 模型名称包含思考后缀
- **则** The system SHALL 用后缀解析的值**覆盖**用户设置的值

### Requirement: 自动设置 include_thoughts

The system SHALL在配置思考时自动设置 include_thoughts。

**文件：** `src/protocol/gemini.rs`

#### Scenario: 自动设置
- **当** 注入思考配置时
- **且** 用户未显式设置 include_thoughts/includeThoughts
- **则** The system SHALL 自动设为 `true`

### 实现说明

```rust
use crate::thinking::ThinkingConfig;

/// 注入 Gemini 思考配置
///
/// 此函数根据 ThinkingConfig 类型决定注入格式：
/// - Budget → Gemini 2.5 格式（thinkingBudget + include_thoughts 蛇形命名）
/// - Effort → Gemini 3 格式（thinkingLevel + includeThoughts 驼峰命名）
///
/// 注意：不需要在此判断模型版本，injector 已根据模型的 levels 字段
/// 决定传递 Budget 还是 Effort 类型。
pub fn inject_gemini(
    mut body: serde_json::Value,
    base_model: &str,
    thinking_config: ThinkingConfig,
) -> serde_json::Value {
    // 更新模型名称（去除后缀）
    body["model"] = serde_json::Value::String(base_model.to_string());

    // 确保 generationConfig.thinkingConfig 存在
    if body.get("generationConfig").is_none() {
        body["generationConfig"] = serde_json::json!({});
    }
    if body["generationConfig"].get("thinkingConfig").is_none() {
        body["generationConfig"]["thinkingConfig"] = serde_json::json!({});
    }

    let thinking_config_obj = &mut body["generationConfig"]["thinkingConfig"];

    match thinking_config {
        ThinkingConfig::Budget(budget) => {
            // Gemini 2.5 格式：使用数值预算
            thinking_config_obj["thinkingBudget"] = serde_json::Value::Number(budget.into());

            // 自动设置 include_thoughts（蛇形命名，Gemini 2.5 风格）
            if thinking_config_obj.get("include_thoughts").is_none() {
                thinking_config_obj["include_thoughts"] = serde_json::Value::Bool(true);
            }
        }
        ThinkingConfig::Effort(level) => {
            // Gemini 3 格式：使用离散等级
            thinking_config_obj["thinkingLevel"] = serde_json::Value::String(level);

            // 移除 thinkingBudget（Gemini 3 不使用）
            if let Some(obj) = thinking_config_obj.as_object_mut() {
                obj.remove("thinkingBudget");
            }

            // 自动设置 includeThoughts（驼峰命名，Gemini 3 风格）
            if thinking_config_obj.get("includeThoughts").is_none() {
                thinking_config_obj["includeThoughts"] = serde_json::Value::Bool(true);
            }

            // 清理旧版蛇形命名字段（Gemini 3 使用驼峰）
            if let Some(obj) = thinking_config_obj.as_object_mut() {
                obj.remove("include_thoughts");
            }
        }
    }

    body
}
```

**关键点：**
- Gemini 2.5 使用 `thinkingBudget`（数值）+ `include_thoughts`（蛇形命名）
- Gemini 3 使用 `thinkingLevel`（字符串）+ `includeThoughts`（驼峰命名）
- injector 负责根据模型版本决定传递 `Budget` 还是 `Effort` 类型
