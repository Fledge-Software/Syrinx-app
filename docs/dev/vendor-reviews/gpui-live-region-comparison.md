# Announcing Syrinx's status lines: patch GPUI, move GPUI, or neither (issue #101)

Status: proposal for the owner, 2026-10-24. Nothing in `vendor/` or
`crates/` has changed. A vendor patch needs the owner's explicit approval
under "Dependencies and licence" in `AGENTS.md`; this document asks for a
decision and grants nothing.

## Why this was looked at

A *live region* is a part of the interface whose text a screen reader reads
out when it changes, without the user moving to it. Syrinx has two lines
that should behave that way: the footer status line at the bottom of the
main window (`crates/syrinx-app/src/ui.rs`, lines 2149–2160, element id
`status`) and the "Restoring the backup…" line that the recovery screen
shows once the owner confirms a restore (`crates/syrinx-app/src/ui/recovery.rs`,
lines 84–91, element id `restore-progress`, from #51). Both already reach
Windows UI Automation as **Status** nodes named after their text, so NVDA can
find them. Neither is marked live, so NVDA says nothing when the text
changes: the owner confirms a restore and hears silence.

Issue #101 records that the pinned GPUI snapshot (`gpui-pre 0.3.8`, Zed
revision `279fe070…`) has no setter for AccessKit's live property, and asks
for a written comparison of two ways out before any code: (a) a small,
owner-approved patch to the vendored GPUI under D12 and D37's rules, or (b)
moving to a GPUI snapshot that already has the setter. The triage expected
the patch to win. The brief was to find out, not to confirm, and the source
says something else: **neither is needed.** GPUI 0.3.8 already exposes the
node that Syrinx needs to mark, through a public API meant for a neighbouring
purpose. The rest of this document shows the evidence for all three routes,
what each would cost, how the result would be proved, and what the owner is
asked to decide.

## Short answer

- **Option (a), the vendor patch,** would be about twenty lines in one file,
  but that file is in GPUI's *core* crate (`gpui-pre`), which Syrinx does not
  vendor. The vendored package is only the Windows backend
  (`vendor/gpui-pre-windows-0.3.8`). The patch therefore means vendoring a
  second GPUI package of 161 files and 8.3 MB, with a provenance record, a
  new `[patch.crates-io]` override, and a new `AGENTS.md` exception for the
  upstream deferred-work comments it carries (fifteen upper-case
  `TODO`/`FIXME`/`HACK` words; twenty-eight whole words of any case, once
  the lower-case `todo` notes are counted).
- **Option (b), the move,** has no target. Upstream Zed's `main` as of today
  (`af4948a1`, 2026-10-24) still has no live setter, and neither does the
  newest published snapshot, `gpui-pre 0.3.9`. A move would re-vendor the
  Windows backend, rebase D37's idle patch and the shader patch onto files
  upstream changed, move `gpui-component` to a release that brings two new
  packages to audit, and recheck every trap pinned to 0.3.8 line numbers; and
  at the end Syrinx would still need (a) or (c).
- **Option (c), no GPUI change,** uses `A11ySubtreeBuilder::parent_node()`,
  a public method of the pinned crate that hands back the element's own
  AccessKit node while it is being built. Calling `set_live` on it is a few
  lines of Syrinx code, and the pinned Windows adapter turns that property
  into exactly the UI Automation event NVDA listens for. This is the
  recommendation, with the reservation that it is a side use of a method
  documented for synthetic children.

## The problem in source

### How an announcement reaches NVDA on Windows

GPUI builds an [AccessKit](https://accesskit.dev/) tree of its elements each
frame while an accessibility client is attached, and AccessKit's Windows
adapter presents that tree to UI Automation, the Windows accessibility API
(`docs/p1-36-accessibility.md` at af3de53, "What GPUI exposes to Windows UI
Automation"). For a status line to be announced, three things must hold, and
all three are already in the pinned dependencies:

1. **AccessKit has the property.** `accesskit 0.24.1` defines
   `Live { Off, Polite, Assertive }` (`src/lib.rs`, lines 584–588) and gives
   every `Node` the methods `live() -> Option<Live>`, `set_live(Live)` and
   `clear_live()` (the `unique_enum_property_methods!` entry at line 2139,
   expanded at lines 1687–1705). An unset property reads as `None`, which the
   adapter treats as off.
2. **The Windows adapter maps it and raises the event.**
   `accesskit_windows 0.34.0` reports the property as UI Automation's
   `LiveSetting` (`src/node.rs`, lines 546–551 and the property table at
   line 1318). It queues a `LiveRegionChanged` event when a live node with a
   name appears in the tree (`src/adapter.rs`, lines 257–264) and when a live
   node's name or liveness changes (lines 315–325). The name is the node's
   label (`src/node.rs`, lines 404–410), which Syrinx sets with `aria_label`.
   The numeric ids, from the pinned `windows 0.62.2`, are
   `UIA_LiveSettingPropertyId = 30135` and
   `UIA_LiveRegionChangedEventId = 20024`; `Role::Status` is presented as the
   `StatusBar` control type, 50017 (`accesskit_windows/src/node.rs`, line
   194).
3. **NVDA speaks on that event.** NVDA maps event 20024 to its
   `liveRegionChange` event (`source/UIAHandler/__init__.py`, line 257, read
   from the NVDA repository today) and its base handler speaks the node's
   name, ahead of queued speech for `Assertive` (NVDA's `NEXT` priority) and
   at normal priority otherwise (`source/NVDAObjects/__init__.py`, lines
   1238–1252). Its UI Automation layer drops a repeat of the same text
   within half a second (`source/NVDAObjects/UIA/__init__.py`, lines
   1663–1673), which suits a footer that may set the same message twice.

So the only missing link is the first step on GPUI's side: nothing in Syrinx's
code path calls `set_live` on the status nodes.

### What GPUI 0.3.8 has and lacks

The element-to-node conversion lives in `gpui-pre 0.3.8` (the core crate,
read from Cargo's registry copy; its manifest names Zed revision
`279fe070bb389b79652e52065b2f001edcc0b11b`, `Cargo.toml`, lines 47–50):

- `AriaProperties` (`src/elements/div.rs`, lines 2106–2128) holds the
  properties an element can set: label, description, keyboard shortcut,
  selected, expanded, toggled, numeric values, value, placeholder,
  orientation, level, set positions and table indices. **No `live` field.**
- The builder methods on `StatefulInteractiveElement` (`div.rs`, lines
  1306–1715: `role`, `accessibility_id`, `aria_label`, `aria_description`,
  `aria_keyshortcuts`, `aria_active_descendant`, `a11y_synthetic_children`,
  `aria_selected` and the rest) have **no `aria_live`.**
- `Interactivity::write_a11y_info` (`div.rs`, lines 3530–3603) copies each
  set property onto the `accesskit::Node`; **no `set_live` call.**
- The accessibility guide (`src/_accessibility.rs`) does not mention live
  regions.

Those three facts are what #101 summarised as "no live-region setter", and
they are correct. What they leave out is the fourth fact:

- `A11ySubtreeBuilder::parent_node(&mut self) -> &mut accesskit::Node` is
  **public** (`src/window/a11y.rs`, lines 363–368), and so is
  `StatefulInteractiveElement::a11y_synthetic_children`, which takes a
  `'static` closure receiving that builder (`div.rs`, lines 1376–1391). The
  builder is re-exported as `gpui::A11ySubtreeBuilder` (`src/window.rs`,
  line 69) and the whole `accesskit` crate as `gpui::accesskit`
  (`src/gpui.rs`, line 97), so `gpui::accesskit::Live` is nameable without
  a new dependency.

The order of operations in `src/element.rs` (lines 368–439) makes the fourth
fact usable. During an element's prepaint, if a client is attached and the
element has an id and a role, GPUI creates the node, writes the aria
properties into it and pushes it onto the builder's stack (lines 370–382).
After the element's own prepaint it constructs an `A11ySubtreeBuilder` for
that node and calls `a11y_synthetic_children` (lines 428–437); `parent_node`
returns the node on top of the stack (`a11y.rs`, lines 459–461). Only then is
the node popped and finalised into the frame's node list (line 439;
`a11y.rs`, lines 465–471). Anything set on the node inside the closure is
therefore what the Windows adapter receives.

## Option (a): a vendor patch to GPUI

### Where the change goes, and why that is the real cost

The issue's wording, "a small, owner-approved vendor patch to GPUI", reads
naturally as a change to the package Syrinx already vendors. That package is
`gpui-pre-windows`, the Windows backend: window creation, Direct3D, the
AccessKit adapter hookup and the message loop (`vendor/gpui-pre-windows-0.3.8/src/`,
24 source files). The accessibility *properties* are written in the core
crate, `gpui-pre`, which Syrinx takes from the registry unmodified. A patch
exposing `live` cannot be made in the vendored backend, because the backend
only forwards the finished `TreeUpdate` to the adapter
(`vendor/gpui-pre-windows-0.3.8/src/window.rs`, lines 1110–1124).

Vendoring `gpui-pre 0.3.8` would follow the shape of the existing GPUI
vendor (`vendor/gpui-pre-windows-0.3.8.provenance.md`): a byte-verified
copy of the published archive, a unified diff with hashes, an
`[patch.crates-io]` entry in `Cargo.toml`, a `.gitattributes` line, and an
`AGENTS.md` approval naming the directory. What is different is the size:

| | `gpui-pre-windows 0.3.8` (vendored today) | `gpui-pre 0.3.8` (option a) |
| --- | ---: | ---: |
| Files in the package | 29 | 161 |
| Bytes | 658,607 | 8,327,520 |
| Rust source files, lines | 21 files, 13,181 lines | 93 files, 86,912 lines |
| Upper-case `TODO`/`FIXME`/`HACK` markers in `src/` (any case in brackets) | 2 (7) | 15 (28) |

Both marker rows count whole words (`grep -w`), upper-case first and any
case in brackets, so a line that holds two markers counts twice.

The gate's marker scan reads only `crates/` (`crates/xtask/src/deferred_work.rs`,
line 24), so the upstream comments would not fail it, but
`AGENTS.md` permits unchanged upstream markers per vendored directory, so
the approval would have to name `vendor/gpui-pre-0.3.8` as the earlier ones
name the backend. Every later GPUI move would then carry two vendored
packages instead of one.

### The exact diff shape

All in `src/elements/div.rs` of `gpui-pre 0.3.8`; about twenty added lines,
no removed lines, no manifest change, no new dependency.

1. **The property** (`AriaProperties`, after line 2109):

   ```rust
   pub(crate) live: Option<accesskit::Live>,
   ```

2. **The builder method** (`StatefulInteractiveElement`, after
   `aria_description` at line 1344), named like the web attribute as the
   others are:

   ```rust
   /// Mark this element as a live region, so assistive technology
   /// announces changes to its name without the user moving to it.
   fn aria_live(mut self, live: accesskit::Live) -> Self {
       self.interactivity().aria.live = Some(live);
       self
   }
   ```

3. **The write** (`write_a11y_info`, after the `description` block at line
   3539):

   ```rust
   if let Some(live) = self.aria.live {
       node.set_live(live);
   }
   ```

4. **A test** beside `test_write_a11y_info_string_and_numeric_properties`
   (line 5266): build an `Interactivity` with `aria.live = Some(Live::Polite)`,
   write it into `accesskit::Node::new(Role::Status)` and assert
   `node.live() == Some(Live::Polite)`.

Optionally, one line in `src/gpui.rs` (line 99) re-exporting `Live` next to
`Orientation, Role, Toggled`, so callers write `gpui::Live`.

Syrinx would then call `.aria_live(gpui::accesskit::Live::Polite)` on the
two status elements, a one-line change in each of `ui.rs` and `recovery.rs`.

### Upstream fate

D12 requires a carried patch to be offered upstream and dropped once merged.
Upstream history is instructive here. The first GPUI accessibility proposal,
Zed PR #54398 ("gpui: Add accessibility annotation API via AccessKit",
closed unmerged on 2026-04-21), included `aria_live()` among twelve builder
methods. The design that did merge, PR #56065 ("gpui: Accesskit support",
2026-05-27), is the one in 0.3.8 and left `live` out. Nine merged pull
requests in the repository mention AccessKit, seven of them after that
design landed; the closest in shape to this patch is PR #61926 ("gpui:
Expose accessibility identifiers to platform clients", 2026-08-07), which
added one property (`accessibility_id`) to
`AriaProperties` and `write_a11y_info` in exactly the way sketched above. So
an upstream `aria_live` pull request is plausible and has a precedent, but
its timing is not Syrinx's to control, and Syrinx would only stop carrying
the local copy after a pin move, which is option (b)'s cost.

### Risk and cost

- **Behavioural risk: none beyond option (c)'s.** The change adds a
  property and no node, so the three debug-assertion panics in the tree
  builder (`a11y.rs`, lines 232–243, 411–423, 530–542; see the P1-36 trap)
  are not reachable from it.
- **Maintenance cost: high relative to the change.** An 8.3 MB vendored
  tree, a 161-row provenance table, and a second package to rebase at every
  GPUI move, for twenty lines whose effect Syrinx can get another way.
- **Approval cost:** a new `AGENTS.md` exception and the owner's explicit
  approval, as for every vendor patch.

## Option (b): move to a GPUI snapshot that has the setter

### There is no such snapshot

- Zed's `main` at `af4948a164b957959738f71a6ff82fe325b18ba6` (committed
  2026-10-24 17:19 UTC, read through the GitHub API) has the same
  `AriaProperties` list as 0.3.8 (`crates/gpui/src/elements/div.rs`, lines
  2156–2178) and no `aria_live` builder; `crates/gpui/src/_accessibility.rs`
  does not mention live regions; `A11ySubtreeBuilder::parent_node` is still
  there (`crates/gpui/src/window/a11y.rs`, line 364). GitHub's code search
  for `aria_live` and `set_live` in the repository returns nothing.
- The published `gpui-pre` snapshots are 0.3.0 through 0.3.9, weekly since
  2026-09-03 (crates.io). 0.3.8 was published 2026-10-05 from Zed
  `279fe070` (2026-10-04); 0.3.9 on 2026-10-12 from Zed `93d74566`
  (2026-10-10). Since even today's `main` lacks the setter, no published
  snapshot can have it.

So option (b) cannot close #101 on its own. If it is ever done, it is for
other reasons, and Syrinx still applies (a) or (c) afterwards.

### What a move would involve anyway

Recorded so the owner can weigh a move on its own merits later:

- **Pins.** `gpui-pre`, `gpui-pre-platform` and the vendored
  `gpui-pre-windows` are `=0.3.8` (`Cargo.toml`, lines 26–27 and 43); the
  platform crate pins its backends to the same version, so all move
  together, with `gpui-pre-macros`, `gpui-pre-sum-tree` and the other
  `gpui-pre-*` packages in the lockfile.
- **gpui-component.** `gpui-component 0.7.1` and `gpui-base 0.7.1` pin
  `gpui-pre =0.3.8`, as does 0.7.2. The first releases built on 0.3.9 are
  0.7.3 and 0.8.0 (registry index, read locally), and both add new packages,
  `gpui-fast` and `gpui-fast-platform`, which would need a licence and
  maintenance audit in `docs/dev/dependency-audit.md` before they enter the
  lockfile.
- **Re-vendoring and rebasing.** The idle-repaint patch (D37) touches
  `events.rs`, `platform.rs`, `window.rs`, `direct_manipulation.rs` and
  `gpui_windows.rs` and adds `frame_demand.rs`; the embedded-shaders patch
  touches `build.rs` and `directx_renderer.rs`. Between the two snapshots
  upstream changed `crates/gpui_windows/Cargo.toml`, `build.rs`,
  `src/display.rs`, `src/events.rs`, `src/platform.rs` and `src/window.rs`
  (GitHub compare `279fe070...93d7456`: 128 commits; the file list is capped
  at 300 entries and this comparison hit the cap, so it is a lower bound).
  The manifest change means the backend's own pins and features would be
  re-checked, not only its sources. Both patches would need rebasing, the
  idle measurement rerun against D37's 0.5 % bar,
  and a release zip tested on a machine other than the builder for the
  shader embedding.
- **Traps pinned to 0.3.8 line numbers** (`docs/dev/trapstoknow.md`, "The
  interface: GPUI on Windows"): `activate_window`'s deferred Win32 work,
  `on_resize`/`on_request_frame` under `try_borrow_mut`, the
  `HitboxId::is_hovered` rule and the bounds observer (all in
  `gpui-pre/src/window.rs`, which changed upstream in the window),
  `open_window` drawing inside the caller's lease (`app.rs`, also changed),
  `tab_index` on tracked handles (`elements/div.rs`), and gpui-base's
  `input-state` focus handling that `text_field.rs` works around. Each would
  be reread at the new lines and its probe rerun. `window/a11y.rs` itself
  last changed on 2026-09-23 and is identical between the two snapshots.

That is a slice of its own, with its own issue, and it does not announce
anything.

## Option (c): the public API the pinned GPUI already has

### The sketch

Two small functions in `crates/syrinx-app`, used by both status elements.
The first takes the bare node so a unit test can drive it; the second is
the one-line adapter GPUI's hook calls:

```rust
/// Marks a node as a polite live region, so a screen reader reads its
/// text when it changes without the owner moving to it.
fn mark_live(node: &mut gpui::accesskit::Node) {
    node.set_live(gpui::accesskit::Live::Polite);
}

/// GPUI 0.3.8 has no `aria_live`; `parent_node` is the element's own node
/// while it is being built, so the synthetic-children hook marks that.
fn announce_changes(builder: &mut gpui::A11ySubtreeBuilder) {
    mark_live(builder.parent_node());
}
```

and at the two call sites:

```rust
div()
    .id("status")
    .role(gpui::Role::Status)
    .aria_label(text.clone())
    .a11y_synthetic_children(announce_changes)
    // …
```

The closure runs only when an accessibility client is attached and the
element has an id and a role (`div.rs`, lines 1380–1382), which both status
elements already satisfy. It adds no child node. The comment at
`recovery.rs`, line 84, "GPUI exposes no live setting to raise", would be
replaced, since it is the claim this document corrects.

`Polite` is proposed for both lines. The footer carries navigation and
error notices ("Saving and closing…", "This popup could not be opened.",
"That tab is no longer open.", the lines assigned to `self.message` in
`ui.rs`), none of which should interrupt what NVDA is already saying; the
restore line appears after the owner's own confirmation, so it needs no
interruption either. `Assertive` maps to NVDA's `NEXT` priority, which
jumps the speech queue, and would suit the permission bar's `Role::Alert`
(`ui.rs`, line 2133), which is outside #101 and could reuse the same helper
later.

### Why it is sound, and where it is a side use

The chain is read end to end in pinned source: `parent_node` returns the
node GPUI pushed for this element (`element.rs`, lines 370–382 and 428–437;
`a11y.rs`, lines 459–461); the node is finalised after the closure (line
439; `a11y.rs`, lines 465–471); the adapter raises `LiveRegionChanged` on
appearance and on name change (`accesskit_windows/src/adapter.rs`, lines
257–264 and 315–325); NVDA speaks the name. No step is inferred.

One boundary for the implementation slice: the consumer library the adapter
reads the tree through, `accesskit_consumer 0.38.0`, resolves `live()` by
falling back to the parent's value when a node sets none (`src/node.rs`,
lines 889–893), so marking a status node live makes every node under it
live as well. Neither status element contributes child nodes today, so
nothing else changes; a future child, such as a synthetic text run, would
be announced too.

The reservation: `parent_node` is documented only as "A mutable reference to
the parent node", inside a builder whose documented purpose is adding
synthetic children (`_accessibility.rs`, "Synthetic children"). Using it to
set a property on the element's own node is within its contract but not its
stated purpose. Upstream has not changed the method since the snapshot and
it is present on `main` today, but it could be narrowed in a future
snapshot. If that happens at a pin move, the fallback is option (a) at that
time, with the same twenty lines. This is the same kind of choice the owner
made for DW-62 (`docs/decisions.md`, 2026-10-14): Syrinx's own code rather
than a further change to the pinned toolkit, and DW-60 and DW-70 took the
same route for focus reporting.

### Risk and cost

- **Code:** one helper and two call sites in Syrinx, no manifest change, no
  new dependency, no vendor change.
- **Approval:** none under the vendor rules. The owner's go-ahead is asked
  for all the same, because the P1-36 record and #101 both state that GPUI
  cannot do this, and this document reverses that.
- **Runtime:** the tree's shape does not change, so the debug-assertion
  panics do not apply. The UI Automation probe is cheap and is rerun anyway
  (next section).

## How the announcement would be proved

Synthetic status text only, on fresh `--test-data-dir` profiles deleted
afterwards; no capture that shows a tab title or address (the AN-02 trap).

1. **Unit test, in the gate.** `mark_live` takes `&mut accesskit::Node`,
   so a test applies it to `accesskit::Node::new(Role::Status)` and asserts
   `live() == Some(Live::Polite)`; `announce_changes` is the one-line
   adapter over it, since the builder's constructor is crate-private and
   cannot be driven from a test.
2. **UI Automation probe, by an agent.** As in P1-36 (`docs/p1-36-accessibility.md`
   at af3de53, "The probe"): attach a client to the `local` build and read
   the footer's node. The .NET `System.Windows.Automation` wrapper P1-36 used
   predates `LiveSetting`, so the probe reads property 30135 through the COM
   `IUIAutomation` client (`CUIAutomation8`) instead, expecting `1`
   (`Polite`), and records event 20024 while a synthetic change happens:
   opening `https://example.com/` changes the footer; a synthetic damaged
   profile from #51's fixtures reaches the recovery screen, where confirming
   makes the restore line appear. The Windows SDK already on this machine
   (it supplies `fxc.exe` for the shader patch) also has
   `C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\inspect.exe`
   and `accevent.exe`, which show `LiveSetting` and log `LiveRegionChanged`
   without any new install; driving them is a window check, not a test.
   The probe also confirms Syrinx survives with the tree active, per the
   P1-36 trap.
3. **NVDA, by the owner.** A fourteenth owner check in the style of P1-36's
   list: with NVDA running on a fresh profile, open a page and hear the
   footer's new text without moving to it; on the recovery screen, confirm
   the restore and hear "Restoring the backup…". Record pass or fail only.
   An agent cannot judge speech, and NVDA's own filters (the half-second
   repeat rule above) are part of what is being judged.

## Recommendation, and what was rejected

**Recommend option (c).** Its two strongest reasons: it needs no change to
any vendored or pinned package, so there is nothing to carry, rebase or
offer upstream; and every link from Syrinx's call to NVDA's speech is in
pinned source that can be cited by line, so the remaining uncertainty is the
owner's NVDA check, which every option needs.

**Rejected: option (a)** as the first step. It is not wrong, and the twenty
lines are the fallback if `parent_node` ever goes away, but it buys nothing
option (c) does not, at the price of vendoring GPUI's 8.3 MB core crate and
a second package at every move. If the owner prefers a named `aria_live`
over a side use of `parent_node`, (a) is the way to get one, and the diff
above is ready to apply under a separate approval.

**Rejected: option (b)** for this issue, because no GPUI snapshot has the
setter, including upstream `main` today. A pin move stays a separate
decision with its own issue; this document lists what it would carry.

## Owner decides

`AGENTS.md` reserves vendor patches and dependency moves to the owner. Three
answers are possible on #101:

- **Approve (c).** The implementation slice adds the helper and the two
  call sites, the unit test and the probe record, and adds the NVDA check to
  `docs/owner-checklist.md`. No vendor approval is involved; the owner's
  yes covers the side use of `parent_node` described above.
- **Approve (a).** A separate, explicit approval to vendor `gpui-pre 0.3.8`
  with the diff above, an `AGENTS.md` exception for that directory's
  upstream markers, and a provenance record, before the same
  implementation slice.
- **Neither yet.** #101 stays open; the status lines stay silent.

Option (b) is not offered as an answer to #101. If the owner wants the GPUI
move, it gets its own issue.

## Sources read

| Source | What it established |
| --- | --- |
| `crates/syrinx-app/src/ui.rs`, lines 2149–2160; `crates/syrinx-app/src/ui/recovery.rs`, lines 84–91 | The two status elements: id, `Role::Status`, `aria_label` |
| `gpui-pre 0.3.8` (registry): `src/elements/div.rs`, lines 1306–1715, 2106–2128, 3530–3603; `src/element.rs`, lines 368–439; `src/window/a11y.rs`, lines 304–368, 447–471; `src/window.rs`, line 69; `src/gpui.rs`, lines 97–99; `src/_accessibility.rs`; `Cargo.toml`, lines 47–50 | No `aria_live`; public `parent_node`; order of node build, synthetic children and finalisation; re-exports; snapshot revision |
| `vendor/gpui-pre-windows-0.3.8/src/window.rs`, lines 1094–1124; `vendor/gpui-pre-windows-0.3.8.provenance.md` | The backend only forwards tree updates; what is vendored today and how |
| `accesskit 0.24.1`: `src/lib.rs`, lines 584–588, 1687–1705, 2139 | `Live` and `set_live` |
| `accesskit_consumer 0.38.0`: `src/node.rs`, lines 889–893 | `live()` falls back to the parent's value when a node sets none |
| `accesskit_windows 0.34.0`: `src/adapter.rs`, lines 245–270, 305–330; `src/node.rs`, lines 194, 365, 404–410, 546–551, 1318 | `LiveSetting` mapping, `LiveRegionChanged` on appearance and name change, name from label, `StatusBar` control type |
| `windows 0.62.2`: `src/Windows/Win32/UI/Accessibility/mod.rs`, lines 20490–20491, 20592 | Property and event ids 30135, 20024, 50017 |
| NVDA repository (`nvaccess/nvda`, `main`, read today): `source/UIAHandler/__init__.py`, line 257; `source/NVDAObjects/__init__.py`, lines 1238–1252; `source/NVDAObjects/UIA/__init__.py`, lines 1663–1673 | Event mapping, speech, repeat filter |
| Zed repository (`zed-industries/zed`, read through the GitHub API): `main` at `af4948a1`; PRs #54398, #56065, #61926; compare `279fe070...93d7456`; commit history of `crates/gpui/src/window/a11y.rs` | No upstream setter; the closed proposal that had one; the one-property precedent; what changed between snapshots |
| crates.io (`gpui-pre`); local registry index (`gpui-component`, `gpui-base`, `gpui-pre-platform`) | Published snapshots and dates; which gpui-component releases pin which GPUI, and their new packages |
| `docs/architecture-decisions.md`, D12 and D37; `docs/decisions.md`, 2026-10-14 (DW-62); `docs/dev/vendor-reviews/gpui-idle-patch-review.md`; `docs/p1-36-accessibility.md` at af3de53; `docs/dev/trapstoknow.md`; `crates/xtask/src/deferred_work.rs`, line 24 | The rules, the precedents, the probe, the traps, the marker scan's scope |

Nothing was built or run for this document; every claim is from source read
on this machine or through the GitHub and crates.io APIs on 2026-10-24.
