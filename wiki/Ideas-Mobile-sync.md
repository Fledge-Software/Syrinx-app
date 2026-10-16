# Idea: mobile sync

> **Status: deferred.** An outline the owner approved recording on 2026-10-10 for a future mobile phase, after the four v1 plans. It adds no work to v1 and does not change D2 (v1 is Windows 11 only); mobile implementation, dependencies and a delivery plan still need their own review. Later decisions in [`docs/decisions.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/docs/decisions.md) and [`docs/architecture-decisions.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/docs/architecture-decisions.md) win.
>
> Moved from `docs/plans/Syrinx later — Mobile sync outline.md` ([original at af3de53](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/plans/Syrinx%20later%20%E2%80%94%20Mobile%20sync%20outline.md)) on 2026-10-16. "The current issue map" below meant Plan 1's slice table; planned work is now tracked in [issues](https://github.com/Fledge-Software/Syrinx-app/issues).

Syrinx should let people carry their saved destinations and selected settings
between desktop and mobile without requiring a Syrinx-hosted sync service.
On 2026-10-10, the owner requested this future feature: an easy in-app
walkthrough, reuse of existing Tailscale accounts, support for existing
Headscale installations, and an optional cloud alternative later.

This is an outline for the future mobile phase, after the four current v1
plans. It does not add implementation slices to v1 or change D2's Windows 11
scope. The owner approved recording the direction; mobile implementation,
dependencies and the detailed delivery plan still need their own review.

## What server independence means

The product promise is **sync without a Syrinx-hosted server**. Each device
keeps its own local data and exchanges changes with paired Syrinx devices.
Tailscale supplies private network connectivity; Syrinx supplies the sync
protocol, offline queues, conflict handling and device pairing.

Hosted Tailscale still uses a coordination service for device discovery,
keys and network policy. Connections can travel directly between devices
or through encrypted packet relays when necessary. Relays carry encrypted
traffic; they are not durable storage for Syrinx changes waiting for an
offline device. These are networking capabilities, not an existing Syrinx
sync implementation. See Tailscale's [control and data planes](https://tailscale.com/docs/concepts/control-data-planes)
and [connection types](https://tailscale.com/docs/reference/connection-types).

Headscale is a [self-hosted implementation of the coordination server](https://github.com/juanfont/headscale).
It replaces who operates that service; it does not eliminate servers.
Supporting an existing installation should be straightforward for the
user. Deploying and maintaining a new Headscale server is separate work,
with its own reachability and HTTPS requirements. Headscale also has
[configurable relays](https://headscale.net/stable/ref/derp/); using Headscale
alone does not establish that every connection avoids Tailscale-operated
infrastructure.

## The setup walkthrough

Offer an optional **Connect your devices** flow during onboarding and in
Settings. Skipping it leaves Syrinx usable locally. Reuse the installed
official Tailscale client first: this preserves the user's existing network
and avoids requiring a second network identity. Embedding a networking
client inside Syrinx is a possible later alternative, but adds packaging,
platform and compatibility work before its usability benefit is proven.

1. **Choose the existing setup.** Present “I'm new to Tailscale,” “I already
   use Tailscale,” and “I use Headscale.” An existing Tailscale account and
   private network can be reused; Syrinx should not require a new account.
2. **Guide installation and connection.** For new users, provide the
   platform's official installation route, explain the system VPN prompt,
   and guide sign-in in Tailscale. Resume the walkthrough afterward and
   verify connectivity. Existing users skip completed steps. Headscale
   users reuse their connected client or follow platform-specific custom
   server instructions and any required node approval.
3. **Pair Syrinx devices.** Use a short-lived QR code, with an accessible
   alternative, and confirmation of the other device. Pairing establishes
   Syrinx-level trust: membership in the same private network must not by
   itself grant access to Syrinx data. Users can inspect and revoke pairs.
4. **Choose what syncs.** Explain each category before enabling it. The
   initial scope proposed below keeps the first delivery manageable.
5. **Check and finish.** Verify the peer connection and complete an initial
   sync. Explain failures in plain language, preserve completed setup steps,
   and provide a retry action instead of making users start over.

After setup, show last successful sync and pending work, with actionable
states such as “Up to date,” “Waiting for your PC,” “Tailscale disconnected,”
or “Connection blocked by your network policy.” A network connection alone
must not be presented as a completed sync.

Both mobile platforms support custom coordination servers through the
official client: [iOS setup](https://tailscale.com/docs/how-to/set-up-custom-control-server?tab=ios)
and [Android setup](https://tailscale.com/docs/how-to/set-up-custom-control-server?tab=android).
Near plug-and-play use is the target for an existing network. Automatic
detection, app-to-app navigation and return-to-Syrinx behavior are unverified
until prototyped on supported client versions. Installation, account sign-in
and OS permission prompts remain user actions; Syrinx never collects the
user's Tailscale or Headscale login credentials.

## Data and sync design

The proposed first categories are saved pages, folders, Favorites and
settings that make sense on both devices. Notes and additional categories
can follow with explicit controls and their own conflict rules. Exact
categories and defaults belong in the future implementation plan. This
outline does not authorize copying cookies, credentials, browsing history,
page-text caches or Fledge data between devices.

Keep a separate database on each device and exchange individual records or
changes. Never synchronize the live SQLite database or its write-ahead log
as files; D13's unsynced local storage rule still applies. A shared file
copy cannot safely reconcile independent edits on two devices.

The Rust sync engine should own stable cross-device identities, revisions,
durable outgoing changes, acknowledgements, retries and conflict handling.
Repeated delivery must not duplicate records. Deletions need retained
deletion records, often called tombstones, so a device returning after a
long absence cannot silently resurrect removed items. The detailed merge
algorithm, retention rules and treatment of incompatible client versions
remain design work. Conflicting edits must not silently discard user data.

Separate the sync engine from its transport, meaning the mechanism that
delivers changes. Tailscale and Headscale provide a reachable private
network for the same Syrinx protocol. Avoid making sync correctness depend
on Tailscale's hosted account APIs or features absent from Headscale.
Application-level device authentication and end-to-end encryption should
be designed with the later cloud route in mind, including revocation and
key recovery, rather than relying solely on network membership.

## Availability and mobile limits

In the device-only design, peers need an opportunity to exchange changes.
If the PC is asleep or Syrinx's sync component is not running there, phone
edits remain queued. An optional always-on user-owned device could hold
changes between visits; that requires a Syrinx sync component, not just a
Tailscale relay. It is a later possibility, not a required purchase or an
approved deployment target.

Sync on opening and resuming Syrinx should be the reliable baseline, with
background work when the operating system permits. A connected VPN does
not keep Syrinx running continuously. Apple controls background execution,
and Android Doze can defer network and sync activity. Do not promise instant
sync while the app is closed. See [Apple background strategies](https://developer.apple.com/documentation/backgroundtasks/choosing-background-strategies-for-your-app)
and [Android Doze and App Standby](https://developer.android.com/training/monitoring-device-state/doze-standby).

Other active mobile VPNs can conflict with Tailscale. Explain this during
setup and offer a clear recovery route; never silently replace the user's
VPN configuration. See [Tailscale VPN compatibility](https://tailscale.com/docs/reference/faq/other-vpns).
Existing network policies may also require administrator approval for peers
to communicate. The walkthrough should diagnose that condition rather than
automatically broadening network access.

## Optional cloud alternative

Later, offer a cloud service as another delivery route for the same sync
engine. An end-to-end encrypted mailbox could retain changes until each
device retrieves them, allowing devices to sync without being online
together. This is proposed Syrinx architecture, not a Tailscale feature.

The cloud option should remain optional, while Tailscale and Headscale
remain usable independently. Switching routes should preserve identities
and sync progress. Cloud design must cover encryption-key recovery, device
revocation, retention, deletion, service metadata and operating costs.
Encryption of content does not promise that the service sees no metadata.
A cloud service also does not remove mobile background-execution limits.

## Outline of later work

1. **Prove mobile feasibility and onboarding.** Choose mobile platform
   order with the owner, prototype the walkthrough on real supported
   devices, and verify Tailscale and Headscale connectivity, permissions,
   VPN conflicts, battery impact and background limits.
2. **Design and test the common sync engine.** Agree data scope, identity,
   encryption, recovery, deletion and conflict rules. Exercise interrupted
   transfers, repeated messages, offline edits and stale devices using only
   synthetic data before connecting any personal data.
3. **Deliver private device sync.** Complete pairing, user controls, initial
   sync, incremental updates, diagnostics and the guided setup. Validate
   new Tailscale users, existing accounts and existing Headscale networks.
4. **Consider availability extensions.** Evaluate a user-owned always-on
   sync device if users need it. Plan optional encrypted cloud sync later,
   reusing the engine and keeping private device sync available.

These are future planning stages, not additions to the current issue map.
The first private-sync delivery is ready only when the walkthrough works
without command-line setup for an ordinary Tailscale user, existing
Headscale users can connect with guided client configuration, queued
changes survive restart, and conflict/deletion tests show no silent loss
or resurrection. Pairing rejection and revocation must also be tested.

## Open threads before implementation

- The owner must choose Android/iOS delivery order and approve a mobile
  feasibility plan; neither platform has been selected by this outline.
- Confirm the initial data categories, defaults and whether remote tabs
  are a later category distinct from desktop session restoration.
- Specify conflict behavior, deletion retention, key recovery and what
  revocation can prevent after data has already reached another device.
- Verify supported client APIs, versions and mobile setup behavior again
  before building; the linked documentation was consulted on 2026-10-10.
- Decide whether a desktop background component or an always-on companion
  is needed, and review its installation and lifecycle explicitly.
- Design the cloud service, recovery experience, metadata disclosure and
  commercial model only when that later option is scheduled.
