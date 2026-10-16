---
status: guide for the owner; nothing here is approved or started
decisions: D2, D3; the 2026-10-15 per-platform engine decision
related: "the wiki's Ideas-Widevine-third-build page (was docs/proposals/widevine-third-build.md) and Background-07-google-sign-in-and-drm page (was docs/research/07-google-sign-in-and-drm.md), both also at af3de53"
date: 2026-10-15
---

# Getting Widevine into Syrinx: licence, signing and delivery

## Why this guide exists

The owner plans to ship a usable Syrinx on Windows first (v1, maybe v2 or
v3), then apply to Google for a Widevine licence and add Widevine to Syrinx
in a later update. This guide explains the three things that update needs:
a licence from Google, signing for Google's Verified Media Path, and a way
to download and update the Widevine module. It then describes what Syrinx
looks like once the update ships. It decides nothing; each step that
touches money, contracts, keys, CI or network calls needs the owner's
approval when the time comes.

Claims marked ⚠️ are unverified. Google does not publish its Widevine terms,
so parts of this come from other projects' public accounts and need
confirming with Google or a Widevine partner before anything is built on
them.

## Read this first: what the licence does and does not change

**On Windows, Syrinx already plays Widevine video without a licence.**
WebView2, the engine Syrinx uses, ships Google's Widevine module inside
Microsoft's runtime. Microsoft holds the agreement and distributes the
module, so Syrinx needs nothing (note 07, section 2.1; Disney+ passed in
Phase 0). A Widevine licence of Syrinx's own adds nothing to WebView2: it
cannot raise WebView2's security level, unlock higher resolution or change
which services play there. Higher resolution on Windows would come from
PlayReady with hardware support, which is Microsoft's, not Google's.

**On macOS, the plan uses Apple's FairPlay**, which WKWebView provides
(`docs/decisions.md`, 2026-10-15). WebKit cannot load Widevine at all, so a
licence does not help there either.

**So the licence matters only for a Chromium-based "media engine"** of
Syrinx's own: option C in [`docs/proposals/widevine-third-build.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/proposals/widevine-third-build.md). That
is the route for protected video on Linux, and a fallback on macOS if a
service refuses WKWebView. The owner should weigh this before spending
months on an application: if Syrinx never ships on Linux and the macOS
spike passes, the licence buys nothing.

## The terms, in plain words

- **EME (Encrypted Media Extensions)** is the web API a page such as
  Disney+ uses to ask the browser: "can you decrypt this video?"
- **A CDM (Content Decryption Module)** is the closed binary that does the
  decrypting. Widevine is Google's CDM (`widevinecdm.dll` on Windows,
  `libwidevinecdm.so` on Linux). It is not part of Chromium's source; it is
  downloaded separately and loaded by the browser.
- **Two different "licences" are involved; don't confuse them.** The
  *licence agreement* is a contract between Syrinx's publisher and Google
  that allows Syrinx to load the CDM. A *DRM licence* is the key a service's
  server (Disney+'s, Hulu's) sends to the CDM each time a video plays. The
  services handle the second; Syrinx only needs the first.
- **Security levels.** L3 is software-only, which is what every desktop
  browser gets, Chrome and WebView2 included. L1 needs the decryption to
  happen in the graphics hardware and is reserved for devices Google
  certifies. Expect L3, and so 720p or 1080p depending on the service, never
  4K, whatever route Syrinx takes.
- **VMP (Verified Media Path)** is Google's check, on Windows and macOS,
  that the program loading the CDM is the genuine, unmodified program that
  was licensed. It reads a signature file shipped next to each program
  file. Services can refuse to play, or play at low resolution, when VMP
  fails. Linux has no VMP; there the CDM reports itself as unverified, and
  persistent licences (offline downloads) do not work.
- **Revocation.** Google keeps a list of valid CDM versions and host
  certificates. An old CDM or a revoked certificate stops playing even with
  a licence, so the CDM must keep updating.

## Step 1: the licence agreement

### Two routes

**Route A: directly with Google.** Google's Widevine page says a licence
agreement is required to use Widevine products, and it has a contact form
for partners ([Widevine overview](https://developers.google.com/widevine/drm/overview)).
Browser makers that have one include Brave, Vivaldi, Opera and Microsoft.
The risk is that Google says no or does not answer: in 2019 Samuel Maddock,
building the open-source Electron browser Metastream, waited four months
and was refused with "we're not supporting an open source solution like
this" ([his account](https://blog.samuelmaddock.com/posts/google-widevine-blocked-my-browser/);
[The Register](https://www.theregister.com/2019/04/03/googles_widevine_drm/)).
Syrinx is open source under MPL-2.0, so that answer may apply. ⚠️ Google's
current policy is not published; it may have changed since 2019.

**Route B: through a Widevine partner.** castLabs, a DRM company, runs a
Widevine "3PL" (third-party licensing) programme and offers commercial VMP
certification for custom Chromium and Electron builds. It also runs EVS, a
free signing service for apps built on its Electron for Content Security
(ECS) releases ([castlabs-evs](https://pypi.org/project/castlabs-evs)).
This is the route most small apps take. If Syrinx's media engine were
castLabs' Electron rather than plain CEF, EVS could sign it without a direct
Google contract. ⚠️ Whether production use of EVS needs a separate Google
agreement, and what castLabs charges for custom-build certification, are
not public; ask castLabs.

**Recommendation:** ask castLabs first, because it has a defined process
and handles small developers; apply to Google directly in parallel. Choose
the media engine (CEF or castLabs' Electron) after the answers, since the
signing route depends on it.

### What to prepare before applying

Google or castLabs will want to see a real product and a real publisher.
Prepare, in roughly this order:

1. **A legal entity.** Apply as Fledge Software, not as an individual, so
   the agreement outlives any one person. The agreement is a contract; the
   owner signs it, never an agent (`AGENTS.md`, "Operator work").
2. **A public, released product.** A shipped Windows Syrinx with release
   notes and a download page shows it is maintained. The weekly preview
   releases (`docs/releasing.md`) help.
3. **Code signing.** VMP signing sits on top of ordinary code signing:
   Authenticode on Windows, a Developer ID with notarization on macOS. The
   2026-10-09 decision deferred signing until Syrinx is productised, so this
   comes back first.
4. **A security and update story.** Show that Syrinx updates promptly
   (the signed update channel from P1-38 and P1-39), keeps the sandbox on,
   and never modifies the CDM.
5. **A privacy policy** that names the CDM download and Google's role.
6. **Answers to the usual questions:** which platforms, how many users,
   which services you expect to play, and whether the source is open. Be
   straightforward that it is open source; the CDM itself stays closed and
   separate, and the signing keys never enter the repository.

## Step 2: VMP signing

### What gets signed

Each executable and library in the media engine that touches the CDM gets
a companion signature file, made with a certificate Google (or castLabs)
issues after licensing. Chrome ships `chrome.exe.sig` beside `chrome.exe`;
Syrinx's media engine would ship, for example, `syrinx-media.exe.sig`.
⚠️ The exact file list depends on the engine and the signing tool supplied.

### Where it fits in a release

Signing happens on every release, after the build and after ordinary code
signing, because VMP covers the final bytes:

1. Build the media engine.
2. Sign it with Authenticode (Windows) or Developer ID (macOS), and
   notarize on macOS.
3. Produce the VMP signature files with the issued certificate (or send the
   package to castLabs' EVS, which returns it signed).
4. Package and publish.

Changing any byte afterwards, including a re-signing step out of order,
breaks VMP. On Linux steps 2 and 3 are skipped.

### Keeping the keys safe

The VMP certificate and its private key are secrets. They live in the
release pipeline's secret store, never in the repository, a log or a chat
(`AGENTS.md`, "Keys and tokens never go into the repository"). Adding the
signing step changes `.github/workflows/weekly-release.yml`, which needs
the owner's approval.

## Step 3: delivering and updating the CDM

Syrinx cannot simply put the CDM in its installer unless the agreement
allows redistribution. ⚠️ The usual pattern is that the browser downloads
it from Google:

- **Chromium's component updater** fetches the CDM from Google's servers
  and keeps it current. CEF does this by default
  ([an Eclipse project's note on disabling it](https://gitlab.eclipse.org/eclipse/set/browser/-/merge_requests/16);
  [CEF's loader source](https://third-party-mirror.googlesource.com/cef/+/refs/heads/master/src/libcef/common/widevine_loader.cc)),
  and castLabs' Electron builds use the same mechanism.
- **Firefox** downloads it from Google using a manifest Mozilla publishes,
  after asking the user.

For Syrinx this is a network call Syrinx makes on its own, which needs the
owner's approval (`AGENTS.md`). The design that fits Syrinx's rules:

- **Ask first.** Download nothing until the owner chooses to install
  protected-video support, the way Firefox and Brave ask.
- **Verify what arrives.** Check the module's hash and signature before
  loading it, as P1-38 does for Syrinx's own updates.
- **Keep it current.** Check for a new CDM on a schedule the owner
  approves; an out-of-date CDM is revoked and stops playing.
- **Disclose it** in the privacy note, next to the existing WebView2 and
  SmartScreen disclosures.

## What Syrinx looks like after the Widevine update

### What stays the same

- **Windows:** every tab still uses WebView2, and DRM works exactly as it
  does today. The update changes nothing here unless a service turns out
  to refuse WebView2.
- **macOS:** every tab still uses WKWebView with FairPlay.
- **Google's sites** never move to the media engine: Google's sign-in block
  names CEF, and WebView2 and WKWebView already work for Google.
- **Syrinx's own interface and settings.** The media engine renders pages
  only; Chrome's menus and settings pages never appear.

### What is new

- **A "protected media" component**, downloaded only on request, a few
  hundred megabytes. It holds the media engine (CEF or castLabs' Electron)
  as a separate, VMP-signed program, `syrinx-media`, plus the Widevine CDM
  it downloads from Google.
- **A per-site choice.** A site that needs Widevine and fails on the system
  engine shows a Syrinx prompt: "This site needs protected video. Install
  protected-media support?" After installing, the owner can mark the site
  to open in the media engine. Settings gains a list of those sites, the
  engine and CDM versions, and a Remove button.
- **A tab marker.** A media-engine tab carries a small badge in the
  sidebar, so it is clear which engine is rendering it.
- **A separate sign-in.** The media engine keeps its own cookies, so the
  owner signs in to Disney+ or Hulu once more inside it. Sharing cookies
  across engines is not possible.
- **Linux becomes possible with DRM**: WebKitGTK for ordinary tabs, the
  media engine for protected video.

### What changes underneath

- **One tab model, two engine kinds.** The generic tab model gains an
  engine field (system or media), which D3 already anticipates for Servo.
  The sidebar and palette read the same model; only `syrinx-platform` grows
  a second engine backend.
- **A heavier release.** The media engine follows Chromium's releases every
  four to eight weeks (note 01), plus VMP signing on each, on top of
  Syrinx's own weekly preview.
- **New approvals recorded in `docs/decisions.md`:** the licence or
  partner agreement, the CDM download as a permitted network call, the
  signing step in CI, and a new dependency audit for the media engine.

## The order of work

| When | Who | What |
|---|---|---|
| Now to v1–v3 | Agents and owner | Ship Windows on WebView2. Nothing Widevine-specific is needed. |
| Before applying | Owner | Decide whether Linux or a macOS fallback is wanted; if neither, stop here. Set up the legal entity, code-signing certificates and a privacy policy. |
| Applying | Owner, with an agent drafting | Contact castLabs and Google. An agent can draft the letters and answers; the owner sends them and signs any agreement. |
| After approval | Agent, reviewed | Spike the media engine (CEF or castLabs' Electron) on one platform: a test stream, then Disney+ played by the owner. |
| Then | Agent, owner approves each | Add the signing step to the release workflow, the on-request CDM download, the per-site setting and the tab marker, as an approved plan with slices. |

## Open questions

- Will Google license an open-source, single-developer browser today?
  Only an application answers this.
- Does castLabs' EVS cover production use of its Electron builds without a
  direct Google agreement, and what does custom-build certification cost?
- Does the agreement allow Syrinx to bundle the CDM, or must it be
  downloaded from Google?
- Which of the owner's services play on Linux at all, and at what
  resolution?

## Sources

- [Widevine overview](https://developers.google.com/widevine/drm/overview), Google.
- [Google Widevine blocked my browser](https://blog.samuelmaddock.com/posts/google-widevine-blocked-my-browser/), Samuel Maddock, 2019, and [The Register's report](https://www.theregister.com/2019/04/03/googles_widevine_drm/).
- [castlabs-evs on PyPI](https://pypi.org/project/castlabs-evs), castLabs' signing client, and [castLabs Electron releases](https://github.com/castlabs/electron-releases/blob/master/docs/api/components.md) on component-updater delivery and Linux's lack of VMP.
- [CEF's Widevine loader](https://third-party-mirror.googlesource.com/cef/+/refs/heads/master/src/libcef/common/widevine_loader.cc) and [an Eclipse project disabling CEF's CDM download](https://gitlab.eclipse.org/eclipse/set/browser/-/merge_requests/16).
- [Kodi inputstream.adaptive on VMP](https://github.com/xbmc/inputstream.adaptive/wiki/Verified-Media-Path-%28VMP%29).
- [`docs/research/07-google-sign-in-and-drm.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/research/07-google-sign-in-and-drm.md) for Widevine in WebView2 on the owner's machine.
