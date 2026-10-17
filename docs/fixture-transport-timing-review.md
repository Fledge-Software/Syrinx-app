# Test socket lifetime around the fixture's final log

This review, dated 2026-10-11, addresses the intermittent synthetic navigation
failure. The owner authorized the exact four-run comparison below. Independent
source review and execution are complete: both variants passed both runs.
The original proposal and protocol remain below, followed by the measured
outcome. The failed intermediary prototype and all previous results remain frozen.

Recommend comparing the preserved diagnostic fixture v4 with one disposable
copy that releases its `TcpStream` immediately before its existing final
server log. This changes socket ownership across that log while retaining
the other server work. It introduces no sleep or production change.

## Why this is the next comparison

The fixed earlier v2/v4 series has three v2 failures and three v4 passes.
V4 changes only the fixture server's counters and logging; its private page
callback module is byte-identical to v2. The changed example logs at accept,
after complete request headers, and after successful `write_all`, and prints
a final server summary. Thus those results do not isolate the final log or
prove a connection-close problem. They suggest server work or timing matters
in that small sample. The records remain under
`target/continuation-native-server-pair{1,2,3}-{v2,v4}.*`, with interpretation
in `docs/integrated-browser-preview.md`.

The source audit also found 26–29 empty read timeouts and 39–42 accepted
connections in each of the passing v4 runs, with 13 completed headers and
13 written responses. The server handles one accepted stream at a time and
permits a one-second empty-read timeout. Waiting on an empty connection can
therefore delay a later request. That is another timing hypothesis, not a
reason to rewrite the server or claim the final log has already been isolated.

The preserved v4 `respond` function owns its stream by value. It writes the
same response, increments `responses_written`, logs that completion, and
returns; the stream is then dropped. Rust documents that dropping
[`TcpStream`](https://doc.rust-lang.org/std/net/struct.TcpStream.html) closes
its connection. Releasing this owner earlier does not establish when the
peer observes closure: Winsock can complete the disconnect in the background.
[Microsoft's socket-close documentation](https://learn.microsoft.com/en-us/windows/win32/api/winsock/nf-winsock-closesocket)
also distinguishes ordinary close from abortive linger settings. Do not
assume the current fixture sends a reset simply because it drops its stream.

Successful server writes are not proof that WebView2 received or processed
the response. [Winsock's send documentation](https://learn.microsoft.com/en-us/windows/win32/api/winsock2/nf-winsock2-send)
explicitly separates successful sending from recipient delivery. The
existing native navigation error 9 likewise does not identify the server's
close mechanism. This experiment observes application behavior, not TCP
FIN/RST packets or acknowledgments.

## Exact proposed difference

Use **A** for the frozen v4 control and **B** for a fresh v4-derived disposable
copy. In B's `crates/syrinx-platform/examples/page_close.rs::respond`, insert
only the following statement after the existing successful-response counter
increment and before the existing final `eprintln!`:

```rust
drop(stream);
```

Both variants retain the stream through the completed write and the counter
increment. A retains it through the final response log; B releases it first.
Both still perform that same log before the serial server accepts another
connection. This holds the other logging sites and server-loop structure
constant; it does not promise identical scheduling or measured close timing.

Retain every response byte, header, fixture script, request parser, early
EOF/timeout path, native callback, assertion, timeout and watchdog. In
particular, do not relax unsuccessful-navigation or deferred-close assertions.
Add no sleep, new timestamp/log, flush, half-close, linger option, socket clone,
request draining or concurrent server rewrite. Those would add another
variable. The original unlogged v2 remains prior evidence, not the control
for this narrower experiment, because its other server logging differs.

If integration accepts this proposal, prepare B in a fresh ignored directory
under `target/`, verify all v4 source identities first, and record B's full
manifest plus the one-line diff. Require independent review before its
isolated offline build. Do not edit the preserved A copy, the intermediary
prototype, live product/vendor source, dependency graph or public API.

The checked source anchors are:

| Artifact | SHA-256 |
| --- | --- |
| `target/native-stress-diagnostics/workspace_v4-manifest.json` | `6f6399b94f212e71c180525b9618c8272cb4e57d104bc63aa4658614030f37cf` |
| V4 `examples/page_close.rs` | `d2084c0e7c1f0fcfc02a272b7c1c48f376117bcd83893b8e1afc76fa209a4990` |
| Identical v2/v4 `src/browser/page.rs` | `e96481a85462a36babdfd5473d562d3214d8dfbbac2d3f48686e340770daef97` |
| Preserved v4 executable, as recorded by integration | `84766b45fc38577c12293ffb25d67a769f81545dd22a1feb6d937ec977b381eb` |

## Four predeclared runs

Integration alone executes the fixed order **A, B, B, A**, serially. These are
two pairs with reversed order, giving two observations per condition. Verify
each executable hash before the series and do not rebuild within it. Keep
desktop context, executable profile and stdout/stderr redirection identical;
each process uses the unchanged fixture's fresh synthetic profile. Preserve
new, non-overwriting output files, terminal status and executable hash for
each run. Do not use personal profiles or delete existing ones.

An ordinary fixture exit one remains a failure, but does not cancel the
remaining predeclared comparisons. Stop and report a launch/policy failure,
unhandled native crash, or watchdog/deadline failure; keep that result and
mark unexecuted runs explicitly. Never repeat an arm to obtain a pass or
extend the series based on the observed outcomes.

The primary observations are each unchanged fixture verdict and first
failure stage. Existing navigation status, close events and server counts
provide context. A `server-response-written` entry means only that the write
returned success. Do not invent a one-to-one relation between accepted
connection number and navigation ID, or infer cross-thread causality from
merged log arrival order.

| Observed outcome | Bounded interpretation |
| --- | --- |
| Both A runs pass and both B runs fail | Supports sensitivity to releasing the stream before the final log in this fixture; it does not establish the transport mechanism. |
| Both variants pass | Retention through that log was not necessary in these runs; the earlier failure remains unexplained. |
| Both variants fail, or outcomes are mixed | The comparison does not isolate the suspected effect. Preserve stage/status differences without selecting only favorable runs. |
| A fails while B passes | Challenges the simple hypothesis that longer retention across the log explains success; it still does not identify a cause. |

Four runs are a diagnostic comparison, not a reliability estimate or a crash
resolution claim. No result alone authorizes a production delay, assertion
change, intermediary promotion or further transport intervention.

## Keep the separate evidence separate

The intermediary `prototype_v2` built successfully but failed its first native
run at cycle two/tab 101. Its frozen source and failure are recorded in
`docs/webview2-breakpoint-investigation.md`; neither arm above incorporates
that topology change. Combining it with a transport change would obscure
which change affected the result.

Integration also relayed the owner's successful manual check on the live
`0fe3b8daad72ddf3383f025756a5174b39f098d2fa7355d81bfd45eaec89c858`
build: address entry and the Go To row remained stable, Enter dismissed the
palette, and one editable-page to palette to Escape cycle returned typing
to the page without another click. That establishes the observed interaction,
not general focus-race reliability or the cause of `0x80000003`. It is not a
result of the unpromoted intermediary prototype.

## Completed comparison, 2026-10-11

Independent review rehashed all 277 v4/v5 source entries and provenance fields.
The sole difference is the approved `drop(stream);` statement after the response
counter increment and before the existing log. All other 276 files, including
the v2 page callbacks, are identical. The v5 manifest SHA-256 is
`fa5e901418a039884a10ec04d8c00e3640bb236ecf7757bbbc2eb729b78f9cb2`;
the changed example is
`8b8eb2cab30808ee224f515f61ddbae04809b5433dfc4a99ed52da389c92c3fc`.

Integration completed the fixed A, B, B, A series with fresh synthetic profiles.
All four recorded statuses are `completed`, all exits are zero, and every stop
reason is null. The actual preserved executables match their result records:
A is `84766b45fc38577c12293ffb25d67a769f81545dd22a1feb6d937ec977b381eb`;
B is `00fcca09f5313592e111f5dac950d811ec455dd066f5595e0531561bf0b5a28d`.
`target/continuation-socket-close-series.json` has SHA-256
`b40081339240a797f4c12720b2bc4460ec487bd270011da0f771cec4008a7c21`.

Each stdout reports native close, visibility, bounds, retry, all 12 lifecycle
cycles and multi-page shutdown passed. Each stderr has 31 successful navigation
completions with distinct navigation IDs: 13 fixture HTTP 200 completions and
18 HTTP 0 completions. All native error codes are zero. The 13 close callbacks
belong to tab 1 and tabs 100–111; shared callback sequence numbers run from
1 through 44 without gaps. Individual server log counts match each final
summary: 13 completed headers, 13 successful writes, zero empty/partial EOFs
and `failed=false`. These counts do not pair connection IDs with navigation IDs.

| Run | Arm | Exit | Accepted connections | Empty timeouts |
| --- | --- | --- | --- | --- |
| 01 | A / v4 | 0 | 39 | 26 |
| 02 | B / v5 | 0 | 35 | 22 |
| 03 | B / v5 | 0 | 38 | 25 |
| 04 | A / v4 | 0 | 35 | 22 |

Preserve `target/continuation-socket-close-{01-A,02-B,03-B,04-A}.{result.json,stdout.log,stderr.log}`.
All four stdout files have SHA-256
`e5924e50649e3a0e8f1eb2bffb9c7588edbdffe400ce638e438ded956d3d4c46`.
The independently checked per-run identities are:

| Run | Result JSON SHA-256 | Stderr SHA-256 |
| --- | --- | --- |
| 01-A | `a1fe88bb525d9e57d7e71dd3eee172ba478e2797f34425d855ebceabc8c9be65` | `1615686c0d8e4859ed5b45bc8da9dc5234f495bc2bd2efb1b842c13a87dd4af4` |
| 02-B | `e3dc35bae22f2ab5654767a4f3236699c1dd37b62f712ae9132b1d988062fb5f` | `49e297290c0e4f7e9968549dfdd69df8228c42467fcd24ae6c55a51d82bf4cf1` |
| 03-B | `985f24efcc7c7c73828ad6727fbc24ab4bc941a3dc2e50631050b73222e7444f` | `1a2c9ae9c8d88176b00440d2e51c2abc653bdbd301b678d5daac4f1dc8c91c46` |
| 04-A | `c35ab3b91b77e34ce7332d3ccfce316d2cfe7e4cccd0458bd569ac804146d786` | `9bb7057d3baa9f319293727ba89c559c4245dc831d4ae6d1823c7cfa91ef57e9` |

Under the predeclared interpretation, retaining the socket through the final
log was not necessary in these runs. This does not measure TCP close timing,
identify the earlier failure mechanism, establish reliability or resolve the
original breakpoint. No production delay, transport change or intermediary
promotion follows from four passes. Earlier native failures, the failed
intermediary run and narrow manual UI passes retain their separate meaning.

The owner authorized committing all current work after completion and the full
gate. `target/continuation-socket-close-commit-gate.log` ends “Local gate passed”:
dependency checks, formatting, Clippy, all 376 tests, documentation tests and
app smoke pass, with zero failed, ignored or filtered tests. Its SHA-256 is
`ca28dc0a417a3e00929ece8c652c21dc8b7b2e58da6acd820fec72af0a03ca62`.
The live application remains the unchanged `0fe3b8da…` binary identified above.
This records gate evidence and commit authorization, not a completed commit.
Merge, remote, push and publication authorization remain absent.
