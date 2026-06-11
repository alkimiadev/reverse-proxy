# ADR-012: Restrict Cipher Suites to Match nginx Scope

## Status

Accepted

## Context

Our current nginx configuration explicitly restricts cipher suites to four
ECDHE-AES-GCM suites:

```
ECDHE-ECDSA-AES128-GCM-SHA256
ECDHE-RSA-AES128-GCM-SHA256
ECDHE-ECDSA-AES256-GCM-SHA384
ECDHE-RSA-AES256-GCM-SHA384
```

rustls 0.23 with the `aws_lc_rs` crypto provider defaults to a conservative
set that excludes all weak ciphers (no SHA-1, no 3DES, no RC4, no CBC-mode
suites, no RSA key exchange). The rustls defaults include these four suites
plus TLS 1.3 suites (which nginx also allows via `TLSv1.3`).

The question was whether to accept the wider rustls defaults or restrict to
the same scope as our current nginx configuration (see OQ-01).

## Decision

Explicitly restrict cipher suites to match the same scope as our current nginx
configuration: the four ECDHE-AES-GCM suites for TLS 1.2, plus all TLS 1.3
suites. This is slightly more restrictive than rustls defaults but preserves
compatibility with all modern clients.

The restricted set in rustls terms:

**TLS 1.2 (explicitly selected):**
- `TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256`
- `TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256`
- `TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384`
- `TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384`

**TLS 1.3 (all default suites):**
- `TLS_AES_128_GCM_SHA256`
- `TLS_AES_256_GCM_SHA384`
- `TLS_CHACHA20_POLY1305_SHA256`

This is configured by building a `CryptoProvider` with a custom
`cipher_suite` list and passing it to `ServerConfig::builder_with_provider()`.

## Rationale

- Maintains behavioral parity with our current nginx configuration during
  migration — no client that worked with nginx should see different TLS
  behavior
- The four TLS 1.2 suites are the same ones nginx allows, providing a known
  security baseline
- TLS 1.3 suites are all modern and secure — restricting them provides no
  meaningful security benefit and reduces compatibility
- rustls defaults include ChaCha20-Poly1305 suites for TLS 1.2 which nginx does
  not; these are cryptographically sound but represent a wider scope than our
  current nginx config allows
- Explicit configuration means the cipher list is documented and auditable
- If compatibility issues arise, expanding the list is straightforward; the
  reverse (restricting after deployment) risks breaking existing clients

## Consequences

**Positive:**
- Behavioral parity with current nginx TLS configuration
- Explicitly auditable cipher list
- No client that currently works will see different TLS behavior
- Matches security review expectations

**Negative:**
- Slightly more restrictive than rustls defaults — excludes ChaCha20-Poly1305
  for TLS 1.2 and AES-CCM suites (rarely used)
- Must update the cipher list when deprecating TLS 1.2 in the future
- Custom `CryptoProvider` construction is slightly more code than using defaults

## References

- [tls.md](../tls.md)
- OQ-01 (now resolved)