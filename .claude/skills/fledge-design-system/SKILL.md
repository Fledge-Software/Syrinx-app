---
name: fledge-design-system
description: Use when building or changing any Fledge UI — a page, a component, a screen, a re-skin, or a throwaway mock. Contains the brand rules, the token map, the app-shell contracts, and the traps that cost real time. Adapted from the claude.ai/design "Fledge Design System" project for this repo.
user-invocable: true
---

# Building Fledge UI

**Source of truth for appearance:** the claude.ai/design project *Fledge Design
System*, id `6c0bed5c-b292-4969-a961-535f3e5d5b47` (read it with the `DesignSync`
tool: `list_files`, `get_file`).
**Source of truth for behavior:** this repo.
**When they disagree:** the repo wins for behavior, the design system wins for
appearance — and the differences that survived adoption are already catalogued in
[`DOCS/DESIGN-SYSTEM-GAPS.md`](../../../DOCS/DESIGN-SYSTEM-GAPS.md). Read that
before "fixing" a mismatch; several are deliberate.

## Read first

| Need | File |
| --- | --- |
| Brand rules, color pairing, voice, theming, logo | `DOCS/BRAND.md` |
| Known differences vs the design system | `DOCS/DESIGN-SYSTEM-GAPS.md` |
| How the shell was adopted, and every decision behind it | `DOCS/DESIGN-SYSTEM-ADOPTION.md` |
| Token values (4 theme scopes, a11y, z-ladder) | `packages/ui/src/brand.css` |
| Tailwind token map | `packages/ui/src/tailwind-preset.ts` |
| What exists — one export per component | `packages/ui/package.json` |

## Before you write a component

1. **Check `packages/ui` first.** There is probably already a `Button`, `Card`,
   `Badge`, `Input`, `Select`, `Textarea`, `Modal`, `SlideOver`, `Icon`,
   the layout-B `shell/*` chrome, `ThemeToggle`, `Logo`, `TrialBanner`,
   `BulkActionBar`, or a `chat/*` primitive that does it.
2. **Never import from the design system.** Its components are JSX with inline
   `style={{…}}` — that's its source format, not the target. **Port**: Tailwind
   classes, `forwardRef`, `cn()`. A port that ships a `style={{…}}` or a
   `<style>` block has failed.
3. **No new design-system dependencies.** No `tailwind-merge`,
   `class-variance-authority`, `clsx`, Radix, Headless UI. `Record<Variant,
   string>` maps work fine.

## Hard rules

- **Tokens only.** No raw hex. `text-dark` / `text-muted` / `bg-surface-raised` /
  `border-line`, never `#1A1A1A` or `text-gray-500`.
- **Status tokens have three tones**: `DEFAULT` (fill), `foreground` (text on the
  fill), `text` (text on the page surface). Error copy is `text-danger-text`, not
  `text-danger`.
- **Amber is never a text color.** Text on amber is near-black, never white.
- **`bg-primary` carries `text-primary-foreground`**, never a hardcoded
  `text-white` — dark mode's primary is `#52B788` with near-black foreground.
- **In-app surfaces stay flat.** Shadow + hairline is the only elevation. Never a
  gradient in-app.
- **One z-index ladder**: `z-chrome` 30 · `z-dropdown` 40 · `z-drawer` 50 ·
  `z-modal` 60 · `z-toast` 70. Never an ad-hoc `z-[60]`.
- **The colored left border belongs to `OpportunityCard` alone.** The rail's
  active row is a filled pill precisely so it doesn't compete.
- **Count badges are amber with near-black text**, only for things that need a
  person.
- **`cn()` has no `tailwind-merge`.** A caller's `className` is appended, not
  merged, so `w-full` and `w-[17rem]` fight on stylesheet order. If a component
  needs override semantics, give it an explicit prop.
- **Don't touch the preferences pipeline.** `preferences.ts`,
  `PreferencesScript`, `PreferencesSeed`, the cookie, the server action.

## The app shell

Every logged-in screen mounts layout B once from its zone's root layout, through
one implementation: `loadZoneShell()` (`@fledge/ui/shell/server`) resolves the
data on the server and `ZoneChrome` (`@fledge/series/ZoneChrome`) draws
`ShellFrame` from it with SERIES wired (the shell's is
`apps/app/app/dashboard/layout.tsx`; `packages/ui/src/shell/README.md` is the
recipe for a module). The data: the modules from `public.my_modules()`, the
person's favourites, the business menu's links and meters. The module ribbon is the
catalogue's, never a hard-coded nav; a module adds its own screens to the
feature ribbon through its catalogue row. `AppShell` and its rail were removed
in restructure step RST; `main` still mounts them until Stage C.

**One scroll mode.** The workbench owns the viewport and the content column is
the scroll container, so a page that pins something to its bottom gets the
`h-full min-h-0` chain for free. The content is flush: each page (or the zone's
content wrapper) sets its own padding and max-width.

## Traps

1. **The Browser pane must be OPEN to verify anything visually.** Closed →
   `document.visibilityState === "hidden"` → `requestAnimationFrame` never fires
   → React never hydrates. Symptoms: spinners forever, `getBoundingClientRect()`
   all zero, `__REACT_DEVTOOLS_GLOBAL_HOOK__.renderers.size === 0`. Not an app
   bug. `getComputedStyle` still resolves, so token/CSS checks work regardless.
2. **`apps/app` has no `.env.local`** in a fresh checkout and can't render an
   authenticated page locally without one. Its public routes (`/book/[slug]`,
   `/docs`) render signed-out; the review, quote and DNS-setup pages moved to
   the Growth and Rankings zones in restructure step RST.
   (`apps/pos/web` and `apps/connect` were extracted on 2026-08-22 and are no
   longer in this repo.)
3. **Moving or deleting a route leaves a stale `.next/types/validator.ts`** that
   fails typecheck against paths that no longer exist. `rm -rf <app>/.next`.
4. **Next.js ignores `_`-prefixed folders.** `app/_preview/` 404s.

## Verifying

```bash
pnpm run typecheck && pnpm run lint && pnpm run test
```

Then look at it, in **both themes** and at **375 / 768 / 1280**. Confirm the
app's Tailwind `content` glob covers `packages/ui/src/**`, or classes used only
in the shared package are silently missing from that app's CSS. The shell
(`apps/app`) is one level deep, like each module repository's app, so the glob
is the same in each.
