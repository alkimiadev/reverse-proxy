---
id: fix/review-005-status-update
name: Update security review #005 status to reflect ADR-028 decision
status: pending
depends_on: []
scope: narrow
risk: low
impact: isolated
level: review
review_findings: [C1, C2, C3, W1, W3, W4, S1, S2, S3, S4, S5, S6]
adr: [028]
---

## Description

Security review #005 (`docs/reviews/005-admin-socket-security-review.md`) is
currently marked as `status: draft`. The review's architectural recommendation
to replace the Unix domain socket with an authenticated HTTP admin endpoint has
been accepted as ADR-028. The review findings should be annotated with their
resolution status.

### Changes Required

**`docs/reviews/005-admin-socket-security-review.md`**:
- Update frontmatter `status` from `draft` to the appropriate post-decision
  status (e.g., `accepted` or `resolved`)
- Add a resolution section at the top of the document noting:
  - C1, C2, C3, W1, W3, W4, S1–S6: **Resolved by ADR-028** (replacing Unix
    domain socket with authenticated HTTP admin API)
  - W2 (config file TOCTOU): **Tracked separately** — ADR-029, task
    `fix/config-reload-toctou`
  - W5 (wildcard flag inconsistency): **Tracked separately** — ADR-030, task
    `fix/wildcard-flag-reload`
  - W6 (changed_fields in reload response): **Tracked** — will be implemented
    as part of `fix/admin-http-api` (the new `/admin/reload` endpoint will
    include changed_fields in its response per operations.md)
  - W7 (health check port recon): **Accepted risk** — health check is
    localhost-only, returns minimal information. The admin HTTP endpoint adds
    authentication for `/admin/*` routes.

**`docs/reviews/006-attack-surface-review.md`**:
- Update Category 5 (Admin Socket) references from `src/admin/socket.rs` to
  `src/admin/auth.rs` and `src/admin/handler.rs` (after admin-http-api task
  is complete)
- Update entry 4.3 (admin reload config file) to reference the shared
  `read_and_validate_config()` function with mtime check
- Remove or update entries that are eliminated by the socket removal (e.g.,
  Category 4: Unix Domain Socket entries)

## Acceptance Criteria

- [ ] Review #005 frontmatter status updated
- [ ] Review #005 has a resolution section annotating each finding with its
      disposition (resolved by ADR-028, tracked separately, accepted risk)
- [ ] Review #006 admin socket references updated (after admin-http-api task)
- [ ] No inline content removed — findings are annotated, not deleted

## References

- docs/reviews/005-admin-socket-security-review.md
- docs/reviews/006-attack-surface-review.md
- docs/architecture/decisions/028-admin-http-api.md

## Notes

> This task should be done after the `fix/admin-http-api` task is complete,
> since review #006 references need to point to the new file structure.

## Summary

> To be filled on completion