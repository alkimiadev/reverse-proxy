---
id: fix/tls-mode-wildcard-mismatch
name: Add explicit listener/acceptor count check or remove TlsMode wildcard (W5)
status: completed
depends_on: []
scope: single
risk: trivial
impact: isolated
level: implementation
review_findings: [W5]
---

## Description

The `match tls_mode` in `main.rs` has a wildcard `_` arm that logs a warning
and pushes **no** acceptor. Then `bound_listeners.into_iter().zip(tls_acceptors.into_iter())`
uses `zip`, which silently stops at the shorter iterator. If the wildcard arm
were ever reached, some listeners would have no TLS acceptor and would be
silently dropped.

`setup_tls` already rejects unknown modes with `bail!`, so the wildcard is
unreachable in practice. But it's a latent bug for future refactors.

### Changes Required

**`src/main.rs`** (lines 170-194):
- Option A (preferred): Remove the wildcard `_` arm entirely. Since `TlsMode`
  only has two variants (`Manual` and `Acme`) and `setup_tls` already validates,
  the wildcard is dead code. Removing it means the compiler will catch future
  `TlsMode` additions.

- Option B: Add an explicit count check after the match loop:
  ```rust
  if bound_listeners.len() != tls_acceptors.len() {
      anyhow::bail!("listener/acceptor count mismatch: {} listeners, {} acceptors",
          bound_listeners.len(), tls_acceptors.len());
  }
  ```

  If removing the wildcard, this check is redundant but harmless as a
  defense-in-depth assertion.

## Acceptance Criteria

- [ ] Wildcard `_` arm removed from the `match tls_mode` block, OR
- [ ] Explicit count mismatch check added after the acceptor construction loop
- [ ] `cargo test` passes
- [ ] `cargo clippy` passes with no warnings

## References

- docs/reviews/003-security-and-bug-review.md — W5 finding
- src/main.rs — TLS acceptor construction loop (lines 170-194)
- src/tls/acceptor.rs — `setup_tls`, `TlsMode` enum

## Notes

> To be filled on completion

## Summary

> To be filled on completion