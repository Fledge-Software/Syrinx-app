# Web engines everywhere: system engines plus a Widevine media engine

> **Status:** Owner-approved direction, 2026-10-28, recorded in
> `docs/decisions.md` § 2026-10-28. It approves no dependency, contract,
> network call or CI change, and starts no work. v1 stays Windows 11 only
> (D2).
>
> **Tracking:** epic #211, with slices #204 to #210 (section 6).

## 1. Why this plan exists

The owner wants Syrinx usable on any desktop computer, with Disney+, Hulu,
YouTube and Google's services working on each one. Those services protect
video with DRM (digital rights management): a page asks the browser, through
the EME web API (Encrypted Media Extensions), for a decryption module, a CDM
(Content Decryption Module). Google's CDM is Widevine and Apple's is FairPlay.

On 2026-10-15 the owner decided that each platform's build uses that
platform's own web engine and that Syrinx bundles no Chromium
(`docs/decisions.md`). That works on two platforms:

- **Windows:** WebView2 ships Widevine inside Microsoft's runtime, and
  Microsoft holds the licence. Disney+ passed in Phase 0.
- **macOS:** WKWebView provides FairPlay, which Disney+, Hulu and Netflix use
  in Safari. ⚠️ Unverified until the macOS spike: that those services accept
  a WKWebView browser that is not Safari.

It does not work on Linux. Linux's system engine, WebKitGTK, cannot load
Widevine, and only two desktop engines can: Chromium and Firefox's Gecko.
Gecko has no embedding API, so it would mean maintaining a Firefox fork,
which the owner dropped on 2026-10-09. Without this plan, Syrinx on Linux
ships with no protected video at all.

The owner's direction (2026-10-27 and 2026-10-28, in chat): build an
optional Chromium-based **media engine** with Widevine during the macOS work,
get the Widevine certificate through **castLabs**, and once Widevine works,
update Syrinx so it runs on Windows, macOS and Linux.

**Assumption:** "anywhere on anything" means the three desktop platforms.
Phones and tablets are not covered: GPUI has no mobile backend, and the DRM
rules there differ. Say so if mobile was meant.

## 2. The design: two kinds of engine

Every tab keeps using its platform's **system engine**. Only sites the owner
marks as needing protected video open in the **media engine**.

| Platform | Ordinary tabs | Protected video | Media engine needed? |
|---|---|---|---|
| Windows | WebView2 | WebView2's own Widevine | No |
| macOS | WKWebView | FairPlay in WKWebView | Only if a service refuses WKWebView |
| Linux | WebKitGTK | Media engine | Yes |

What the media engine is and is not:

- **It is an engine, not Chrome.** It is Chromium's content layer, either
  CEF (Chromium Embedded Framework) or castLabs' Electron for Content
  Security (ECS). Chrome's interface, settings, sync, Google account and
  extension store are not in it. Syrinx draws its own interface around it,
  as it does around WebView2.
- **It cannot be trimmed down to "just Widevine".** Widevine only decrypts.
  Disney+ and Hulu are full web apps, so the engine needs Chromium's
  renderer, JavaScript engine, networking and media pipeline. Expect a
  couple of hundred megabytes on disk. A self-built, stripped Chromium saves
  little and would mean building Chromium on every release; use prebuilt
  binaries.
- **It is optional and downloaded on request.** Nothing is installed until
  the owner asks for protected-video support. Someone who never asks never
  carries its size or its updates.
- **It updates on Chromium's schedule.** Chromium ships a major version every
  four weeks with security fixes in between, often weekly. The media engine
  must follow, because an old engine carries known exploits and Google
  revokes old Widevine versions, which stops playback.
- **Google's own sites never use it.** Google's embedded-framework sign-in
  block names CEF, and the system engines already work for Google.
- **It has its own cookies.** The owner signs in to Disney+ or Hulu once more
  inside it; cookies cannot be shared across engines.
- **Quality is L3, Widevine's software-only level,** as in Chrome and
  WebView2: 720p or 1080p depending on the service, never 4K.

In the code, the generic tab model gains an engine field (system or media),
which D3 already anticipates for a second engine. The sidebar and palette
read the same model; only `syrinx-platform` grows a second engine backend. A
media-engine tab carries a small badge in the sidebar so it is clear which
engine renders it.

## 3. Getting the Widevine certificate through castLabs

Loading Widevine needs a licence agreement and, on Windows and macOS,
signing for Google's VMP (Verified Media Path): a check that the program
loading Widevine is the genuine signed build. Without VMP, services refuse
to play or drop the resolution. Linux has no VMP; there Widevine reports
itself unverified and offline downloads do not work.

The owner chose castLabs, a DRM company that runs a Widevine third-party
licensing programme for small developers. castLabs offers two routes, and
the choice decides the media engine:

- **castLabs' Electron (ECS) with EVS signing.** EVS is castLabs' free
  signing service for apps built on ECS. This is the defined, cheapest path.
  Its cost is that the media engine becomes an Electron runtime, which runs
  as a separate program rather than a library embedded in Syrinx.
- **CEF with castLabs' commercial VMP certification** for a custom Chromium
  build. CEF embeds more naturally beside wry, but the certification is paid
  and its terms are not public.

Choose the engine after castLabs answers. The questions to ask are in
section 7.

Before applying, prepare (the owner does these; an agent can draft):

1. **The publisher:** apply as Fledge Software, so the agreement outlives any
   one person. The owner signs it, never an agent.
2. **A shipped product:** a released, maintained Windows Syrinx with release
   notes. The weekly previews help.
3. **Code signing:** Authenticode on Windows and a Developer ID with
   notarization on macOS. VMP sits on top of it, and it was deferred until
   Syrinx is productised.
4. **A security and update story:** the signed update channel, the sandbox
   kept on, and the CDM never modified.
5. **A privacy policy** that names the CDM download and Google's role.
6. **Straight answers:** platforms, expected users, services, and that the
   source is open (MPL-2.0) while the CDM stays closed and the signing keys
   never enter the repository.

Apply early. The application can take months, and a refusal means Linux
ships without protected video, so its answer should come before the engine
work starts.

### Signing on every release

VMP covers the final bytes, so the order is fixed: build the media engine,
code-sign it, produce the VMP signatures (or send the package to EVS), then
package and publish. Changing any byte afterwards breaks VMP. The VMP
certificate lives in the release pipeline's secret store, never in the
repository, a log or a chat. Adding this step changes
`.github/workflows/weekly-release.yml`, which needs the owner's approval.

### Delivering the CDM

The usual pattern is that the engine downloads Widevine from Google rather
than shipping it. In Syrinx that is a network call Syrinx makes on its own,
which needs the owner's approval. It must:

- download nothing until the owner chooses to install protected-video
  support;
- verify the module's hash and signature before loading it, as Syrinx does
  for its own updates;
- check for a new CDM on a schedule the owner approves;
- be disclosed in the privacy note, next to the WebView2 and SmartScreen
  disclosures.

## 4. What the other platforms need besides DRM

DRM is the hardest gap, not the only one. Everything outside
`syrinx-platform` already carries over: `syrinx-core`, `syrinx-store`,
`syrinx-search`, the plugin host and the CodeMirror bundle. The rest:

- **GPUI:** Syrinx pins `gpui-pre-windows`, and both vendor patches (D37's
  idle repaint and the embedded shaders) are Windows-only. Each platform
  needs its GPUI crates audited, pinned with `=` and measured for idle CPU.
- **macOS:** a WKWebView backend for `syrinx-platform`, with the owner's
  spike on a Mac first.
- **Linux, embedding:** wry embeds a child web view on Linux only under X11,
  not Wayland, and GPUI draws its own window rather than a GTK one. Fitting
  WebKitGTK and the media engine into the page card on Wayland is unsolved.
- **Linux, popups:** the palette and peek are separate windows placed at
  exact positions, which Wayland does not allow; they become xdg-popups or
  are redesigned.
- **Per-feature counterparts:** captured shortcuts, background-tab
  suspension, page-text extraction (today it uses a Chrome DevTools Protocol
  call that WebKitGTK lacks, and it must still inject nothing on excluded
  sites), permissions, certificate errors, popups, find, local crash dumps
  and the synced-folder check each need a version per engine.
- **Phishing protection:** SmartScreen has no WebKitGTK equivalent; Linux
  needs a replacement or a disclosed limit.
- **Updates and install:** WinHTTP, Windows key storage and the PowerShell
  installer each need a macOS and a Linux counterpart.
- **The gate:** `cargo xtask check` and the native fixtures need a macOS and
  a Linux runner, and the owner needs a machine for each.

## 5. What this changes, and what it needs approved

- **The 2026-10-15 "no bundled Chromium" decision gets one exception:** an
  optional media engine downloaded on request and used only for protected
  video, recorded in `docs/decisions.md` § 2026-10-28.
- **D2 is unchanged until v1 ships.** v1 is Windows only, and none of this
  starts before it.
- **⚠️ Licence audit:** Chromium bundles code under several licences, and its
  media stack includes FFmpeg built under the LGPL. The LGPL is not among the
  licences `AGENTS.md` allows, so the media engine needs a full audit and,
  if LGPL code is in it, the owner's explicit exception, as wasmtime's was.
- **Approvals still needed:** the castLabs agreement, the CDM download as a
  permitted network call, the signing step in CI, and the media engine's
  dependency audit.

## 6. Order of work

| Phase | Issue | Who | What |
|---|---|---|---|
| Now to v1 | | Agents and owner | Ship Windows on WebView2. Nothing here is needed. |
| Before v1 ships | #204 | Owner | Set up code signing and a privacy policy; contact castLabs (an agent drafts); choose the engine from the answer. |
| macOS spike | #205 | Owner on a Mac | Disney+, Hulu, a YouTube rental and Google sign-in in a WKWebView build, recording each key system. |
| Licence audit | #206 | Agent, owner decides | The chosen engine's licences, including FFmpeg's LGPL. |
| Media engine spike | #207 | Agent, reviewed | After castLabs answers: the chosen engine on one platform, a test stream, then Disney+ played by the owner. |
| macOS build | #208 | Agents, owner approves each slice | WKWebView backend, the media engine as a fallback if the spike needs it, VMP signing in CI. |
| Linux spike | #209 | Owner on Linux | WebKitGTK on Wayland inside a GPUI window, Google sign-in, the media engine for Disney+. |
| Linux build | #210 | Agents, owner approves each slice | WebKitGTK backend, the media engine for protected video, Linux packaging. |
| Everywhere | #211 | Owner | Syrinx is updated to ship on Windows, macOS and Linux. |

## 7. Open questions

- **For castLabs:** does EVS cover production use of ECS without a direct
  Google agreement? What does VMP certification for a CEF build cost? Does
  the agreement allow bundling the CDM, or must it come from Google? Is an
  open-source browser accepted?
- Which of the owner's services play on Linux at all, and at what resolution?
- Can the media engine sit inside the page card under Wayland, or must it
  open in its own window?
- Does ECS, running as a separate program, keep Syrinx's focus and shortcut
  rules (the traps in `docs/dev/trapstoknow.md`)?

## Related

- `docs/widevine-licence-guide.md`: the licence, VMP and delivery details.
- The archived proposal, read at `af3de53`:
  `docs/proposals/widevine-third-build.md` (options A to D; this plan is
  option C).
- `docs/decisions.md`, 2026-10-15: one system engine per platform.
- Research note 07 (wiki `Background-07-google-sign-in-and-drm`) for
  Widevine in WebView2.
