//! CLI configuration for RS-Proxy.
//!
//! This module defines command-line arguments using argh.

use argh::FromArgs;

/// Default port for the proxy server.
fn default_port() -> u16 {
    6356
}

/// Default upstream base URL (without protocol prefix).
fn default_base_url() -> String {
    String::from("cpa.1percentsync.games")
}

/// RS-Proxy: A reverse proxy for injecting thinking configuration into API requests.
///
/// Parses model name suffixes (e.g., `model(high)` or `model(16384)`) and injects
/// corresponding thinking configuration into API requests.
#[derive(FromArgs, Debug)]
pub struct Args {
    /// port to listen on (default: 6356)
    #[argh(option, short = 'p', default = "default_port()")]
    pub port: u16,

    /// upstream base URL without protocol (default: cpa.1percentsync.games)
    #[argh(option, short = 'b', default = "default_base_url()")]
    pub base_url: String,
}

impl Args {
    /// Parse command line arguments.
    pub fn parse() -> Self {
        argh::from_env()
    }

    /// Get the full upstream URL with HTTPS protocol.
    pub fn upstream_url(&self) -> String {
        format!("https://{}", self.base_url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_values() {
        assert_eq!(default_port(), 6356);
        assert_eq!(default_base_url(), "cpa.1percentsync.games");
    }
}
