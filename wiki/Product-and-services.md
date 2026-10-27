# Product and services

Syrinx is free and open source. To pay for its development without making
ordinary browsing depend on a subscription, the owner plans optional paid
services beside the free browser. This page is the short version; the
records are [`docs/product-map.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/docs/product-map.md)
(the owner's "Version A2" map of 2026-10-26 and every conflict it settled),
the [paid-services plan](https://github.com/Fledge-Software/Syrinx-app/blob/main/docs/paid-services-plan.md)
and the [included-features plan](https://github.com/Fledge-Software/Syrinx-app/blob/main/docs/browser-protections-plan.md).
Where they disagree, [`docs/decisions.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/docs/decisions.md)
wins.

None of the paid services exists yet, and none jumps the work order: the
browser core, the plugin runtime and Notes come first.

## Free

| Feature | What it is | Tracked in |
|---|---|---|
| The browser | Tabs, saved pages, history, the palette and local search. | [#50](https://github.com/Fledge-Software/Syrinx-app/issues/50) |
| Plugins | Sandboxed WebAssembly add-ons, personal or, later, published. | [#25](https://github.com/Fledge-Software/Syrinx-app/issues/25), [#183](https://github.com/Fledge-Software/Syrinx-app/issues/183) |
| Customize | The theme editor today; later functions and workflows. | [#182](https://github.com/Fledge-Software/Syrinx-app/issues/182) |
| Ad and tracker blocker | Local request blocking, after v1, leaving Google's pages untouched. | [#179](https://github.com/Fledge-Software/Syrinx-app/issues/179) |

## Paid (planned)

| Service | Price | Tracked in |
|---|---|---|
| Sync | $4/month or $40/year, 10 GB, end-to-end encrypted | [#176](https://github.com/Fledge-Software/Syrinx-app/issues/176) |
| Extra storage | $2/month per 30 GB | [#174](https://github.com/Fledge-Software/Syrinx-app/issues/174) (SVC-08) |
| Household Sync | $20/month or $200/year, six people, 100 GB pooled | [#176](https://github.com/Fledge-Software/Syrinx-app/issues/176) |
| AI | $20/month plus one-off credit packs; your own API key is free | [#177](https://github.com/Fledge-Software/Syrinx-app/issues/177), [#180](https://github.com/Fledge-Software/Syrinx-app/issues/180) |
| VPN | $8/month, Guardian, device-wide | [#178](https://github.com/Fledge-Software/Syrinx-app/issues/178) |
| Support Syrinx | $5/month or $50/year, voluntary, unlocks nothing | the website's Donate page |

A "Browser Package" bundle ($30/month or $246/year) is an idea the owner
has not decided ([#181](https://github.com/Fledge-Software/Syrinx-app/issues/181)).

## One account

One Syrinx account signs in on syrinxapp.dev, where you manage it, and in the
app, where it unlocks paid features
([#175](https://github.com/Fledge-Software/Syrinx-app/issues/175)). It runs on
Supabase Auth in a dedicated Syrinx project. An account is optional: free
browsing never needs one. Syrinx collects no browsing data or content; an
account holds only what sign-in and billing need.

## Directions

- **Syrinx Distros**: forks built on Syrinx, perhaps shipped as a
  preconfigured plugin-pack, that can still use plugins and the
  subscriptions ([#184](https://github.com/Fledge-Software/Syrinx-app/issues/184)).
- **A Fledge companion app** on the desktop or a phone, after v1
  ([#185](https://github.com/Fledge-Software/Syrinx-app/issues/185); see also
  [Mobile sync](Ideas-Mobile-sync)).
