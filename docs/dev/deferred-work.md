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
| #55 | Pin the owner's release public keys in `PRODUCTION_KEYS` (`crates/syrinx-update/src/keys.rs`); empty until the owner creates the keys, so every update is refused (was DW-30). | Open |
| #56 | Show "Last checked N days ago" for updates; waits until after P1-40 (was DW-67). | Open |
| #58 | Right-click menu on sidebar rows; Ctrl W on a focused row is built (DW-63 follow-up). | Open |
| #95 | Mute a tab from the row menu; until then the app's mute entry (`ui/mute.rs`, `change_mute`) builds only with tests, waiting on the owner's confirmation of the action list. The platform's `SetMuted` and the re-mute after a rebuilt page are live. | Open |
