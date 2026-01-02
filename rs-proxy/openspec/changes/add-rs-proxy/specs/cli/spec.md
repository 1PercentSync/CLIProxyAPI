## ADDED Requirements

### Requirement: CLI Argument Parsing
The system SHALL accept command-line arguments for configuration using argh.

**File:** `src/config.rs`

#### Scenario: Default configuration
- **WHEN** rs-proxy is started without arguments
- **THEN** it SHALL listen on port 6356
- **AND** use base URL "cpa.1percentsync.games"

#### Scenario: Custom port
- **WHEN** rs-proxy is started with `-p 8080` or `--port 8080`
- **THEN** it SHALL listen on port 8080

#### Scenario: Custom base URL
- **WHEN** rs-proxy is started with `-b example.com` or `--base-url example.com`
- **THEN** it SHALL proxy requests to `https://example.com`

### Implementation Notes

```rust
use argh::FromArgs;

#[derive(FromArgs)]
/// RS-Proxy: Thinking configuration injection proxy
struct Args {
    #[argh(option, short = 'p', default = "6356")]
    /// port to listen on
    port: u16,

    #[argh(option, short = 'b', default = "String::from(\"cpa.1percentsync.games\")")]
    /// upstream base URL
    base_url: String,
}
```
