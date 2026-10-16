# Deferred work

Every deferred-work marker in Syrinx-authored code names the GitHub issue that
tracks it: `TODO(#<issue>)`, `FIXME(#n)` or `HACK(#n)`. The issue must be
listed in the table below, and `cargo xtask check` rejects an untagged marker
or one whose issue is not listed. The owner set this rule on 2026-10-11 and
switched it from `DW-n` tags to issue numbers on 2026-10-16. A marker never
excuses a failing test, a silenced lint or a swallowed error.

## Deferred work

| Issue | Deferred work and why | Status |
| --- | --- | --- |
| #28 | Bound palette command ranking before plugins can register commands (was DW-5). | Open |
