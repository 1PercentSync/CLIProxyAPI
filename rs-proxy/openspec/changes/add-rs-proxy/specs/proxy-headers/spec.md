## ADDED Requirements

### Requirement: Header Forwarding
The system SHALL transparently forward authentication and other headers.

**File:** `src/proxy/client.rs`

#### Scenario: Authorization header
- **WHEN** request contains `Authorization` header
- **THEN** the system SHALL forward it to upstream unchanged

#### Scenario: API key header
- **WHEN** request contains `x-api-key` header
- **THEN** the system SHALL forward it to upstream unchanged

### Implementation Notes

```rust
fn forward_headers(incoming: &HeaderMap) -> HeaderMap {
    let mut outgoing = HeaderMap::new();

    // Forward all headers except host-specific ones
    for (key, value) in incoming.iter() {
        if key != "host" && key != "content-length" {
            outgoing.insert(key.clone(), value.clone());
        }
    }

    outgoing
}
```

**Critical:**
- ALL authentication headers must be forwarded unchanged
- Do not modify, strip, or rewrite auth headers
- Exclude only `Host` and `Content-Length` (recomputed for proxy)
