## ADDED Requirements

### Requirement: ThinkingConfig 枚举定义

The system SHALL 定义 `ThinkingConfig` 枚举，作为协议间传递思考配置的统一类型。

**文件：** `src/thinking/mod.rs`

> **说明：** `ThinkingConfig` 枚举是 thinking 模块的公共类型，用于：
> - `thinking/injector.rs`：作为 `resolve_thinking_config()` 的返回值
> - `protocol/*.rs`：作为 `inject_*()` 函数的输入参数

```rust
/// 思考配置类型
///
/// 表示经过解析和转换后的思考配置，准备注入到请求体中。
/// 具体类型由目标协议决定：
/// - OpenAI 协议：使用 Effort（等级字符串）
/// - Anthropic 协议：使用 Budget（数值）
/// - Gemini 协议：根据模型版本（2.5 用 Budget，3 用 Effort）
#[derive(Debug, Clone, PartialEq)]
pub enum ThinkingConfig {
    /// 数值预算（tokens），用于 Anthropic 和 Gemini 2.5
    Budget(i32),
    /// 努力等级字符串，用于 OpenAI 和 Gemini 3
    Effort(String),
}
```

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

#### Scenario: 无效等级字符串
- **当** 模型名称包含等级后缀（如 `claude-sonnet-4(superfast)`）
- **且** 等级字符串不是有效值（none, auto, minimal, low, medium, high, xhigh）
- **则** The system SHALL 返回 HTTP 400 错误
- **且** 错误信息应说明等级无效

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

#### Scenario: 模型无预算范围（跨协议调用）
- **当** 模型只定义了离散等级（如 OpenAI 模型），没有 min/max 范围
- **且** 协议需要数值预算（如 Anthropic 协议）
- **则** The system SHALL 直接使用等级映射后的预算值，不做钳制
- **且** 预算值按通用映射：none→0, auto→-1, minimal→512, low→1024, medium→8192, high→24576, xhigh→32768

> **说明：** 这种情况发生在跨协议调用时（如通过 Anthropic 协议调用 OpenAI 模型）。
> 虽然 OpenAI 模型原生只支持等级，但 RS-Proxy 会注入 Anthropic 格式的 budget_tokens。
> 上游服务（中转）负责将其转换为 OpenAI 的 reasoning_effort。

### 实现说明

```rust
use crate::models::registry::{get_model_info, ModelInfo};
use crate::protocol::{Protocol, inject_openai, inject_anthropic, inject_gemini};
use crate::thinking::parser::{parse_model_suffix, ParsedModel, ThinkingValue};
use crate::thinking::models::{level_to_budget, budget_to_effort, clamp_budget, clamp_effort_to_levels};
use crate::thinking::ThinkingConfig;

/// 检测是否为 OpenAI Responses 端点
/// 根据请求路径判断使用哪种字段格式
fn is_responses_endpoint(path: &str) -> bool {
    path.contains("/responses")
}

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
    request_path: &str,  // 用于检测 OpenAI 端点类型
) -> InjectionResult {
    // 1. 解析后缀
    let parsed = parse_model_suffix(model_with_suffix);
    let base_model = parsed.base_name;

    // 2. 无后缀或空后缀，去除括号并透传
    let thinking_value = match parsed.thinking {
        ThinkingValue::None => {
            // 空括号或无后缀：更新模型名（去除括号）后透传
            let mut body = body;
            body["model"] = serde_json::Value::String(base_model);
            return InjectionResult::PassThrough(body);
        }
        v => v,
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
    let thinking_config = match resolve_thinking_config(thinking_value, model_info, protocol) {
        Ok(config) => config,
        Err(e) => return InjectionResult::Error(e),
    };

    // 6. 根据协议注入
    let injected = match protocol {
        Protocol::OpenAI => {
            // OpenAI 需要区分端点类型：chat/completions vs responses
            let is_responses = is_responses_endpoint(request_path);
            inject_openai(body, &base_model, thinking_config, is_responses)
        }
        Protocol::Anthropic => inject_anthropic(body, &base_model, thinking_config, model_info),
        Protocol::Gemini => inject_gemini(body, &base_model, thinking_config),
    };

    InjectionResult::Injected(injected)
}

/// 解析思考配置，处理数值/等级转换和钳制
///
/// 关键设计：返回类型由**协议需求**决定，而非后缀类型或模型原生格式
/// - OpenAI 协议：始终返回 Effort（即使模型原生使用 Budget）
/// - Anthropic 协议：始终返回 Budget（即使模型原生使用 Effort/Levels）
/// - Gemini 协议：根据模型版本（2.5 用 Budget，3 用 Effort）
///
/// 跨协议调用处理：
/// - 模型有 min/max 范围：使用 clamp_budget 钳制
/// - 模型无 min/max 范围（如 OpenAI 模型只有 Levels）：直接使用转换后的值，不钳制
///
/// 错误：
/// - 无效等级字符串返回 InjectionError
fn resolve_thinking_config(
    thinking_value: ThinkingValue,
    model_info: &ModelInfo,
    protocol: Protocol,
) -> Result<ThinkingConfig, InjectionError> {
    let thinking = model_info.thinking.as_ref().unwrap();
    let model_uses_levels = thinking.levels.is_some();

    // 判断模型是否有有效的预算范围（用于 clamp）
    // 如果 max == 0，说明模型没有定义预算范围（只使用 Levels，如 OpenAI 模型）
    let has_budget_range = thinking.max > 0;

    // 确定协议需要的返回类型
    // - OpenAI：始终需要 Effort
    // - Anthropic：始终需要 Budget
    // - Gemini：根据模型是否有 levels（Gemini 2.5 无 levels，Gemini 3 有 levels）
    let needs_effort = match protocol {
        Protocol::OpenAI => true,
        Protocol::Anthropic => false,
        Protocol::Gemini => model_uses_levels,
    };

    match thinking_value {
        ThinkingValue::Budget(budget) => {
            // 数值后缀
            let clamped = if has_budget_range {
                // 模型有预算范围，进行钳制
                clamp_budget(budget, thinking.min, thinking.max,
                             thinking.zero_allowed, thinking.dynamic_allowed)
            } else {
                // 模型没有预算范围（如 OpenAI 模型），直接使用原始值
                budget
            };

            if needs_effort {
                // 协议需要 Effort，将预算转换为等级
                let effort = budget_to_effort(clamped, thinking.levels);
                let final_effort = if let Some(levels) = thinking.levels {
                    // 模型有离散等级，clamp 到支持的等级
                    clamp_effort_to_levels(effort, levels)
                } else {
                    // 跨协议调用（如 OpenAI 调用 Claude），直接使用通用映射结果
                    effort
                };
                Ok(ThinkingConfig::Effort(final_effort.to_string()))
            } else {
                // 协议需要 Budget
                Ok(ThinkingConfig::Budget(clamped))
            }
        }
        ThinkingValue::Level(level) => {
            // 等级后缀
            let level = level.to_lowercase();

            // 验证等级字符串是否有效
            if level_to_budget(&level).is_none() {
                return Err(InjectionError {
                    status: 400,
                    message: format!("invalid thinking level: {}", level),
                });
            }

            if needs_effort {
                // 协议需要 Effort
                if let Some(levels) = thinking.levels {
                    // 模型有离散等级，clamp 到支持的等级
                    let clamped = clamp_effort_to_levels(&level, levels);
                    Ok(ThinkingConfig::Effort(clamped.to_string()))
                } else {
                    // 跨协议调用（如 OpenAI 调用 Claude），直接使用输入的等级
                    Ok(ThinkingConfig::Effort(level))
                }
            } else {
                // 协议需要 Budget，将等级转换为预算（已验证，unwrap 安全）
                let budget = level_to_budget(&level).unwrap();
                let clamped = if has_budget_range {
                    // 模型有预算范围，进行钳制
                    clamp_budget(budget, thinking.min, thinking.max,
                                 thinking.zero_allowed, thinking.dynamic_allowed)
                } else {
                    // 模型没有预算范围（如 OpenAI 模型通过 Anthropic 协议调用）
                    // 直接使用转换后的预算值，让上游服务处理
                    budget
                };
                Ok(ThinkingConfig::Budget(clamped))
            }
        }
        ThinkingValue::None => unreachable!("None case handled earlier"),
    }
}
```

### 模块依赖关系

```
thinking/injector.rs
    ├── thinking/parser.rs      (解析后缀)
    ├── thinking/models.rs      (等级/预算映射)
    ├── models/registry.rs      (模型查询)
    ├── is_responses_endpoint() (本模块内，端点类型检测)
    └── protocol/*.rs           (协议特定注入)
```

### 调用方说明

调用 `inject_thinking_config` 时需要传入 `request_path` 参数：

```rust
// 在代理核心模块中调用
let result = inject_thinking_config(
    body,
    model_name,
    protocol,
    request.uri().path(),  // 传入请求路径，用于 OpenAI 端点类型检测
);
```
