# Dependency audit

Dependencies enter Atlas only after their maintenance, security record and
licence have been checked (`AGENTS.md`). Record the exact package graph and
features: a permissive top-level crate does not establish its dependencies'
licences, and a lockfile can contain packages that this configuration never
compiles. Repeat the audit when versions or enabled features change.

## P1-01: reading Cargo metadata, 2026-10-10

The architecture check needs Cargo's structured metadata so dependency
aliases and target/build/development declarations cannot hide a forbidden
dependency. Use `serde_json::Value` in `xtask` to read that JSON, rather than
writing a JSON parser or adding a larger Cargo-specific library. This is a
development-tool dependency; `atlas-app` has no third-party dependency yet.

The selected direct pin is `serde_json =1.0.151`, default features disabled,
with only `std` enabled. A read-only audit inspected the cached manifests,
licence files, index entries and build scripts before this dependency was
added. The selected versions were not marked yanked. The package's
[releases](https://github.com/serde-rs/json/releases) and the upstream
manifests below show current maintenance. The offline lockfile resolution
and Windows `cargo tree` output matched the five active packages below.

| Active package | Version | Enabled features | Allowed licence selected | Upstream manifest |
|---|---|---|---|---|
| serde_json | 1.0.151 | std | MIT | [serde_json](https://raw.githubusercontent.com/serde-rs/json/master/Cargo.toml) |
| serde_core | 1.0.229 | std | MIT | [serde_core](https://raw.githubusercontent.com/serde-rs/serde/master/serde_core/Cargo.toml) |
| itoa | 1.0.18 | None | MIT | [itoa](https://raw.githubusercontent.com/dtolnay/itoa/master/Cargo.toml) |
| memchr | 2.8.3 | std, alloc | MIT | [memchr](https://raw.githubusercontent.com/BurntSushi/memchr/master/Cargo.toml) |
| zmij | 1.0.23 | None | MIT | [zmij](https://raw.githubusercontent.com/dtolnay/zmij/master/Cargo.toml) |

The [RustSec package index](https://rustsec.org/packages/) had no published
entry for these five packages on the review date. This is a scoped advisory
check, not a guarantee that no vulnerability exists. The inspected active
build scripts check compiler/target properties and, for `serde_core`, write
generated content under Cargo's output directory; no network operation was
found. The scaffold resolves and builds offline from the existing cache.

Serde uses `cfg(any())` dependencies to keep otherwise inactive serde/derive
versions aligned. Empty `any()` is false ([Rust conditional compilation](https://doc.rust-lang.org/reference/conditional-compilation.html#predicate)),
so these entries may appear in `Cargo.lock` without entering this build.
Unicode-3.0 appears in that inactive derive closure; this scaffold does not
enable, compile or vendor it. Its presence does not approve future use.
Activating derive or another route to that code requires a fresh licence
review. Future Cargo commands can fetch inactive packages; the lockfile is
not a claim that every listed crate was used or audited as active code.

Verify with:

```powershell
cargo tree --workspace --target x86_64-pc-windows-msvc --edges normal,build,features --offline --locked
```

GPUI, wry, WebView2 bindings and other future crates are not added by this
slice. Their existing spike pins stay unchanged and need their own audit
before entering the product workspace.
