//! HTTP client and request forwarding.
//!
//! This module provides HTTP client wrapper and SSE stream handling.

use axum::http::HeaderMap;
use bytes::Bytes;
use reqwest::{Client, Method, Response};
use std::time::Duration;

/// Create a shared HTTP client with connection pooling.
///
/// # Configuration
/// - Timeout: 120 seconds (to support long API calls)
/// - Max idle connections per host: 10
pub fn create_client() -> Client {
    Client::builder()
        .timeout(Duration::from_secs(120))
        .pool_max_idle_per_host(10)
        .build()
        .expect("Failed to create HTTP client")
}

/// Forward headers from incoming request to outgoing request.
///
/// All headers are forwarded except:
/// - `Host`: Will be set by reqwest based on target URL
/// - `Content-Length`: Will be recalculated by reqwest
pub fn forward_headers(incoming: &HeaderMap) -> reqwest::header::HeaderMap {
    let mut outgoing = reqwest::header::HeaderMap::new();

    for (key, value) in incoming.iter() {
        // Skip hop-by-hop headers and headers that will be set by reqwest
        let key_str = key.as_str().to_lowercase();
        if key_str == "host" || key_str == "content-length" {
            continue;
        }

        // Convert axum HeaderName to reqwest HeaderName
        if let Ok(name) = reqwest::header::HeaderName::from_bytes(key.as_str().as_bytes()) {
            if let Ok(val) = reqwest::header::HeaderValue::from_bytes(value.as_bytes()) {
                outgoing.insert(name, val);
            }
        }
    }

    outgoing
}

/// Forward a request to the upstream server.
///
/// # Arguments
/// * `client` - Shared HTTP client
/// * `base_url` - Upstream base URL (without protocol, e.g., "api.example.com")
/// * `method` - HTTP method
/// * `path` - Request path (including query string)
/// * `headers` - Headers from incoming request
/// * `body` - Request body
///
/// # Returns
/// The upstream response or an error.
pub async fn forward_request(
    client: &Client,
    base_url: &str,
    method: Method,
    path: &str,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, reqwest::Error> {
    let url = format!("https://{}{}", base_url, path);
    let forwarded_headers = forward_headers(&headers);

    client
        .request(method, &url)
        .headers(forwarded_headers)
        .body(body)
        .send()
        .await
}

/// Convert reqwest response headers to axum headers.
pub fn convert_response_headers(
    response_headers: &reqwest::header::HeaderMap,
) -> axum::http::HeaderMap {
    let mut headers = axum::http::HeaderMap::new();

    for (key, value) in response_headers.iter() {
        // Skip hop-by-hop headers
        let key_str = key.as_str().to_lowercase();
        if key_str == "transfer-encoding" || key_str == "connection" {
            continue;
        }

        if let Ok(name) = axum::http::HeaderName::from_bytes(key.as_str().as_bytes()) {
            if let Ok(val) = axum::http::HeaderValue::from_bytes(value.as_bytes()) {
                headers.insert(name, val);
            }
        }
    }

    headers
}

/// Check if the response is a streaming response (SSE).
pub fn is_streaming_response(response: &Response) -> bool {
    response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|ct| ct.contains("text/event-stream"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_forward_headers_excludes_host() {
        let mut incoming = HeaderMap::new();
        incoming.insert(
            axum::http::header::HOST,
            "example.com".parse().unwrap(),
        );
        incoming.insert(
            axum::http::header::AUTHORIZATION,
            "Bearer token".parse().unwrap(),
        );

        let outgoing = forward_headers(&incoming);

        assert!(outgoing.get("host").is_none());
        assert!(outgoing.get("authorization").is_some());
    }

    #[test]
    fn test_forward_headers_preserves_auth() {
        let mut incoming = HeaderMap::new();
        incoming.insert(
            axum::http::header::AUTHORIZATION,
            "Bearer secret".parse().unwrap(),
        );
        incoming.insert("x-api-key", "api-key-value".parse().unwrap());

        let outgoing = forward_headers(&incoming);

        assert_eq!(
            outgoing.get("authorization").unwrap().to_str().unwrap(),
            "Bearer secret"
        );
        assert_eq!(
            outgoing.get("x-api-key").unwrap().to_str().unwrap(),
            "api-key-value"
        );
    }
}
