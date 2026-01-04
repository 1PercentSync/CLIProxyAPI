## ADDED Requirements

### Requirement: ThinkingConfig 枚举定义

The system SHALL 定义 `ThinkingConfig` 枚举，作为协议间传递思考配置的统一类型。

**文件：** `src/thinking/mod.rs`

> **说明：** `ThinkingConfig` 枚举是 thinking 模块的公共类型，用于：
> - `thinking/injector.rs`：作为 `resolve_intent_to_config()` 的返回值
> - `protocol/*.rs`：作为 `inject_*()` 函数的输入参数

```rust
/// 思考配置类型
///
/// 表示经过解析和转换后的思考配置，准备注入到请求体中。
/// 具体类型由目标协议决定：
/// - OpenAI 协议：使用 Effort（等级字符串）或 Disabled
/// - Anthropic 协议：使用 Budget（数值）或 Disabled
/// - Gemini 协议：根据模型版本（2.5 用 Budget，3 用 Effort）
#[derive(Debug, Clone, PartialEq)]
pub enum ThinkingConfig {
    /// 数值预算（tokens），用于 Anthropic 和 Gemini 2.5
    Budget(i32),
    /// 努力等级字符串，用于 OpenAI 和 Gemini 3
    Effort(String),
    /// 思考已禁用（level="none" 或 budget=0）
    /// 协议处理器应注入适当的禁用状态
    Disabled,
}
```

### Requirement: 统一思考注入入口

The system SHALL 提供统一的思考配置注入入口，协调解析、验证、映射和注入流程。

**文件：** `src/thinking/injector.rs`

#### Scenario: 注入流程（意图驱动）
- **当** 收到包含模型名称的 API 请求时
- **则** The system SHALL 执行以下流程：
  1. 解析模型后缀（如 `model(high)` → `model` + `high`）
  2. 将解析结果转换为用户意图（`ThinkingIntent`）
  3. **若无后缀**：更新模型名后直接透传，不注入
  4. 检查基础模型是否在注册表中
  5. 检查模型是否支持思考配置
  6. **根据意图类型分流处理**：
     - `Disabled` → 协议特定的禁用处理
     - `Dynamic` → 协议特定的动态处理
     - `Fixed` → 正常的 clamp 和转换逻辑
  7. 根据协议类型注入对应的思考字段

> **设计决策 - 意图分流架构：**
>
> 传统实现在 `resolve_thinking_config` 中有大量 if-else 分支处理 `auto`、`-1`、`none`、`0` 等特殊值。
> 新架构将这些特殊值的识别提升到意图层面，使用 `ThinkingIntent` 枚举明确分类用户意图：
>
> ```
> 用户输入: model(suffix)
>     ↓
> 解析: parse_model_suffix() → ThinkingValue
>     ↓
> 分类: to_intent() → ThinkingIntent
>     ├─ Disabled: (none)/(0)
>     ├─ Dynamic: (auto)/(-1)
>     └─ Fixed: 其他等级/数值
>     ↓
> 协议适配: resolve_intent_to_config()
>     ↓
> 注入: inject_{openai,anthropic,gemini}()
> ```

### Requirement: 意图到配置的解析

The system SHALL 根据用户意图和目标协议生成对应的 `ThinkingConfig`。

**文件：** `src/thinking/injector.rs`

#### Scenario: Disabled 意图处理

| 协议 | 模型类型 | 输出 |
|------|----------|------|
| Anthropic | 任意 | `ThinkingConfig::Disabled` |
| OpenAI | 任意 | `ThinkingConfig::Disabled` |
| Gemini | Gemini 3（有 levels） | `ThinkingConfig::Budget(0)` |
| Gemini | Gemini 2.5（原生，无 levels） | `ThinkingConfig::Budget(min)` |
| Gemini | 跨协议（如 Claude via Gemini） | `ThinkingConfig::Budget(0)` |

> **注意：原生 Gemini 模型判断**
>
> 使用白名单判断模型是否为原生 Gemini 模型：
> ```rust
> const NATIVE_GEMINI_PREFIXES: &[&str] = &[
>     "gemini-2.5-",
>     "gemini-3-",
>     "gemini-pro",
>     "gemini-flash",
> ];
>
> fn is_native_gemini_model(model_id: &str) -> bool {
>     NATIVE_GEMINI_PREFIXES.iter().any(|prefix| model_id.starts_with(prefix))
> }
> ```
>
> 这样 `gemini-claude-opus-4-5-thinking` 等跨协议模型不会被误识别为原生 Gemini。

#### Scenario: Dynamic 意图处理

| 协议 | 输出 | 说明 |
|------|------|------|
| Anthropic | `Budget(auto_budget)` 或 `Budget((min+max)/2)` | Anthropic 不支持 `-1` |
| OpenAI | `Effort("medium")` 或 clamp 后的等级 | OpenAI 不支持 `auto` |
| Gemini | `Budget(-1)` | Gemini 支持动态 |

> **注意：OpenAI 协议的 Dynamic 处理**
>
> OpenAI 协议将 `auto` 转为 `medium`，然后**还需要 clamp 到模型支持的 levels**。
> 例如 Gemini 3 的 levels 是 `["low", "high"]`，`medium` 不在列表中，
> 会被 clamp 到 `high`。

#### Scenario: Fixed 意图处理

- **Fixed(Level)**: 验证等级有效性 → clamp 到模型支持的 levels → 转换为协议格式
- **Fixed(Budget)**: clamp 到模型范围 → 转换为协议格式

### Requirement: 未知模型错误处理

The system SHALL 对未知模型带思考后缀返回 HTTP 400 错误。

**文件：** `src/thinking/injector.rs`

#### Scenario: 未知模型带思考后缀
- **当** 模型名称包含思考后缀（如 `unknown-model(high)`）
- **且** 基础模型不在注册表中
- **则** The system SHALL 返回 HTTP 400 错误
- **且** 错误信息应说明模型未知

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

### 实现说明

```rust
use crate::models::registry::{ModelInfo, get_model_info};
use crate::protocol::{Protocol, inject_anthropic, inject_gemini, inject_openai};
use crate::thinking::{FixedThinking, ThinkingConfig, ThinkingIntent};
use crate::thinking::models::{
    budget_to_effort, clamp_budget, clamp_effort_to_levels, level_to_budget,
};
use crate::thinking::parser::parse_model_suffix;

/// 默认 medium 预算，当 auto_budget 未配置时使用
const DEFAULT_MEDIUM_BUDGET: i32 = 8192;

/// 原生 Gemini 模型前缀（白名单）
const NATIVE_GEMINI_PREFIXES: &[&str] = &[
    "gemini-2.5-",
    "gemini-3-",
    "gemini-pro",
    "gemini-flash",
];

/// 检查模型是否为原生 Gemini 模型
fn is_native_gemini_model(model_id: &str) -> bool {
    NATIVE_GEMINI_PREFIXES.iter().any(|prefix| model_id.starts_with(prefix))
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

/// 统一注入入口
pub fn inject_thinking_config(
    body: serde_json::Value,
    model_with_suffix: &str,
    protocol: Protocol,
    request_path: &str,
) -> InjectionResult {
    // 1. 解析后缀
    let parsed = parse_model_suffix(model_with_suffix);
    let base_model = parsed.base_name;

    // 2. 转换为意图 - 无后缀则提前返回
    let intent = match parsed.thinking.to_intent() {
        None => {
            let mut body = body;
            body["model"] = serde_json::Value::String(base_model);
            return InjectionResult::PassThrough(body);
        }
        Some(intent) => intent,
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
        let mut body = body;
        body["model"] = serde_json::Value::String(base_model);
        return InjectionResult::PassThrough(body);
    }

    // 5. 根据意图解析为 ThinkingConfig（协议适配）
    let thinking_config = match resolve_intent_to_config(intent, model_info, protocol) {
        Ok(config) => config,
        Err(e) => return InjectionResult::Error(e),
    };

    // 6. 协议特定注入
    let injected = match protocol {
        Protocol::OpenAI => {
            let is_responses = is_responses_endpoint(request_path);
            inject_openai(body, &base_model, thinking_config, is_responses)
        }
        Protocol::Anthropic => inject_anthropic(body, &base_model, thinking_config, model_info),
        Protocol::Gemini => inject_gemini(body, &base_model, thinking_config),
    };

    InjectionResult::Injected(injected)
}

/// 解析用户意图为协议特定的思考配置
///
/// 三种意图的处理：
/// - Disabled: 禁用思考
/// - Dynamic: 动态/自动思考
/// - Fixed: 固定等级或预算
fn resolve_intent_to_config(
    intent: ThinkingIntent,
    model_info: &ModelInfo,
    protocol: Protocol,
) -> Result<ThinkingConfig, InjectionError> {
    let thinking = model_info.thinking.as_ref().unwrap();
    let model_uses_levels = thinking.levels.is_some();

    let needs_effort = match protocol {
        Protocol::OpenAI => true,
        Protocol::Anthropic => false,
        Protocol::Gemini => model_uses_levels,
    };

    match intent {
        ThinkingIntent::Disabled => match protocol {
            Protocol::Anthropic | Protocol::OpenAI => Ok(ThinkingConfig::Disabled),
            Protocol::Gemini => {
                if model_uses_levels {
                    Ok(ThinkingConfig::Budget(0))
                } else if is_native_gemini_model(model_info.id) {
                    Ok(ThinkingConfig::Budget(thinking.min))
                } else {
                    Ok(ThinkingConfig::Budget(0))
                }
            }
        },

        ThinkingIntent::Dynamic => match protocol {
            Protocol::Anthropic => {
                let fallback = thinking.auto_budget.unwrap_or_else(|| {
                    if thinking.max > 0 {
                        (thinking.min + thinking.max) / 2
                    } else {
                        DEFAULT_MEDIUM_BUDGET
                    }
                });
                Ok(ThinkingConfig::Budget(fallback))
            }
            Protocol::OpenAI => {
                let effort = if let Some(levels) = thinking.levels {
                    clamp_effort_to_levels("medium", levels)
                } else {
                    "medium"
                };
                Ok(ThinkingConfig::Effort(effort.to_string()))
            }
            Protocol::Gemini => Ok(ThinkingConfig::Budget(-1)),
        },

        ThinkingIntent::Fixed(fixed) => match fixed {
            FixedThinking::Level(level) => {
                resolve_fixed_level(&level, model_info, protocol, needs_effort)
            }
            FixedThinking::Budget(budget) => {
                resolve_fixed_budget(budget, model_info, protocol, needs_effort)
            }
        },
    }
}

/// 解析固定等级为协议配置
fn resolve_fixed_level(...) -> Result<ThinkingConfig, InjectionError> { ... }

/// 解析固定预算为协议配置
fn resolve_fixed_budget(...) -> Result<ThinkingConfig, InjectionError> { ... }

/// 辅助函数：使用模型配置 clamp 预算
fn clamp_budget_for_model(budget: i32, thinking: &ThinkingSupport, allow_dynamic: bool) -> i32 {
    if thinking.max > 0 {
        clamp_budget(budget, thinking.min, thinking.max,
                     thinking.zero_allowed, allow_dynamic, thinking.auto_budget)
    } else {
        budget
    }
}
```

### 模块依赖关系

```
thinking/injector.rs
    ├── thinking/parser.rs      (解析后缀 + to_intent)
    ├── thinking/models.rs      (等级/预算映射)
    ├── thinking/mod.rs         (ThinkingIntent, FixedThinking, ThinkingConfig)
    ├── models/registry.rs      (模型查询)
    └── protocol/*.rs           (协议特定注入)
```
