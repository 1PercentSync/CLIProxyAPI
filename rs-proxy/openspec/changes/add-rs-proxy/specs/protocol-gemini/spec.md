## ADDED Requirements

### Requirement: Gemini 协议思考注入

The system SHALL为 Gemini 协议注入思考配置。

**文件：** `src/protocol/gemini.rs`

> **注意：** 此模块只负责注入逻辑。模型验证、后缀解析、预算钳制由 `thinking/injector.rs` 完成。
> 此模块接收已处理好的 `ThinkingConfig` 并注入到请求体中：
> - 收到 `ThinkingConfig::Budget` → 注入 Gemini 2.5 格式（thinkingBudget + include_thoughts）
> - 收到 `ThinkingConfig::Effort` → 注入 Gemini 3 格式（thinkingLevel + includeThoughts）
> - 收到 `ThinkingConfig::Disabled` → **不应发生**（injector 会转换为 Budget）
>
> injector 根据模型是否有 `levels` 决定传递哪种类型，本模块不需要再判断模型版本。

#### Scenario: Gemini 2.5 注入（使用数值预算）
- **当** 模型为 Gemini 2.5 变体
- **且** 收到 `ThinkingConfig::Budget(budget)`
- **则** The system SHALL 设置 `generationConfig.thinkingConfig.thinkingBudget` 为预算值
- **且** 将 `include_thoughts` 设为 `true`

#### Scenario: Gemini 3 注入（使用离散等级）
- **当** 模型为 Gemini 3 变体
- **且** 收到 `ThinkingConfig::Effort(level)`
- **则** The system SHALL 设置 `generationConfig.thinkingConfig.thinkingLevel` 为等级字符串
- **且** 将 `includeThoughts` 设为 `true`（驼峰命名）

#### Scenario: Disabled 意图的处理
- **当** 用户请求禁用思考（`(none)` 或 `(0)`）
- **则** injector 将 Disabled 意图转换为 Budget 类型：
  - Gemini 3（有 levels）→ `Budget(0)`
  - Gemini 2.5（原生）→ `Budget(min)`（如 128）
  - 跨协议模型 → `Budget(0)`
- **因此** `inject_gemini` 不会收到 `ThinkingConfig::Disabled`

#### Scenario: 覆盖用户已设置的值
- **当** 用户请求中已包含思考相关字段
- **且** 模型名称包含思考后缀
- **则** The system SHALL 使用"先清理后设置"模式覆盖所有相关字段

### Requirement: 先清理后设置模式

The system SHALL采用"先清理后设置"模式注入思考配置，但尊重用户的 include_thoughts 设置。

**文件：** `src/protocol/gemini.rs`

#### Scenario: 清理现有思考值字段
- **当** 注入思考配置时
- **则** The system SHALL 移除现有的思考值字段：
  - `thinkingBudget`
  - `thinkingLevel`
- **但** 保留用户设置的 `include_thoughts` / `includeThoughts`

#### Scenario: 尊重用户的 include_thoughts 设置
- **当** 用户已设置 `include_thoughts` 或 `includeThoughts`
- **则** The system SHALL 保留用户的设置值
- **且** 不做版本转换（蛇形 ↔ 驼峰）
- **例如** 用户设置 `include_thoughts: false` + Gemini 3 注入
  - 保留 `include_thoughts: false`
  - 不添加 `includeThoughts`

#### Scenario: 自动设置 include_thoughts
- **当** 用户未设置任何 include_thoughts 变体
- **则** The system SHALL 根据注入格式自动设置：
  - Gemini 2.5 格式 → `include_thoughts: true`
  - Gemini 3 格式 → `includeThoughts: true`

**原因：**
1. 尊重用户对思考输出的控制权
2. 避免不必要的版本转换可能导致的兼容问题
3. 只有思考值（budget/level）需要被后缀覆盖

### 实现说明

```rust
use crate::thinking::ThinkingConfig;

/// 注入 Gemini 思考配置
///
/// 此函数根据 ThinkingConfig 类型决定注入格式：
/// - Budget → Gemini 2.5 格式（thinkingBudget + include_thoughts 蛇形命名）
/// - Effort → Gemini 3 格式（thinkingLevel + includeThoughts 驼峰命名）
/// - Disabled → 不应发生（injector 转换为 Budget）
///
/// 注入遵循"先清理后设置"模式，但尊重用户的 include_thoughts 设置：
/// 1. 移除 thinkingBudget/thinkingLevel 字段
/// 2. 保留用户设置的 include_thoughts/includeThoughts
/// 3. 只在用户未设置时自动添加对应的 include_thoughts 字段
///
/// 对于 Gemini 协议，injector 将 Disabled 意图转换为 Budget：
/// - Gemini 3（有 levels）→ Budget(0)
/// - Gemini 2.5（原生）→ Budget(min)
/// - 跨协议模型 → Budget(0)
pub fn inject_gemini(
    mut body: serde_json::Value,
    base_model: &str,
    thinking_config: ThinkingConfig,
) -> serde_json::Value {
    // 更新模型名称（去除后缀）
    body["model"] = serde_json::Value::String(base_model.to_string());

    // 注意：对于 Gemini 协议，injector 将 Disabled 转换为 Budget(0) 或 Budget(min)
    // 所以这里不应该收到 Disabled。如果收到，优雅地处理（不注入任何配置）
    if matches!(thinking_config, ThinkingConfig::Disabled) {
        return body;
    }

    // 确保 generationConfig.thinkingConfig 存在
    if body.get("generationConfig").is_none() {
        body["generationConfig"] = serde_json::json!({});
    }
    if body["generationConfig"].get("thinkingConfig").is_none() {
        body["generationConfig"]["thinkingConfig"] = serde_json::json!({});
    }

    let thinking_config_obj = &mut body["generationConfig"]["thinkingConfig"];

    // 检查用户是否设置了 include_thoughts 或 includeThoughts
    let user_set_include_thoughts = thinking_config_obj.get("include_thoughts").is_some()
        || thinking_config_obj.get("includeThoughts").is_some();

    // Step 1: 清理思考值字段（但保留 include_thoughts/includeThoughts）
    if let Some(obj) = thinking_config_obj.as_object_mut() {
        obj.remove("thinkingBudget");
        obj.remove("thinkingLevel");
    }

    // Step 2: 根据 config 类型设置新字段
    match thinking_config {
        ThinkingConfig::Disabled => unreachable!("Handled above"),
        ThinkingConfig::Budget(budget) => {
            // Gemini 2.5 格式：数值预算 + 蛇形命名
            thinking_config_obj["thinkingBudget"] = serde_json::Value::Number(budget.into());

            // 只在用户未设置任何变体时才设置 include_thoughts
            if !user_set_include_thoughts {
                thinking_config_obj["include_thoughts"] = serde_json::Value::Bool(true);
            }
        }
        ThinkingConfig::Effort(level) => {
            // Gemini 3 格式：离散等级 + 驼峰命名
            thinking_config_obj["thinkingLevel"] = serde_json::Value::String(level);

            // 只在用户未设置任何变体时才设置 includeThoughts
            if !user_set_include_thoughts {
                thinking_config_obj["includeThoughts"] = serde_json::Value::Bool(true);
            }
        }
    }

    body
}
```

**关键点：**
- Gemini 2.5 使用 `thinkingBudget`（数值）+ `include_thoughts`（蛇形命名）
- Gemini 3 使用 `thinkingLevel`（字符串）+ `includeThoughts`（驼峰命名）
- **禁用思考**：injector 转换为 `Budget(0)` 或 `Budget(min)`，不会传递 `Disabled`
- **尊重用户设置**：保留用户的 `include_thoughts`/`includeThoughts` 值和命名格式
- 只有思考值（budget/level）会被后缀覆盖
- injector 负责根据模型版本决定传递 `Budget` 还是 `Effort` 类型
