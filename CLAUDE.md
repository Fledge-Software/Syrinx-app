# Atlas

Atlas (working name) is a browser-first workspace app: a Tauri 2 desktop
browser with a vertical-tab sidebar, where Forge (code), Notes and Fledge run
as built-in plugins that open as tabs.

- The product spec, the decisions already made and the phased plan are in
  `docs/spec.md`, loaded below. Its "Decisions already made" are not to be
  relitigated.
- The design target and the decisions made since the spec are in
  `design/README.md`. The live canvas is the source of truth for the design;
  `design/canvas/` is a snapshot of it.

## Working agreements

- This repository is local only for now: no git remote and no pushes.
- Commit only when the owner asks.
- Work the phases in order. Phase 0, the feasibility spike, comes first; stop
  after it and report what worked, what didn't and what you recommend.
- Atlas is a separate project from Fledge-App. Fledge reaches Atlas only as a
  plugin talking to Fledge's API; don't put Atlas code or docs in the
  Fledge-App repository.

@docs/spec.md
