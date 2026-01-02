## ADDED Requirements

### Requirement: SSE Streaming Support
The system SHALL correctly handle SSE streaming responses.

**File:** `src/proxy/streaming.rs`

#### Scenario: Streaming response passthrough
- **WHEN** upstream returns a streaming response with `Content-Type: text/event-stream`
- **THEN** the system SHALL forward each chunk to the client immediately
- **AND** maintain proper SSE framing

### Implementation Notes

```rust
use futures::StreamExt;
use reqwest::Response;

async fn forward_stream(response: Response) -> impl axum::body::Body {
    let stream = response.bytes_stream();
    axum::body::Body::from_stream(stream)
}
```

**Critical:**
- NO buffering - forward chunks immediately
- Use `bytes_stream()` from reqwest
- Preserve `Content-Type: text/event-stream` header in response
