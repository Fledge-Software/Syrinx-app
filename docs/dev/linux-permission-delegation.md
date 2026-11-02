# Linux permission delegation

This records the native test that selects the owner's 2026-11-02 permission
decision. WebKitGTK's public permission objects do not expose the requesting
frame's origin. Treating the top-level origin as that frame's identity would
mislead the owner, especially for remembered consent.

The owner said **"test first, then decide"**: if undelegated cross-origin
requests are blocked before Syrinx's handler, adopt top-level delegation.
Otherwise refuse requests whose real principal cannot be established.

## Native observations, 2026-11-02

The test ran the real app under Hyprland and XWayland with WebKitGTK 2.54.1,
inherited `GDK_SCALE=2` and fcitx enabled. The matrix used one fresh scratch
profile, with a distinct loopback port for each case. A parent on `127.0.0.1` embedded `localhost` on
that port: these are different origins. Undelegated frames had no `allow`
attribute and responses had no Permissions-Policy header. Authored test-page
buttons called the web APIs; no scripts, IPC or filters were injected by
Syrinx. Temporary instrumentation at the entry to the native handler counted
only static request type names; it was removed after the experiment.

| Request in undelegated frame | Page result | Handler calls |
| --- | --- | --- |
| Geolocation | permission denied, error code 1 | 0 |
| Notifications | `denied` | 0 |
| Camera | `NotAllowedError` | 0 |
| Microphone | `NotAllowedError` | 0 |

All four APIs were present and reported a secure context. A delegated location
frame reached `WebKitGeolocationPermissionRequest`; a top-level notification
button reached `WebKitNotificationPermissionRequest`. Both were explicitly
denied in Syrinx. Top-level camera and microphone returned
`OverconstrainedError` with no native request: no usable devices were offered
in this sandbox. Their controls therefore do not prove capture works with
hardware. No request was granted, no device identifiers or location data were
collected, and no real site or account was used.

The outcome selects top-level delegation. Syrinx's prompt says
"<origin>, or a site it embeds, wants to …" and its remembered choices remain
keyed by the top-level origin. A combined audio/video request is refused:
one camera choice cannot grant independent microphone consent.

## Regression and source

`crates/syrinx-platform/src/browser_gtk/permission_tests.rs` serves authored
loopback pages and checks three actual engine paths: a top-level location
request reaches the handler, an undelegated cross-origin location request
does not, and a delegated one does. The fixture denies every request and
uses an ephemeral WebKit context. It runs through the ordinary Linux gate;
it does not need location hardware or a grant. The four-kind interactive
matrix above remains explicitly distinct from that automated regression.

[WebKitUIClient.cpp at WebKitGTK 2.54.1](https://github.com/WebKit/WebKit/blob/webkitgtk-2.54.1/Source/WebKit/UIProcess/API/glib/WebKitUIClient.cpp#L259)
forwards geolocation without its frame metadata and constructs the public
media request without exposing the origin. Syrinx cannot recover an origin
through an unsupported getter. Injecting a page bridge, changing the user
agent or modifying request policy was rejected by the standing page rules.
The engine's existing delegation policy remains responsible for blocking
undelegated frames; Syrinx adds no grants or policy exceptions.
