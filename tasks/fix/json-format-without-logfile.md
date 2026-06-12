---
id: fix/json-format-without-logfile
name: Fix JSON format not applied when no log file is configured (C4)
status: completed
depends_on: []
scope: single
risk: trivial
impact: isolated
level: implementation
review_findings: [C4]
---

## Description

When `format = "json"` is configured but no `log_file_path` is set, the `None`
branch of `init_json` creates a layer **without** calling `.json()`. The output
is plain text, not JSON. The `format = "json"` config value is silently ignored.

The spec (operations.md) states: "Both output destinations must respect the
`format` config value: when `format = "json"`, both file and stdout output must
use JSON formatting."

### Changes Required

**`src/logging/mod.rs`** — `init_json` function, `None` branch (lines 54-58):
- Add `.json()` to the stdout-only layer:
  ```rust
  None => {
      let layer = tracing_subscriber::fmt::layer()
          .json()
          .with_ansi(false)
          .with_filter(env_filter);
      tracing_subscriber::registry().with(layer).try_init()?;
  }
  ```

## Acceptance Criteria

- [ ] `.json()` is called in the `None` branch of `init_json`
- [ ] `format = "json"` with no `log_file_path` produces JSON output on stdout
- [ ] `format = "json"` with `log_file_path` still produces JSON on both outputs
- [ ] `format = "text"` paths are unchanged
- [ ] `cargo test` passes
- [ ] `cargo clippy` passes with no warnings

## References

- docs/architecture/operations.md — logging output, format guarantee
- docs/reviews/003-security-and-bug-review.md — C4 finding
- src/logging/mod.rs — `init_json` function

## Notes

> To be filled on completion

## Summary

> To be filled on completion