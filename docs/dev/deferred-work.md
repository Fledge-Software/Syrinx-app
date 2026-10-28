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
| #28 | Bound palette command ranking before plugins can register commands (was DW-5). | Done: the registry refuses commands past `CommandRegistry::MAX_COMMANDS` (1,024), and the marker is gone. |
| #55 | Pin the owner's release public keys in `PRODUCTION_KEYS` (`crates/syrinx-update/src/keys.rs`); empty until the owner creates the keys, so every update is refused (was DW-30). | Open |
| #56 | Show "Last checked N days ago" for updates; waits until after P1-40 (was DW-67). | Open |
| #126 | Draw the back and forward list that a right-click, a held press or Alt Down on Back or Forward asks for (`crates/syrinx-app/src/ui/back_forward.rs`); the data path is built, and the menu waits for its canvas design. | Open |
