# libsqlite3-sys 0.38.2 with SQLite 3.53.4

P1-04 validates potentially damaged recovery inputs. This local patch brings
in SQLite's 3.53.3/3.53.4 journal, WAL and corruption-check fixes while
retaining Atlas's audited Rust wrapper, build script and feature set. The
owner approved this source patch and the narrow exception for unchanged
upstream TODO/FIXME comments on 2026-10-10. The rationale and verification
requirements are in `docs/p1-04-sqlite-maintenance-review.md`.

## Sources and identity

The baseline is the published `libsqlite3-sys 0.38.2` registry archive:

```text
Archive SHA-256:
f1d20bef17f513b9b3004532233187769cd072d790971f4e4da0e346eb6401e8
Packaged upstream Git revision:
e88f112bef7899234a497baed5cc3c3d553deeb8
Path in upstream repository: libsqlite3-sys
```

The archive checksum was verified before reading its selected files. The
13 retained files below were copied directly from that archive, not from a
modified working checkout. Four complete files were fetched over HTTPS from
the exact [upstream SQLite 3.53.4 import](https://github.com/rusqlite/rusqlite/commit/901f9946efdaaa289e6b1c5bd56dc67f4b651e51),
revision `901f9946efdaaa289e6b1c5bd56dc67f4b651e51`, under
`libsqlite3-sys/sqlite3/`. No upstream script was run.

All four replacements identify this engine:

```text
SQLite version: 3.53.4
SQLite source ID:
2026-07-24 19:02:57 bf7c7f30031888f4e796e429ab3978879485813aaca6f641c7b33e4e09459bcc
sqlite3.c SHA3-256:
67f423e9ebbbdc473cbc4772c872ee6b89f31fde4ed0279a5c25d5f65c043a16
```

The downloaded C file's SHA3-256 matches the independent
[SQLite release record](https://www.sqlite.org/releaselog/3_53_4.html).
The wrapper package remains version 0.38.2. Its unchanged upstream README
describes the registry release's 3.53.2 bundle; this provenance records the
actual patched source and supersedes that version statement for Atlas.

## Exact file inventory

There are 17 upstream files in `vendor/libsqlite3-sys-0.38.2/`, totalling
11,185,233 bytes. This provenance file lives beside that directory. SHA-256
values below describe the exact imported bytes; the scoped `.gitattributes`
rule prevents Git text conversion from changing them.

| Retained baseline file | SHA-256, identical to the registry archive |
| --- | --- |
| `Cargo.toml` | `ce2c66104c29834e8c5e875f765e2ddfc577b9366b1ccef80f3680c2039948b3` |
| `Cargo.toml.orig` | `f7a4bf06e3f2b8ad8f1e559637b84c19fec5ad3daee05c42b75fb596c3c188e9` |
| `README.md` | `17a4e830209b642f0bfebd4b3f581fa4fea24efca5e500e10eee58760de99969` |
| `LICENSE` | `c10c1f27337546471e5f7e4e97fdd398b35b9d4e126115dcd22de8d8e65abf6f` |
| `build.rs` | `269acf1864de5046ac5b77814926d41fe24108b0d027f635812a1db3c965f81f` |
| `src/lib.rs` | `f61d8e306e11357479e20999f7b7adc5f3dbaea40cd17361853574001cf79360` |
| `src/error.rs` | `9685678a8752bf47e5e39923bb2cf466cc312a25848d91cc206bf89eaccfb7d6` |
| `wrapper.h` | `b78f576f7eeabf316e183d476b0f16344b6897680b4df47628d3ce38c0aa979a` |
| `wrapper_ext.h` | `fa7a53fea99318ee29d75e6d5fab4d7ed26e9599803268e6a24396265f2a6f3a` |
| `bindgen-bindings/bindgen_3.34.1.rs` | `4f94d0192ec890b62773368cb026daf219f5182094952c007d28ed6d3812fd68` |
| `bindgen-bindings/bindgen_3.34.1_ext.rs` | `44ff11f09886f8c41494f65043ac2ddba4e476bb17a81fe93a8c10c7bffdf4a8` |
| `sqlite3/sqlite3ext.h` | `ac9645e5c9ff0cf176efdd6e75cb5e98f46295d38e02db5c4d208826a39ab4be` |
| `sqlite3/wasm32-wasi-vfs.c` | `b95842044da2b8777876b7d3fef8c9711e8c201fb4a4255dbac98729a456bb5d` |

| Replaced file | Imported SHA-256 |
| --- | --- |
| `sqlite3/sqlite3.c` | `b1dd5d74ec7f29055a6684fa06fb3c2f6821c87dd38f9a458dfd2e8a1db28189` |
| `sqlite3/sqlite3.h` | `919e7f2e8ed1d8f56ac17b412b8971c76aa5d1a879752cc6058f75e7d5910e1d` |
| `sqlite3/bindgen_bundled_version.rs` | `59b073d337482286fb0e1b009e2800b70e6ce84b839c089b5cfa5e8059d44e4e` |
| `sqlite3/bindgen_bundled_version_ext.rs` | `08c67370ccbc5050789ba6915b07c6c401e0b829db0cf22244eeaa8e87ec3c41` |

Their corresponding baseline SHA-256 values, in the same order, are:

```text
0a409f1633283fa31a9126b11fbfd64a1991c5d30defad07e5745d4667f5e23d
9e69a1353a4288450b0d5239ede11fc7f1f4c8e5eb07491fc8317eacb5b7de7e
5b5a37a74dc728fb0bcd041867eb577c03c95e2fe46285262c589a3b1cd4bbac
445cc206e04a87131d4690742517ee7dcc3a430319bac847ce5e9586f98ee301
```

The exact omitted archive files are `.cargo_vcs_info.json`, `.gitignore`,
`Cargo.lock`, `Upgrade.md`, `sqlcipher/LICENSE`,
`sqlcipher/bindgen_bundled_version.rs`, `sqlcipher/sqlite3.c`,
`sqlcipher/sqlite3.h` and `sqlcipher/sqlite3ext.h`. Cargo's extracted-cache
marker `.cargo-ok` is also not copied. The source is deliberately trimmed,
not represented as a complete registry package.

Only Atlas's approved Windows bundled build is supported by this copy.
The small alternate bindings and headers remain unchanged. The WASI file
remains because the unchanged build script watches it, but Windows does not
compile it. SQLCipher and other optional feature paths are not authorized
or promised to work merely because the unchanged manifest declares them.

## Licence and review boundaries

The wrapper, generated bindings and retained package notices remain under
MIT. SQLite's deliverable C source and headers are public domain, as stated
in their headers and the [SQLite copyright statement](https://www.sqlite.org/copyright.html).
Do not relabel the SQLite implementation MIT. The original `LICENSE` remains
intact; omitted SQLCipher source is not compiled or distributed here. This
patch introduces no new licence category or dependency feature.

The C replacement includes the reviewed upstream damaged-input handling and
Windows path-classification fixes. The header and active Rust bindings
update the source identity; the active bindings also make two extern blocks
explicitly unsafe. The optional extension bindings have a larger generated
diff that adds explicit unsafe blocks. That file is preserved from the
approved upstream revision and is not compiled by Atlas's current features.
All four files are complete upstream imports, not manual Atlas edits.

## Verification and removal condition

Completed on 2026-10-10: archive checksum; all 17 file hashes and inventory;
13 byte-identical retained inputs; four matching version/source identifiers;
the official amalgamation SHA3-256; and Git's scoped byte-preservation rule.
Offline Cargo metadata before and after the patch confirms unchanged package
versions, resolved Windows features, dependency edges and five workspace
members. Only libsqlite3-sys 0.38.2 changes source. Its lockfile entry loses
the registry source/checksum, and every package version remains unchanged.
The vendor directory is explicitly excluded from workspace membership.
The pre-build environment check found no `LIBSQLITE3_SYS_USE_PKG_CONFIG`,
`LIBSQLITE3_FLAGS`, `SQLITE3_LIB_DIR`, `SQLITE3_INCLUDE_DIR`, `SQLITE3_STATIC`,
`SQLITE_MAX_VARIABLE_NUMBER` or `SQLITE_MAX_EXPR_DEPTH` override.

At the source-import handoff, compilation and runtime checks were still
pending. The subsequent full `cargo xtask check` passed all 78 tests,
including the linked-engine identity test, on 2026-10-10. Independent
inventory and focused native-source reviews also passed. The maintenance
review and dependency audit record that evidence; backup and recovery
behavior is still unimplemented. No upstream script, bindgen generation,
machine setting change or product-network feature is part of this patch.

Replace this path override with an audited, exactly pinned published package
containing the fixes when it is compatible with the approved wrapper. The
owner must approve the maintenance change and removal of the vendor tree;
preserve the equivalent source-identity, long-path and recovery tests. Keep
the exception for upstream comments restricted to this dependency until
that removal. Do not alter the files behind these hashes without recording
and reviewing another explicit source change.
