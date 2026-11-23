# `syrinx:plugin@0.1.1`

This folder is the contract every Syrinx plugin is written against: one WIT
(WebAssembly Interface Type) package whose world, `plugin`, lists what Syrinx
offers a plugin (imports) and what a plugin offers Syrinx (exports). Syrinx's
host (`crates/syrinx-plugin-host`) and every plugin's guest code are both
generated from `plugin.wit`, so a mismatch fails when a plugin loads, with a
named error, rather than deep inside a call.

The owner approved its shape on 2026-10-26 (G9 in `docs/decisions.md`). It
counts as frozen once #28's contract tests pass; the plugin template (#30) and
Notes copy it from the commit #28's closing comment names.

0.1.1 adds one import, `service` (#280), and nothing else: a plugin built
for 0.1.0 loads unchanged, and the host's contract tests load one to prove it
(`a_plugin_built_for_0_1_0_still_loads_and_runs`).

## Changing it

A mistake here costs every plugin, so the rules are strict. They come from
what wasmtime 48 was observed to do (`docs/dev/vendor-reviews/wasmtime-wit-prebuild-check.md`,
"The version rules, observed"):

- **Within 0.1.x, only imports may be added**, each marked
  `@since(version = 0.1.x)`: a new imported interface, or a new function or
  type inside an imported interface. A 0.1.0 plugin keeps loading on a host
  that adds them.
- **Nothing else changes within 0.1.x.** No export, no function in an
  exported interface, no field in a record and no case in a variant or enum is
  added, renamed or removed, `@since` or not. wasmtime's `bindgen!` requires
  every export it knows of every plugin, so an added export breaks every older
  plugin.
- **Anything else starts a new minor track**, `wit/v0_2/`, with its own
  `bindgen!` module in the host, which then loads both tracks.
- **`@unstable` items are never exported.** Home widgets (#31) return as a
  new track or a separate world, not as a gated export.

## Rules the host keeps

These are part of the contract, written here and in `plugin.wit`, and the
host's contract tests check them:

- **Forms.** For a command declared with a form, the host calls
  `forms.form-for` on preview, `forms.validate` as fields change and
  `forms.submit` on Enter, and never `commands.run`. `form-for` and
  `validate` have no side effects. `submit` gets a request id the host
  chose; a `pending` answer returns exactly that id.
- **Delivery order.** The host never calls an export from inside an import.
  Calls it defers run first in, first out, after the current export returns.
  For each view, `opened` comes before any `message` and `closed` comes last.
  `events.deliver(r, ...)` follows the export that returned `pending(r)` or
  armed `r`.
- **Cancellation.** There is no cancel export. When a view cancels a
  request, or a plugin calls `host.cancel`, the host drops anything still
  queued for that id before delivering it. `plugin-error.cancelled` is never
  used where a write may have happened; that is `outcome.uncertain`.
- **Service requests.** `service.request` reaches only the one address the
  owner approved for the plugin, and only under the `service` grant. The
  plugin gives a path; the host chooses the scheme, host and port, adds the
  credential (never returned, logged or stored in the repository), refuses
  the headers it sets itself, follows a redirect only within that address,
  bounds the request and response sizes and the time
  (`crates/syrinx-plugin-host/src/limits.rs`), and starts no request while
  the owner types in the palette. A remote address is HTTPS only. Each
  refusal's text starts with its rule's name, as in
  `denied("redirect-off-host: ...")`. `unavailable` means asking again
  cannot repeat a write: nothing was sent, or the request was a GET or HEAD.
  Once a POST, PUT, PATCH or DELETE may have reached the server, any failure
  after that point (a lost or oversized answer, a refused redirect, a later
  hop that cannot be sent) is `internal("uncertain: ...")`, naming the rule
  that failed, and the request is never resent.
- **Search keywords.** A catalogue entry carries at most 16 keywords of at
  most 120 characters; the host drops and logs an over-limit keyword instead
  of refusing the batch.

## Names no item may use

These are WIT keywords in `wit-parser` 0.254 (the version wasmtime 48 uses);
an interface, type, field, case or function with one of these names fails to
parse unless it is escaped with `%` (as `icon`'s `%list` is). Prefer another
name: `documents.list` became `list-entries` for this reason.

`as`, `async`, `bool`, `borrow`, `char`, `constructor`, `enum`,
`error-context`, `export`, `f32`, `f64`, `flags`, `from`, `func`, `future`,
`import`, `include`, `interface`, `list`, `map`, `option`, `own`, `package`,
`record`, `resource`, `result`, `s8`, `s16`, `s32`, `s64`, `static`, `stream`,
`string`, `tuple`, `type`, `u8`, `u16`, `u32`, `u64`, `use`, `variant`,
`with`, `world`.

A type and a function in one interface also share one namespace, so
`variant vault-state` and `vault-state: func()` cannot both exist.
