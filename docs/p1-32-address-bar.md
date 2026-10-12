# P1-32 address bar: what was built and what the owner checks

Plan 1's slice P1-32 makes the command palette double as the address bar (S5,
spec section 3). P1-10 already had an address parser and an Alt Enter row,
and they merged with P1-30/31, but the parser had three gaps: it treated any
text starting with `localhost` (such as `localhosting`) as an address, it
turned `1.5` or `e.g.` into a web address because they contain a dot, and it
opened local development servers over `https`, which they rarely speak. It
lived in `atlas-app`, so its tests could not sit beside the shared address
rule in `atlas-core`. The address field also showed the raw percent-encoded
address. Finally, D9 ("no live suggestions; keystrokes stay on the machine")
was a promise in prose with nothing in the gate to keep it true.

## What the address bar does now

The rule is `atlas_core::entry_destination` and `atlas_core::recognized_address`
in `crates/atlas-core/src/address_entry.rs`. Both end in DW-1's one
`atlas_core::web_address` check, so the address bar, the bookmark importer
and P1-34's forwarded links still agree on what a web address is.
`crates/atlas-app/src/navigation.rs` is now a two-function adapter that turns
the typed error into the owner-facing sentence.

| Typed text | Result |
| --- | --- |
| `example.com`, `example.com/path?q=1`, `example.com:8443` | `https://` that address |
| `localhost`, `localhost:8080`, `app.localhost:3000`, `127.0.0.1:8080`, `[::1]:8443`, `::1` | `http://` that address (loopback only) |
| `192.0.2.10`, `[2001:db8::1]:8443`, `2001:db8::1/path` | `https://` that address; a bare IPv6 literal gets its brackets |
| `bücher.example` | `https://xn--bcher-kva.example/` (the `url` crate's punycode) |
| `http://…`, `https://…` in any letter case | that address, checked by `web_address` |
| `about:blank` | `about:blank` |
| Text with a space, one word, `1.5`, `e.g.`, `a.b`, `word:word`, `someone@example.com` | Google search |
| `javascript:`, `data:`, `file://`, `ftp://` and other `scheme://` | refused with "This preview opens HTTP and HTTPS addresses." |
| Anything, with **Alt Enter** | Google search for exactly the typed text |

A schemeless word counts as a host when it is `localhost` (or ends in
`.localhost`), a four-part IPv4 address, an IPv6 literal, or a dotted name
whose last label has at least two characters and starts with a letter. That
last rule is a heuristic, not the public suffix list: `node.js` and
`file.txt` still open as addresses, as they would have before. Shipping the
suffix list would add a data dependency that needs updating; the owner can
always press Alt Enter to search instead.

Loopback hosts open over `http` because local development servers almost
never have certificates; Chrome and Edge also use `http` there. Every other bare
host, including private network addresses such as a router's, opens over
`https`; that is an owner question below. Text containing `@` before its
first `/` is searched rather than opened, because `web_address` refuses
credentials in addresses and an e-mail address is far more likely than a
`user@host` URL.

## What the address field shows

`atlas_core::readable_address` (`crates/atlas-core/src/address_display.rs`)
is what the field above the page card displays. It decodes percent-escapes
in the path, query and fragment only into characters that cannot change how
the address reads:

- ASCII is decoded only for letters, digits and `-._~`. A reserved character
  such as `/`, `?`, `#`, `&`, `=` or `%` stays escaped, so `a%2Fb` never reads
  as two path segments.
- Non-ASCII characters are decoded (`caf%C3%A9` reads `café`) unless they are
  controls, white space, invisible or direction-changing characters (for
  example U+202E, which reverses the text after it) or characters that look
  like `/`, `?`, `#` or `.`. Those stay escaped.
- Escapes that are not valid UTF-8 are left exactly as they are.

The host is always shown in its ASCII form. A Unicode host such as one with a
Cyrillic `а` in place of a Latin `a` appears as `xn--…`, the name the page
actually loaded, so it cannot pass for a familiar site. Chrome shows Unicode
hosts when a set of script-mixing rules says they are safe; implementing those
rules honestly is substantial work, and P1-14's truthful security display
gains more from never showing a deceptive host than from showing a pretty
one. Ctrl L still edits the raw address, so editing and resubmitting can
never change it by accident.

## Proving that typing sends nothing (D9)

Typing in the palette runs `PaletteView::query`, which ranks commands on the
UI thread and sends `SessionCommand::Query` to the session worker, which
ranks the in-memory `atlas-search` catalogue. Only Enter, a click or Alt
Enter reaches `navigate`, and only `navigate` asks a page to load anything.
`cargo xtask check-deps` (part of `cargo xtask check`) now keeps that path
free of network code structurally, in three parts:

1. **No network client crate in any Atlas crate.** `dependency_policy.rs`
   rejects a direct dependency of any workspace member on a client or socket
   crate (`reqwest`, `hyper`, `ureq`, `tokio`, `socket2`, `mio` and others in
   `NETWORK_CRATES`), including dev, build, optional and renamed ones, and any
   `windows` feature that exposes Windows networking (`Win32_Networking*`,
   `Win32_NetworkManagement*`, `Win32_Web*`, `Networking*`, `Web*`). The same
   pattern already confines `atlas-update` to cryptography.
2. **The headless crates cannot reach one at all.** The check now reads
   Cargo's full resolved graph (it previously used `--no-deps`) and walks it
   from `atlas-core`, `atlas-search`, `atlas-store` and the other headless
   crates. Any path to a network crate, however indirect, fails with that
   path printed. This is where the typed text is parsed and ranked.
3. **No socket or HTTP API on the typing path.** Standard-library sockets need
   no dependency, so `network_sources.rs` reads the source of the headless
   crates, the palette and `navigation.rs` and rejects `TcpStream`,
   `TcpListener`, `UdpSocket`, `ToSocketAddrs`, WinHTTP/WinINet entry points
   and `http_client`.

What this does **not** prove: `atlas-app` depends on GPUI, and GPUI itself
depends on an HTTP client (`gpui-pre-http-client`, `gpui-pre-reqwest`) for
loading remote images. Atlas never passes GPUI a remote image, and part 1
stops `atlas-app` from using such a crate directly, but the check cannot rule
out that a later GPUI call fetches something. WebView2 is also, by design,
the network for pages; the check proves that typing does not drive it, not
that pages are offline. A text scan can be evaded by deliberate aliasing; it
is a tripwire against accidents, which review backs up.

## Also in this branch

- **Home page date (P1-28).** `atlas_platform::calendar::local_date_heading`
  calls Windows' `GetDateFormatEx` with the canvas's `dddd, MMMM d` picture
  ("Friday, October 9") in the owner's display language and local time zone.
  It adds the `Win32_Globalization` feature to the existing `windows =0.62.2`
  pin; no package and no lockfile change. If Windows refuses, the header says
  "Date unavailable". The date is read at each render, so a window left idle
  past midnight keeps showing yesterday until it next redraws.
- **Clear browsing data from the palette (P1-27).** The registry gains
  `browser.clear-data`, "Clear browsing data…", with no shortcut. It opens
  P1-27's existing dialog, which still asks before deleting anything.
- **Ledger.** `docs/implementation-status.md` now records P1-06, P1-17,
  P1-19/20, P1-28/29 and P1-30/31 as merged into the integration branch with
  owner checks pending, and P1-32's state.

## Files

- `crates/atlas-core/src/address_entry.rs`, `address_display.rs` (new, with
  tests), `lib.rs`, `commands.rs`.
- `crates/atlas-app/src/navigation.rs` (now an adapter), `ui.rs`,
  `ui/home.rs`.
- `crates/atlas-platform/src/calendar.rs` (new), `lib.rs`, `Cargo.toml`
  (one `windows` feature).
- `crates/xtask/src/dependency_policy.rs`, `network_sources.rs` (new),
  `main.rs`.
- `docs/implementation-status.md`, this note.

## What the owner checks in the running app

Use synthetic addresses only. Nothing here needs a signed-in account.

1. Press Ctrl K and type `example.com`, then Enter: `https://example.com/`
   opens. Type `example.com` and press Alt Enter: a Google search for
   "example.com" opens.
2. With a local server running (or none), type `localhost:8080` and Enter:
   Atlas goes to `http://localhost:8080/` (a connection error page is fine).
3. Type `1.5`, `e.g.` and `two words`, each with Enter: each is a Google
   search.
4. Open `https://example.com/caf%C3%A9`: the address field reads
   `https://example.com/café`. Ctrl L shows the raw address for editing.
5. Open a page on a Unicode host, for example type `bücher.example`: the
   field shows `xn--bcher-kva.example` (an error page is expected).
6. On Home, the header shows today's date in the canvas's form, for example
   "Friday, October 9", matching the Windows clock.
7. Press Ctrl K, type `clear`, and choose "Clear browsing data…": the dialog
   opens in the page card; Cancel leaves everything as it was.

## Owner questions

1. Should bare private-network addresses such as `192.168.1.1` open over
   `http` like loopback ones (routers and NAS boxes rarely have certificates),
   or stay on `https`? Recommendation: `http` for private ranges too,
   because an `https` attempt to a router usually just fails and Atlas has no
   automatic fallback.
2. Should Atlas ever display Unicode host names? Recommendation: no for v1;
   keep punycode, and revisit only with Chrome's full IDN rules.
