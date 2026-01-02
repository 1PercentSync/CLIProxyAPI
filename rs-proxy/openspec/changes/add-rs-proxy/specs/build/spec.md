## ADDED Requirements

### Requirement: Compile-time Model Data
The system SHALL fetch model support data from CLIProxyAPI at compile time.

**File:** `build.rs`

#### Scenario: Build-time fetching
- **WHEN** the project is compiled
- **THEN** build.rs SHALL fetch CLIProxyAPI source files from GitHub
- **AND** parse model definitions to extract thinking support data
- **AND** generate Rust code for model registry

### Source Files to Parse

The primary source file for model data is:
- `internal/registry/model_definitions.go` - Contains all static model definitions with `ThinkingSupport` configuration

Additional files for Gemini-specific patterns (optional):
- `internal/util/gemini_thinking.go` - Contains Gemini 3 model detection regexes and level mappings

### Data Structures to Extract

From `model_definitions.go`, extract for each model:

```go
// ModelInfo structure (from model_registry.go)
type ModelInfo struct {
    ID                  string           // Model identifier
    MaxCompletionTokens int              // Max tokens for completion (used for max_tokens adjustment)
    Thinking            *ThinkingSupport // nil if thinking not supported
}

// ThinkingSupport structure
type ThinkingSupport struct {
    Min            int      // Minimum thinking budget
    Max            int      // Maximum thinking budget
    ZeroAllowed    bool     // Whether budget=0 is valid
    DynamicAllowed bool     // Whether budget=-1 (auto) is valid
    Levels         []string // Discrete levels for models that use levels instead of budgets
}
```

### Example Model Definitions

```go
// Claude models with thinking support
{
    ID:                  "claude-sonnet-4-5-20250929",
    MaxCompletionTokens: 64000,
    Thinking:            &ThinkingSupport{Min: 1024, Max: 100000, ZeroAllowed: false, DynamicAllowed: true},
}

// Claude Haiku - no thinking support (Thinking: nil)
{
    ID:                  "claude-3-5-haiku-20241022",
    MaxCompletionTokens: 8192,
    // Thinking: not supported for Haiku models
}
```

### Implementation Notes

```rust
// build.rs
use std::{env, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let base_url = "https://raw.githubusercontent.com/1PercentSync/CLIProxyAPI/main/";
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);

    // Primary file: model definitions
    let model_defs_url = format!("{}internal/registry/model_definitions.go", base_url);
    let model_defs = reqwest::blocking::get(&model_defs_url)?.text()?;

    // Parse Go source to extract ModelInfo structs
    let models = parse_model_definitions(&model_defs);

    // Generate Rust code
    let generated = generate_rust_registry(&models);
    fs::write(out_dir.join("model_registry.rs"), generated)?;

    // Rerun if source changes
    println!("cargo:rerun-if-changed=build.rs");

    Ok(())
}

fn parse_model_definitions(go_source: &str) -> Vec<ModelInfo> {
    // Use regex to extract:
    // - ID: "model-name"
    // - MaxCompletionTokens: 64000
    // - Thinking: &ThinkingSupport{Min: 1024, Max: 100000, ...}
    // ...
}
```

**Cargo.toml:**
```toml
[build-dependencies]
reqwest = { version = "0.13.1", features = ["blocking"] }
regex = "1.12.2"
```

**Generated Rust structures:**
```rust
pub struct ThinkingSupport {
    pub min: i32,
    pub max: i32,
    pub zero_allowed: bool,
    pub dynamic_allowed: bool,
    pub levels: Option<Vec<&'static str>>,
}

pub struct ModelInfo {
    pub id: &'static str,
    pub max_completion_tokens: i32,
    pub thinking: Option<ThinkingSupport>,
}
```

**In main code:**
```rust
use std::sync::LazyLock;

static MODEL_REGISTRY: LazyLock<Vec<ModelInfo>> = LazyLock::new(|| {
    include!(concat!(env!("OUT_DIR"), "/model_registry.rs"))
});

pub fn get_model_info(id: &str) -> Option<&'static ModelInfo> {
    MODEL_REGISTRY.iter().find(|m| m.id == id)
}

pub fn model_supports_thinking(id: &str) -> bool {
    get_model_info(id).map(|m| m.thinking.is_some()).unwrap_or(false)
}
```
