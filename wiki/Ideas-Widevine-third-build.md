# Idea: a third build for Widevine

> **Status: proposal, not approved.** Written 2026-10-15 for the owner's review; it writes no code and decides nothing. It relates to D2, D3 and the owner's 2026-10-15 decision that each platform's build uses its own system web engine and bundles no Chromium ([`docs/decisions.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/docs/decisions.md)).
>
> Moved from `docs/proposals/widevine-third-build.md` ([original at af3de53](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/proposals/widevine-third-build.md)) on 2026-10-16.

## Why this proposal exists

On 2026-10-15 the owner decided that each Syrinx build uses its platform's
own web engine: WebView2 on Windows and WKWebView on macOS, with no bundled
Chromium ([`docs/decisions.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/docs/decisions.md)). The owner also wants Disney+, Hulu, YouTube
and Google's services to work properly everywhere Syrinx runs. Those
services protect video with DRM (digital rights management): the page asks
the browser, through the EME web API (Encrypted Media Extensions), for a
decryption module, called a CDM (Content Decryption Module). Google's CDM is
Widevine; Apple's is FairPlay; Microsoft's is PlayReady.

The two system engines already cover DRM on their own platforms. WebView2
ships Widevine inside Microsoft's runtime ([note 07](Background-07-google-sign-in-and-drm), section 2.1), and
WKWebView has FairPlay, which Disney+, Hulu and Netflix use in Safari. So a
third build is only needed where no system engine brings a CDM the services
accept. In practice that is Linux: its system web engine, WebKitGTK, has no
Widevine, and D2 already keeps Linux out of v1. It could also become needed
on macOS if a service the owner uses turns out to be Widevine-only, or
refuses a WKWebView browser that is not Safari. The macOS spike answers that.

This document answers the owner's question, sets out the options, and
recommends one. It writes no code and decides nothing.

## Does Widevine need all of Chromium?

**No, but it needs an engine that can host it, and in practice only two
exist.** The Widevine CDM is a separate, closed binary of a few tens of
megabytes (`widevinecdm.dll`, `libwidevinecdm.so`). It is not part of
Chromium; it plugs into a browser through a defined CDM interface. Firefox
proves the point: it is not Chromium, and it loads the same Widevine module
through its own plugin host (Gecko Media Plugins).

What a Widevine build needs, whatever the engine:

1. **An engine that implements EME and hosts the CDM.** Chromium (and
   everything built on it: Chrome, Edge, WebView2, CEF, Electron) and Gecko
   do. WebKit does only in set-top-box builds (WPE WebKit with GStreamer and
   an "OpenCDM" bridge), where the device maker licenses the Widevine source
   from Google. ⚠️ Unverified for a desktop browser.
2. **A licence from Google.** Google's Widevine page says a licence
   agreement is required to use Widevine
   ([overview](https://developers.google.com/widevine/drm/overview)). WebView2
   avoids this for Syrinx because Microsoft distributes the module, not
   Syrinx. A third build would need its own agreement; whether Google grants
   one to a single-developer browser is unknown.
3. **Signing for the Verified Media Path on Windows and macOS.** There,
   Widevine checks that the host program is signed through Google's VMP
   process before services serve full quality. Linux has no such check.
   This is why a Linux third build is easier than a Windows or macOS one.
4. **A way to deliver and update the module.** Chromium downloads it with
   its component updater; Firefox downloads it from Google. Syrinx would
   have to do the same, which is a network call Syrinx makes on its own and
   needs the owner's approval ([`AGENTS.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/AGENTS.md)).

## The options

**A. WebKitGTK with a Widevine bridge (no Chromium).** Keeps one engine
family with macOS (WebKit) and stays small. But the Widevine bridge exists
only in licensed set-top-box stacks, the CDM source is licensed to device
makers, and wry's Linux child web views are X11-only ([note 06](Background-06-engine-and-ui-performance)). Rejected
unless Google offers a licence path, which is unlikely.

**B. A Gecko build (no Chromium).** Gecko hosts Widevine today. But Gecko
has no desktop embedding API, so this means maintaining a Firefox fork
rebased on every release; the owner dropped that on 2026-10-09. Rejected.

**C. A Chromium "media engine" used only for protected video, downloaded on
demand.** Syrinx stays on its system engine for every tab, and only a site
the owner marks as needing Widevine (or one that fails EME on the system
engine) opens in a CEF-backed tab. This reuses D3's plan to add a second
engine per tab (Servo later). Chromium's own settings and interface never
appear: CEF is the rendering engine only, and Syrinx draws its own chrome
and settings, as it does over WebView2 today. Costs: the engine is still
Chromium, roughly a few hundred megabytes, updated every four to eight weeks
([note 01](Background-01-shell-and-engine)); it needs the Widevine licence above; and Google's
embedded-framework sign-in block names CEF, so Google's own sites must
stay on the system engine. Downloading it only when first needed keeps it
out of the installer for people who never use it.

**D. No third build; DRM only where the system engine provides it.**
Windows and macOS cover the owner's services; Linux ships without protected
video, flagged as a known limit, like the absence of Chrome extensions.
Costs nothing now.

## Recommendation

**D now, and C only if a real need appears.** The owner's machine is
Windows, where DRM already works; macOS is covered by FairPlay if the
spike confirms it; and Linux is outside v1 (D2). Option C is the only
realistic Widevine route without a Firefox fork, but it is a bundled
Chromium in all but name, which is what the owner set out to avoid, and it
depends on a Google licence that has not been asked for. Revisit it if the
macOS spike finds a service that refuses WKWebView, or when Linux is
planned.

## What would settle it

How the licence, signing and delivery would actually be done, and what
Syrinx looks like after a Widevine update, is in
[`docs/widevine-licence-guide.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/docs/widevine-licence-guide.md).

- The macOS spike, run on a Mac by the owner: Disney+, Hulu, a YouTube
  rental and Google sign-in in a WKWebView build, recording which key
  system each uses.
- If option C is pursued: an inquiry to Google about a Widevine licence and
  VMP signing for Syrinx, sent only with the owner's approval.
- If Linux is planned: a check of which of the owner's services support
  Linux at all, and at what resolution, before any engine work.
