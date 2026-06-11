# ADR-011: Multi-Domain TLS Configuration

## Status

Accepted

## Context

With multi-site support in Phase 1 (ADR-010), the TLS configuration must
support multiple domains. The previous design used a single `tls.acme_domain`
string field, which only works for one domain.

There are several approaches to multi-domain TLS:

1. **Single ACME config with domain list**: `acme_domains = ["git.alk.dev",
   "alk.dev"]` — one certificate covering all domains (SAN certificate)
2. **Per-site TLS configuration**: Each site entry specifies its own TLS
   mode (ACME or manual) and domain — more flexible but complex
3. **Hybrid**: A global TLS section with ACME domains, plus per-site overrides
   for manual certificates

For our use case, all proxied domains use the same ACME certificate authority
(Let's Encrypt) and the same challenge type (TLS-ALPN-01). There's no need
for per-site TLS configuration in Phase 1.

## Decision

Use a single ACME configuration with a list of domains, producing one SAN
certificate covering all proxied domains. Manual mode uses certificate file
paths (single cert file with all domains, or one cert per domain resolved via
SNI).

The config format changes from the previous single-domain format:

```toml
# Previous (single-domain) format — no longer used
[tls]
mode = "acme"
acme_domain = "git.alk.dev"  # single string
```

To the current multi-domain format:

```toml
[tls]
mode = "acme"
acme_domains = ["git.alk.dev", "alk.dev"]  # array of strings
```

In ACME mode, `rustls-acme` provisions a single certificate covering all
listed domains via Subject Alternative Names (SAN). This is the standard
Let's Encrypt approach for multi-domain certificates.

In manual mode, the cert and key files must cover all domains (either a SAN
certificate or separate certificates resolved via SNI).

## Rationale

- A single SAN certificate is simpler to manage (one renewal, one cert)
- Let's Encrypt supports SAN certificates with up to 100 domains
- `rustls-acme` accepts `Vec<String>` for domain lists — this is its natural
  API
- All our domains use the same ACME configuration (Let's Encrypt production,
  TLS-ALPN-01 challenge)
- Per-site TLS overrides add complexity with no current benefit
- If per-site TLS configuration is needed later (e.g., a site with a manual
  cert), it can be added as an optional override without changing the global
  config structure

## Consequences

**Positive:**
- Single certificate for all domains — simpler renewal, simpler cert management
- Matches `rustls-acme`'s natural API (`AcmeConfig::new(domains: Vec<String>)`)
- All domains in one cert means SNI resolution is handled by ACME automatically
- Config format is a minimal change from single-domain

**Negative:**
- Adding or removing a domain requires re-provisioning the certificate (ACME
  handles this automatically, but it means cert changes affect all domains)
- If one domain fails ACME validation, the entire cert renewal fails (all
  domains must be validated) — mitigated by Let's Encrypt's domain-level
  validation
- Per-site TLS configuration (e.g., a domain with a manual cert) requires a
  future config extension (OQ-07)

## References

- [tls.md](../tls.md)
- [config.md](../config.md)
- ADR-010 (multi-site in Phase 1)
- ADR-004 (ACME-primary certificate management)