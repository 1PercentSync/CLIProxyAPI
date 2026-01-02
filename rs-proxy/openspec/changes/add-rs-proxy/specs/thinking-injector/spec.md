## ADDED Requirements

### Requirement: 统一思考注入入口

The system SHALL 提供统一的思考配置注入入口，协调解析、验证、映射和注入流程。

**文件：** `src/thinking/injector.rs`

#### Scenario: 注入流程
- **当** 收到包含模型名称的 API 请求时
- **则** The system SHALL 执行以下流程：
  1. 调用 parser 解析模型后缀（如 `model(high)` → `model` + `high`）
  2. **若无后缀或空后缀**：更新模型名（去除括号）后直接透传，不注入
  3. 检查基础模型是否在注册表中
  4. 检查模型是否支持思考配置
  5. 根据后缀类型（等级/数值）进行映射和钳制
  6. 根据协议类型注入对应的思考字段

### Requirement: 未知模型错误处理

The system SHALL 对未知模型带思考后缀返回 HTTP 400 错误。

**文件：** `src/thinking/injector.rs`

#### Scenario: 未知模型带思考后缀
- **当** 模型名称包含思考后缀（如 `unknown-model(high)`）
- **且** 基础模型不在注册表中
- **则** The system SHALL 返回 HTTP 400 错误
- **且** 错误信息应说明模型未知

> **⚠️ 设计决策 - 与 CLIProxyAPI 不同：**
> CLIProxyAPI 允许未知模型使用思考后缀并采用回退行为。
> RS-Proxy 返回错误，确保行为可预测。

#### Scenario: 已知模型不支持思考
- **当** 模型名称包含思考后缀
- **且** 基础模型在注册表中但不支持思考
- **则** The system SHALL 去除后缀并使用基础模型名称
- **且** 不注入任何思考字段

#### Scenario: 空括号处理
- **当** 模型名称包含空括号（如 `model()`）
- **则** The system SHALL 去除空括号
- **且** 使用基础模型名称，不注入思考配置

### Requirement: 透传策略（最小干预）

The system SHALL 对用户请求中已存在的思考配置采用透传策略。

**文件：** `src/thinking/injector.rs`

#### Scenario: 无后缀时透传用户配置
- **当** 模型名称不包含思考后缀
- **且** 用户请求中已包含思考配置（如 `reasoning_effort`、`thinking.budget_tokens` 等）
- **则** The system SHALL 直接透传，不做任何处理

#### Scenario: 有后缀时覆盖用户配置
- **当** 模型名称包含思考后缀
- **且** 用户请求中已包含思考配置
- **则** The system SHALL 用后缀解析的值**覆盖**用户设置的值

> **⚠️ 设计决策 - 与 CLIProxyAPI 不同：**
> CLIProxyAPI 会主动清理不适用的思考字段（如对不支持思考的模型移除 `reasoning_effort`）。
> RS-Proxy 采用"最小干预"策略：
> - 无后缀 → 完全透传，不处理用户已设置的字段
> - 有后缀 → 只覆盖相关字段，不清理其他字段
>
> **理由：**
> - 用户请求中已有思考配置，说明用户有意设置，应尊重用户意图
> - RS-Proxy 是透明代理，不应过度干预请求内容
> - 简化实现，避免复杂的清理逻辑

### Requirement: 协议分发

The system SHALL 根据检测到的**请求协议**类型分发到对应的注入逻辑。

**文件：** `src/thinking/injector.rs`

#### Scenario: 协议特定注入
- **当** 思考配置已解析和映射完成
- **则** The system SHALL 根据协议类型调用对应的注入函数：
  - OpenAI → `protocol/openai.rs`
  - Anthropic → `protocol/anthropic.rs`
  - Gemini → `protocol/gemini.rs`

> **说明：跨协议调用**
> 注入格式由**请求协议**决定，而非模型的原生协议。
> 例如：通过 `/v1/chat/completions`（OpenAI 协议）调用 `claude-sonnet-4(high)`
> → 注入 `reasoning_effort = "high"`，而非 `thinking.budget_tokens`
>
> 这是因为 RS-Proxy 是透明代理，不做协议转换。上游服务（如中转）负责将
> OpenAI 格式转换为模型原生格式。

### Requirement: 数值预算等级钳制

The system SHALL 在数值预算转换为等级后，将等级钳制到模型支持的范围。

**文件：** `src/thinking/injector.rs`

#### Scenario: 转换后等级不在支持列表
- **当** 数值预算（如 `8000`）转换为等级（如 `medium`）
- **且** 模型只支持离散等级（如 `["low", "high"]`）
- **且** 转换后的等级不在支持列表中
- **则** The system SHALL 向上 clamp 到最近的支持等级（如 `medium` → `high`）
- **若** 向上没有支持的等级（如输入 `xhigh` 但模型只支持 `["low", "medium"]`）
- **则** The system SHALL 使用最高可用等级（如 `xhigh` → `medium`）

> **⚠️ 设计决策 - 与 CLIProxyAPI 不同：**
> CLIProxyAPI 不验证转换后的等级，依赖后续 `ValidateThinkingConfig` 返回 400。
> RS-Proxy 主动 clamp，提供更好的用户体验。

### Requirement: 等级到数值预算钳制

The system SHALL 在等级转换为数值预算后，将预算钳制到模型支持的范围。

**文件：** `src/thinking/injector.rs`

#### Scenario: 转换后预算超出模型范围
- **当** 等级（如 `xhigh`）转换为预算（如 `32768`）
- **且** 模型使用数值预算且范围为 `[min, max]`（如 `[128, 16384]`）
- **且** 转换后的预算超出范围
- **则** The system SHALL 钳制到模型范围（如 `32768` → `16384`）

#### Scenario: 特殊值处理
- **当** 等级为 `none`（预算 0）且模型 `zero_allowed == false`
- **则** The system SHALL 钳制到 `min`
- **当** 等级为 `auto`（预算 -1）且模型 `dynamic_allowed == false`
- **则** The system SHALL 钳制到中点 `(min + max) / 2`

### 实现说明

```rust
use crate::models::registry::{get_model_info, model_supports_thinking};
use crate::protocol::Protocol;
use crate::thinking::parser::parse_model_suffix;
use crate::thinking::models::{level_to_budget, budget_to_effort_for_model, clamp_budget};

/// 思考注入结果
pub enum InjectionResult {
    /// 成功注入，返回修改后的请求体
    Injected(serde_json::Value),
    /// 无需注入（模型不支持或无后缀）
    PassThrough(serde_json::Value),
    /// 错误（未知模型带后缀）
    Error(InjectionError),
}

/// 注入错误类型
pub struct InjectionError {
    pub status: u16,
    pub message: String,
}

/// 统一注入入口
pub fn inject_thinking_config(
    body: serde_json::Value,
    model_with_suffix: &str,
    protocol: Protocol,
) -> InjectionResult {
    // 1. 解析后缀
    let (base_model, suffix) = parse_model_suffix(model_with_suffix);

    // 2. 无后缀或空后缀，去除括号并透传
    let suffix = match suffix {
        Some(s) if !s.is_empty() => s,
        _ => {
            // 空括号或无后缀：更新模型名（去除括号）后透传
            let mut body = body;
            body["model"] = serde_json::Value::String(base_model);
            return InjectionResult::PassThrough(body);
        }
    };

    // 3. 检查模型是否已知
    let model_info = match get_model_info(&base_model) {
        Some(info) => info,
        None => return InjectionResult::Error(InjectionError {
            status: 400,
            message: format!("unknown model with thinking suffix: {}", model_with_suffix),
        }),
    };

    // 4. 检查模型是否支持思考
    if model_info.thinking.is_none() {
        // 已知模型但不支持思考，去除后缀透传
        let mut body = body;
        body["model"] = serde_json::Value::String(base_model);
        return InjectionResult::PassThrough(body);
    }

    // 5. 解析后缀值（数值或等级）并钳制
    let thinking_config = resolve_thinking_config(&suffix, model_info, protocol);

    // 6. 根据协议注入
    let injected = match protocol {
        Protocol::OpenAI => inject_openai(body, &base_model, thinking_config),
        Protocol::Anthropic => inject_anthropic(body, &base_model, thinking_config),
        Protocol::Gemini => inject_gemini(body, &base_model, thinking_config),
    };

    InjectionResult::Injected(injected)
}

/// 解析思考配置，处理数值/等级转换和钳制
fn resolve_thinking_config(
    suffix: &str,
    model_info: &ModelInfo,
    protocol: Protocol,
) -> ThinkingConfig {
    let thinking = model_info.thinking.as_ref().unwrap();

    // 判断后缀是数值还是等级
    if let Ok(budget) = suffix.parse::<i32>() {
        // 数值后缀：钳制到模型范围
        let clamped = clamp_budget(budget, thinking.min, thinking.max,
                                   thinking.zero_allowed, thinking.dynamic_allowed);

        // 如果协议需要等级（OpenAI），转换为等级字符串
        if matches!(protocol, Protocol::OpenAI) {
            let effort = budget_to_effort(clamped, thinking.levels);
            // 只有当模型有离散等级时才 clamp
            let final_effort = if let Some(levels) = thinking.levels {
                clamp_effort_to_levels(effort, levels)
            } else {
                effort  // 跨协议调用，直接使用通用映射结果
            };
            ThinkingConfig::Effort(final_effort.to_string())
        } else {
            ThinkingConfig::Budget(clamped)
        }
    } else {
        // 等级后缀
        let level = suffix.to_lowercase();

        // 如果模型使用离散等级，验证或向上 clamp
        if let Some(levels) = thinking.levels {
            let clamped = clamp_effort_to_levels(&level, levels);
            ThinkingConfig::Effort(clamped.to_string())
        } else {
            // 模型使用数值预算，将等级转为预算
            let budget = level_to_budget(&level).unwrap_or(8192); // 默认 medium
            let clamped = clamp_budget(budget, thinking.min, thinking.max,
                                       thinking.zero_allowed, thinking.dynamic_allowed);
            ThinkingConfig::Budget(clamped)
        }
    }
}

/// 向上 clamp 等级到模型支持的等级列表
fn clamp_effort_to_levels<'a>(effort: &str, levels: &'a [&'a str]) -> &'a str {
    // 等级优先级顺序（auto 特殊处理：如果模型支持则保留，否则当作 medium）
    const LEVEL_ORDER: &[&str] = &["none", "minimal", "low", "medium", "high", "xhigh"];

    // auto 特殊处理：如果模型支持 auto 则直接返回，否则当作 medium 处理
    let effort = if effort == "auto" {
        if levels.contains(&"auto") {
            return "auto";
        }
        "medium"  // auto 回退到 medium
    } else {
        effort
    };

    // 如果等级在支持列表中，直接返回
    if levels.contains(&effort) {
        return levels.iter().find(|&&l| l == effort).unwrap();
    }

    // 找到输入等级的位置
    let effort_idx = LEVEL_ORDER.iter().position(|&l| l == effort).unwrap_or(3);

    // 向上 clamp：找到第一个 >= 当前等级的支持等级
    for &level in LEVEL_ORDER[effort_idx..].iter() {
        if levels.contains(&level) {
            return levels.iter().find(|&&l| l == level).unwrap();
        }
    }

    // 如果没有更高的，返回最高支持等级
    levels.last().unwrap()
}
```

### 模块依赖关系

```
thinking/injector.rs
    ├── thinking/parser.rs      (解析后缀)
    ├── thinking/models.rs      (等级/预算映射)
    ├── models/registry.rs      (模型查询)
    └── protocol/*.rs           (协议特定注入)
```
