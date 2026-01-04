//! Protocol handlers module.
//!
//! This module provides protocol detection and protocol-specific
//! thinking configuration injection.

mod anthropic;
mod gemini;
mod openai;

pub use anthropic::inject_anthropic;
pub use gemini::inject_gemini;
pub use openai::inject_openai;

use axum::http::HeaderMap;

/// API protocol type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    OpenAI,
    Anthropic,
    Gemini,
}

/// Detect protocol type from request path and headers.
///
/// Detection priority:
/// 1. Exact path match (most endpoints)
/// 2. Path prefix match (Gemini)
/// 3. Header-based detection (only for /v1/models)
/// 4. Fallback to OpenAI
///
/// # Arguments
/// * `path` - Request path
/// * `headers` - Request headers
///
/// # Returns
/// Detected protocol type.
pub fn detect_protocol(path: &str, headers: &HeaderMap) -> Protocol {
    match path {
        "/v1/chat/completions" | "/v1/responses" => Protocol::OpenAI,
        "/v1/messages" => Protocol::Anthropic,
        p if p.starts_with("/v1beta/models") => Protocol::Gemini,
        "/v1/models" => {
            // Shared endpoint: use headers to distinguish
            if headers.contains_key("x-api-key") {
                Protocol::Anthropic
            } else {
                Protocol::OpenAI
            }
        }
        _ => Protocol::OpenAI, // Unknown path fallback to OpenAI
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_openai_chat_completions() {
        let headers = HeaderMap::new();
        assert_eq!(
            detect_protocol("/v1/chat/completions", &headers),
            Protocol::OpenAI
        );
    }

    #[test]
    fn test_detect_openai_responses() {
        let headers = HeaderMap::new();
        assert_eq!(
            detect_protocol("/v1/responses", &headers),
            Protocol::OpenAI
        );
    }

    #[test]
    fn test_detect_anthropic_messages() {
        let headers = HeaderMap::new();
        assert_eq!(
            detect_protocol("/v1/messages", &headers),
            Protocol::Anthropic
        );
    }

    #[test]
    fn test_detect_gemini() {
        let headers = HeaderMap::new();
        assert_eq!(
            detect_protocol("/v1beta/models/gemini-pro:generateContent", &headers),
            Protocol::Gemini
        );
    }

    #[test]
    fn test_detect_models_openai() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer sk-xxx".parse().unwrap());
        assert_eq!(detect_protocol("/v1/models", &headers), Protocol::OpenAI);
    }

    #[test]
    fn test_detect_models_anthropic() {
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", "sk-ant-xxx".parse().unwrap());
        assert_eq!(
            detect_protocol("/v1/models", &headers),
            Protocol::Anthropic
        );
    }

    #[test]
    fn test_unknown_path_fallback() {
        let headers = HeaderMap::new();
        assert_eq!(
            detect_protocol("/unknown/endpoint", &headers),
            Protocol::OpenAI
        );
    }
}
