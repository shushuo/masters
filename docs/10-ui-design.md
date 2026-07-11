# 10 — UI / UX Design System

This document is the pixel-level companion to `docs/07-ux-flows.md` (which stays wireframe-level).
It records the **current** desktop design system as implemented under `ui/desktop/src/` — color
tokens, layout, components, and interaction principles — then benchmarks it against the products
Masters deliberately draws from (**Manus**, **Claude Cowork**, Notion), and closes with a
prioritized **enhancement design**. Where a gap is stated it was verified against the source (file
references throughout); where a contrast ratio is stated it was computed (WCAG 2.1 relative
luminance).

Scope: the Tauri/React desktop app (`ui/desktop`). The marketing site lives in `masters-cloud` and
is out of scope. Nothing here changes the architectural invariants — the desktop stays
dependency-light, bundle-only verified (`tsc --noEmit` + `pnpm build`), and every visual decision
must keep the trust surfaces (gating, audit, revert) first-class.

> **Implementation status (P0 + P1 landed).** The §7 roadmap's P0 and P1 slices are now
> implemented on the desktop; §7.9 marks each item. Highlights: self-hosted Inter + AA-contrast
> tokens + a single-source dark palette; a Markdown transcript with collapsible tool-step cards,
> stick-to-bottom, and a multiline composer; keyboard shortcuts + an approval focus-trap +
> live-region a11y; hash routing with a sidebar session list (and a `DELETE /sessions/{id}`
> endpoint); and **diff-in-approval** — the gate now attaches a display-only before/after
> `FilePreview` to write approvals (proto `FilePreview`, `permission::file_preview`), rendered as a
> diff card in the approval bar. Still open: the P1 live *plan* panel (needs a server-side plan
> event) and P2 (command palette, per-master avatar identity).

---

## 1. Design principles (as built)

The current system is Notion/Manus-inspired and encodes five working principles:

1. **Tokens over utilities.** All color flows through CSS variables in `src/index.css`; Tailwind
   only aliases them (`tailwind.config.js`). Components never hard-code a hex value, so the dark
   theme is a token override, not a `dark:` sweep.
2. **Semantic naming.** Tokens name *roles* (`--color-surface-2`, `--color-tool-fg`,
   `--color-danger-bg`), not hues. The brand hue can be retuned in one file.
3. **Dependency-light primitives.** Eight shared primitives (`src/components/ui/*`) instead of a
   component framework: Button, IconButton, Card, Input, Textarea, Select, Badge, Brand. No
   headless-UI library, no CSS-in-JS — `cn()` (a tiny class joiner) is the whole abstraction.
4. **Trust is visible.** The permission gate, audit trail, and revert are UI citizens: an approval
   bar interrupts the chat, a right-hand audit panel lists every gated call with a decision badge,
   and Revert sits next to Send. Agent tool activity renders inline (single chat) and attributed
   per-master (group chat).
5. **The daemon is the source of truth.** The UI owns no business state; every screen is a thin
   client over the generated OpenAPI client (`src/api/client.ts`), and screens degrade to
   non-fatal notices when a read fails.

These principles are sound and are **kept** by every enhancement in §7 — the proposals refine the
system, they do not replace it.

## 2. Color system

### 2.1 Brand triad

The palette derives from the panda logo (`assets/logo.svg`) — see the header comment in
`src/index.css`:

| Role | Hex | Use |
|---|---|---|
| **Cream** | `#f3f3e7` | Light canvas/surfaces; dark-theme text warms toward it |
| **Slate** | `#445159` | Light-theme accent; dark canvas; brand body |
| **Sage** | `#7ea39c` | Secondary highlight; becomes the dark-theme accent |

The one non-obvious decision is documented in the CSS: **the accent hue swaps per theme**. Slate
is the accent on cream, but slate-on-slate would vanish, so dark mode promotes sage
(`--color-accent: #8fb3ad`). Components built on `bg-accent`/`text-accent` recolor automatically.

### 2.2 Token architecture

Four token families, each with a light value in `:root` and a dark override (duplicated by hand in
the `prefers-color-scheme` block and `[data-theme="dark"]` — see §7.4 for the de-duplication fix):

**Surfaces** — a 3-step elevation ramp plus 2 border weights:

| Token | Light | Dark | Role |
|---|---|---|---|
| `--color-bg` | `#faf9f1` | `#1a2125` | App canvas |
| `--color-surface` | `#f3f1e6` | `#212a2f` | Sidebar, sunken panels |
| `--color-surface-2` | `#eae6d6` | `#2a343a` | Cards, hover wells |
| `--color-border` | `#e5e1d0` | `#313c42` | Hairlines |
| `--color-border-strong` | `#d6d1bb` | `#3d4951` | Inputs, weighty dividers |

**Text** — a 3-step hierarchy: `text` (body) / `text-muted` (secondary labels) / `text-faint`
(timestamps, slugs).

**Accent & brand** — `accent` / `accent-hover` / `accent-fg` (content on accent) /
`accent-subtle` (active-nav tint); `brand` + `brand-sage` for the wordmark and flourishes.

**Semantic state** — a dedicated **tool** family (`tool-bg`/`tool-fg`/`tool-border`, an earthy
amber that marks *agent tool activity* everywhere: tool-step lines, the approval bar, tool
badges), plus `success(-hover)` and `danger(-hover/-bg/-fg)`. There is **no independent
warning/info family** — `warning` currently aliases the tool colors in `Badge.tsx`, which
overloads "agent did something" with "be careful" (§7.4).

Non-color tokens: radii `--radius-sm/base/lg` (7/10/14 px, softened to echo the round mark) and
two warm-tinted shadows (`--shadow-sm`, `--shadow`).

### 2.3 Theme mechanics

`color-scheme: light dark` lets native controls follow the OS. The theme is **tri-state**
(`system | light | dark`, `src/lib/theme.ts`): "system" removes `data-theme` so the
`prefers-color-scheme` media query wins; pinning sets `data-theme` on `<html>` and persists to
`localStorage` (`getmasters-theme`), applied pre-render by `initTheme()` to avoid a
flash-of-wrong-theme. The Sidebar's toggle cycles system → light → dark.

### 2.4 Measured contrast (WCAG 2.1)

| Pair | Light | Dark | Verdict |
|---|---|---|---|
| `text` on `bg` | 10.7:1 | 13.5:1 | AAA |
| `text-muted` on `bg` | 5.1:1 | 6.5:1 | AA |
| `text-faint` on `bg` | **2.9:1** | **3.6:1** | **Fails AA (4.5:1)** |
| `accent-fg` on `accent` | 7.5:1 | 7.2:1 | AAA |
| `tool-fg` on `tool-bg` | **4.2:1** | 7.9:1 | **Light fails AA by a hair** |
| `success` on `bg` | 4.5:1 | — | AA (borderline) |
| `danger` on `bg` | 4.7:1 | — | AA |
| white on `danger` (button) | 4.9:1 | — | AA |

`text-faint` is used for genuinely incidental content (timestamps, slugs, session ids) but also
for **tool lines in group chat** and the round divider — content a user reads. Fix in §7.5.

## 3. Typography, iconography, elevation

- **Type**: a single stack — `"Inter var", Inter, ui-sans-serif, system-ui, …`. ⚠️ **Inter is
  referenced but never shipped**: nothing in `index.html` or the bundle loads the font, so every
  install without Inter locally renders system UI type. Either bundle it (self-hosted
  `@font-face`, Tauri-friendly, no CDN) or remove it from the stack — the current state is a
  silent per-machine inconsistency (§7.4).
- **Scale** (by convention, not tokens): `text-xs` for meta/badges/tool lines, `text-sm` for
  nearly all UI copy, `text-base`/`text-lg`/`text-xl` for brand and screen titles, mono
  (`font-mono`) for tool output, args, ids. ⚠️ `MastersHub.tsx` uses `font-display`, a utility no
  config defines — it silently no-ops (§7.4).
- **Icons**: `lucide-react` exclusively, sized `size-4` (16px) inline / `size-3.5` in small
  buttons, always `aria-hidden` with a text or `aria-label` companion. The brand mark
  (`PandaMark`/`Wordmark`, `ui/Brand.tsx`) is a single SVG legible on both themes.
- **Elevation**: borders do most separation work (flat, Notion-like); shadows are reserved and
  subtle. Radius: `rounded-sm` (7px) for controls/chips, `rounded` (10px) for cards/panels,
  `rounded-lg` (14px) for chat bubbles.

## 4. Layout system

### 4.1 App shell

```
┌──────────┬──────────────────────────────────────────────┐
│ Sidebar  │ main (min-w-0 flex-1)                        │
│ w-60 /   │ ┌ update banner (conditional) ─────────────┐ │
│ w-14     │ │ view: Chat | MastersHub | Projects |     │ │
│ collapsed│ │       ProjectDetail | Settings |         │ │
│          │ │       Onboarding                         │ │
│ nav      │ └──────────────────────────────────────────┘ │
│ ⋮        │                                              │
│ Settings │                                              │
│ ⚙ theme  │                                              │
│ ● daemon │                                              │
└──────────┴──────────────────────────────────────────────┘
```

- **Sidebar** (`Sidebar.tsx`): brand + collapse toggle; primary nav (Chat / Masters / Projects);
  a pinned-bottom Settings entry + theme toggle; a daemon-status footer (green dot ·
  `provider · version`, pulsing while connecting). Collapsed it becomes a 56px icon rail with
  `title` tooltips. It owns no routing state — `App.tsx` is the single source of truth for the
  active `View`.
- **Main region** hosts exactly one view. Routing is a `useState<View>` switch — fine at this
  scale, but there is no URL/back-stack (§7.6).

### 4.2 Screen archetypes

Three recurring layouts:

1. **Conversation** (`Chat.tsx`, `GroupChat.tsx`): header bar (session switcher / roster) →
   scrollable transcript → conditional interrupt strip (approval / notice / error) → composer
   row. Chat adds a toggleable **right context panel** (`w-72`, `bg-surface`): session id, the
   audit trail (tool + decision badge + redacted args + timestamp), and Revert — the direct
   descendant of docs/07 §2's "right panel".
2. **Hub with tabs** (`MastersHub.tsx`, `ProjectDetail.tsx`): title header → an underline tab
   bar (`border-b-2 border-accent` active state) → a scrollable tab body. ProjectDetail nests
   the Phase-2/3/4 management tabs (Instructions/Knowledge/Memory/Skills/Study/Recipes/Routines/
   Masters/Teams/Extensions).
3. **List/detail** (`Projects.tsx` → `ProjectDetail.tsx`; Teams → GroupChat): card grids
   (`grid md:grid-cols-2`) of interactive `Card`s, with a ghost "back" button in the detail
   header rather than breadcrumbs.

Density is uniform: `p-4` bodies, `px-3 py-2` bars, `gap-2/3` stacks, `space-y-*` lists.

## 5. Component inventory

### 5.1 Primitives (`src/components/ui/`)

| Component | Variants / notes |
|---|---|
| `Button` | `primary` (accent), `secondary` (outlined on `bg`), `ghost`, `danger`; sizes `sm`/`md`; focus ring `ring-2 ring-accent`, `disabled:opacity-50` |
| `IconButton` | `ghost`/`secondary`; **requires** a `label` used as both `aria-label` and tooltip |
| `Card` | bordered `bg` panel; `interactive` adds hover border/fill for clickable cards |
| `Input` / `Textarea` / `Select` | share `inputBase` (border-strong, `bg`, accent focus ring); Select overlays a lucide chevron on a native `<select>` |
| `Badge` | `neutral`/`accent`/`tool`/`success`/`warning`/`danger` chips; `warning` duplicates `tool` |
| `PandaMark` / `Wordmark` | brand lockup, 3 sizes |

### 5.2 Composed patterns (in-screen, not yet extracted)

- **NavButton** (Sidebar): icon + label, `aria-current="page"`, `bg-accent-subtle` active tint.
- **Chat bubble**: user right-aligned `bg-accent text-accent-fg`, assistant left `bg-surface-2`,
  asymmetric corner (`rounded-br-sm`/`rounded-bl-sm`), `max-w-[80%]`, plain
  `whitespace-pre-wrap` text (no markdown — §7.1).
- **Tool-step line** (Chat): a full-width mono strip on the `tool` palette, wrench icon (or
  `CircleAlert` on error) + summary. Call and result render as **two separate strips** with no
  grouping or expandable detail (§7.2).
- **Approval bar** (Chat): a `tool`-tinted strip above the composer — tool badge + summary +
  permission classes, and the three standing choices from docs/06/07: **Allow once / Always
  allow this tool / Deny**.
- **Audit entry** (Chat panel): mono tool name + decision `Badge`
  (`auto`→neutral, `approved`→success, `denied`→danger) + redacted-args `<pre>` + time.
- **Group-chat bubble** (GroupChat): author label (`text-faint`) above the bubble, per-bubble
  dim tool lines (`→ tool` / `← result` / `⚠️`), a centered `ROUND n` divider when a
  mention-driven follow-up round begins, and **@-mention chips** (roster buttons that insert
  `@slug`; the coordinator gets a ★).
- **Status dot**: sidebar daemon health; also the pattern for Settings' provider-check result.
- **Empty states**: Chat's centered mark + "What can Masters do for you?"; list tabs use a
  one-line `text-muted` hint. No illustrated/action-forward empty states yet (§7.6).

### 5.3 Feedback conventions

- Errors: red text lines (`text-danger`) inline, or a `danger-bg` box for fatal daemon errors.
- Notices: a `bg-surface` strip above the composer (e.g. revert results) — passive, not a toast.
- Loading: text placeholders ("Loading masters…", "connecting…"); no skeletons or spinners
  beyond `animate-pulse` on the status dot and `animate-spin` on the catalog-sync icon.
- Streaming: token deltas append into the last assistant bubble; group chat seeds an empty
  bubble per addressed master on `GroupStart` and fills per-author deltas; "…" is the
  in-flight placeholder. Send flips to **Stop** while streaming.

## 6. Interaction principles

1. **Gate, then show your work.** Every side-effecting call surfaces twice: at decision time
   (approval bar, unless headless-gated) and after the fact (audit panel, tool strips). This is
   the product's core differentiator and the UI treats it as content, not chrome.
2. **Escape hatches always visible.** Stop is available during any stream (and aborts all
   in-flight group masters); Revert sits permanently next to the composer.
3. **Deterministic addressing, visible.** Group chat exposes the ADR-0012 addressing rules in
   the UI itself — the chip row *is* the affordance (`@all`, per-master chips, ★ coordinator),
   the empty state teaches the rule, and rounds are visually explicit.
4. **The UI never blocks on non-fatal failures.** Session-list, audit, roster loads all degrade
   silently or to a notice.
5. **Single-keystroke send**: Enter sends everywhere; composers disable while `busy`/`streaming`
   rather than queuing. (Multiline input is the cost — §7.1.)
6. **Theme follows the OS by default** and can be pinned; no per-screen theming.

Current keyboard surface is thin: Enter-to-send is the only shortcut; there is no palette, no
Esc-to-stop, no `⌘N` new chat (§7.6). Focus rings are consistently `focus-visible` (good), and
icon-only controls are labelled — the a11y baseline is real but unaudited beyond that.

---

## 7. Benchmark: Manus & Claude Cowork — and the enhancement design

### 7.1 What the benchmarks do well

**Manus** (agentic "AI worker"):

- **"Manus's Computer" panel** — the signature move: a persistent right-hand viewport showing
  what the agent is *doing right now* (browser, editor, shell), with a **step-wise task plan**
  (todo list) that ticks off as the run progresses. The user watches progress, not a spinner.
- **Task-centric sessions** — runs are long-lived tasks with progress states, resumable and
  reviewable; deliverables (files, sites, reports) are collected as **artifacts**, not lost in
  the transcript.
- Calm, mostly-monochrome visual language that lets activity color (status chips, plan ticks)
  carry meaning.

**Claude Cowork** (Anthropic's agentic desktop/workspace):

- **Working-context transparency** — the folder/files the agent may touch are always visible;
  permissioning is progressive and legible (this, Masters already matches — grants + gate).
- **Plan-before-act** — multi-step work surfaces an editable plan/checklist up front; long runs
  show per-step status lines rather than raw tool noise.
- **Rich transcript** — full markdown (code blocks with syntax highlighting + copy, tables,
  lists), collapsible tool/"working" sections, file diffs rendered as diffs, artifact previews.
- Refined type + spacing; a **multiline composer** (Enter sends, Shift+Enter newline, attach,
  model picker) that reads as a serious workbench, not a chat toy.

**Where Masters is already ahead** of both: the **explicit trust surface** (per-call audit trail
with decision badges and redacted args in the UI — neither benchmark exposes this), **per-master
model/persona identity** (ADR-0013), deterministic @-mention **multi-master rounds** (ADR-0012),
and a local-first posture the UI can honestly advertise (per-master privacy boundary).

### 7.2 Gap analysis (summary table)

| Dimension | Masters today | Manus / Cowork | Gap severity |
|---|---|---|---|
| Transcript rendering | plain pre-wrap text | full markdown, code blocks, diffs | **High** |
| Composer | single-line `Input` | multiline, Shift+Enter, attach | **High** |
| Agent activity | flat mono strips, call/result split | grouped, collapsible, status-ful steps; live plan | **High** |
| Plan/progress visibility | none | first-class (both) | High |
| Session navigation | a `<select>` dropdown | scannable list w/ titles, search, pinning | Medium |
| Artifacts/deliverables | none (files change on disk) | collected + previewable | Medium |
| Diff preview on writes | promised in docs/07 §4, absent | Cowork renders diffs | Medium |
| Auto-scroll / stick-to-bottom | absent (streams can run off-screen) | standard | **High (cheap)** |
| Keyboard depth | Enter only | palette, shortcuts | Medium |
| Empty/loading states | text lines | guided, action-forward | Low |
| Trust/audit surface | **ahead** | — | keep & amplify |
| Multi-agent identity | slugs + ★ | n/a (single agent) | amplify (avatars/colors) |

### 7.3 Enhancement A — the transcript becomes a work surface (P0)

The single highest-leverage change. Four parts, all local to `Chat.tsx`/`GroupChat.tsx` plus two
small dependencies (`react-markdown` + a highlighter, or a hand-rolled marked-style renderer to
stay dependency-light):

1. **Markdown rendering** of assistant/master bubbles (streaming-safe: render on each delta;
   code blocks get syntax highlighting + a copy button). User bubbles stay plain.
2. **Tool-step cards**: merge each `ToolCallStarted`+`ToolResult` pair (correlated by the `id`
   both events already carry) into **one collapsible card** — header line = status icon
   (spinner → check/`CircleAlert`) + tool name + summary, body (collapsed by default) = args +
   result. Consecutive read-only calls collapse into a "Read N files"-style group, mirroring
   docs/07 §2's "tool-step cards, each expandable". Group chat reuses the same card inside
   author bubbles, replacing the `text-faint` mono lines (also fixes the §2.4 contrast fail).
3. **Stick-to-bottom auto-scroll** with the standard escape (a "jump to latest ↓" pill when the
   user has scrolled up). Trivial (`ref` + `scrollIntoView` on delta) and currently missing —
   streams literally run off-screen.
4. **Multiline composer**: swap `Input` for an auto-growing `Textarea` (max ~8 rows); Enter
   sends, Shift+Enter newlines; keep the Stop/Send swap. Shared between Chat and GroupChat
   (extract a `Composer` primitive; GroupChat adds the chips row and rounds select as slots).

### 7.4 Enhancement B — token & correctness fixes (P0, zero-risk)

- **Ship or drop Inter.** Recommended: self-host `InterVariable.woff2` in `public/fonts/` +
  `@font-face` in `index.css` (no CDN — the desktop must not need egress to render). Add a
  `--font-display` face (or delete the stray `font-display` class in `MastersHub.tsx`).
- **De-duplicate the dark palette.** The dark block is pasted twice ("kept in sync by hand").
  Move it to a shared custom property set via a `[data-theme="dark"], @media(dark) :root:not([data-theme="light"])`
  strategy — e.g. define the dark values once in a CSS layer and `@import`/mixin them, or
  generate both blocks from one source in a tiny build step. Hand-sync will eventually drift.
- **Split `warning` from `tool`.** Add a true `--color-warning(-bg/-fg)` family (amber, distinct
  from the earthy tool tint) so Badge's `warning` stops aliasing "agent activity".
- **Add an `info` family** (sage-tinted) for passive notices — today notices borrow `surface`.
- **Motion & focus tokens**: formalize the implicit 150ms standard as `--duration-fast/base`,
  honor `prefers-reduced-motion` globally (one `@media` rule disabling transitions), and add a
  `--focus-ring` token.

### 7.5 Enhancement C — accessibility to AA (P0/P1)

- Darken `--color-text-faint` to ≥ 4.5:1 (light: `#6d787e` ≈ 4.6:1 on `bg`; dark: `#8a969b`),
  or restrict `text-faint` to true decoration and promote read-content (tool lines, round
  dividers, group author labels) to `text-muted`.
- Nudge light `--color-tool-fg` (`#8a6d2f` → `#7d6229` ≈ 4.9:1) over the AA line.
- `aria-live="polite"` on the transcript container so streaming completions and approval
  prompts reach screen readers; `role="log"` on the transcript.
- Keyboard: Esc = Stop while streaming; `⌘/Ctrl+N` new chat; trap focus in the approval bar
  while a decision is pending (it is the security-critical control).

### 7.6 Enhancement D — navigation & structure (P1)

- **Session list, not a dropdown**: replace Chat's `<select>` with a session list in the sidebar
  under the Chat nav item (title + relative time, hover-delete, search), matching both
  benchmarks and docs/07 §2's left rail ("projects, sessions within the active project"). The
  sidebar already collapses; sessions become its scrollable middle section on the Chat view.
- **Lightweight routing**: encode `view` (+ project id / session id) in the URL hash so
  back/forward and deep links work; keeps `useState` semantics, adds history.
- **Breadcrumbs** for nested contexts (Projects → project → tab; Masters → quick chat) instead
  of per-screen back buttons.
- **Command palette** (`⌘K`): views, sessions, masters, "new chat with @master", theme — the
  standard power-user escape hatch in Notion/Cowork-class products; can be built
  dependency-light on the existing primitives.
- **Action-forward empty states**: each empty tab offers its primary action ("Define your first
  master", "Grant a folder") instead of a sentence.

### 7.7 Enhancement E — the agent-progress panel (P1/P2, the Manus move)

Upgrade Chat's right panel from a static audit list into a **live run panel** with three stacked
sections (the audit trail remains — it is the differentiator):

1. **Plan** (when present): agents that emit a plan/steps render a tick-list; per-step status
   from the event log (migration 0020 already persists tool_call/tool_result/complete rows —
   the data exists server-side).
2. **Activity**: the live tool-step cards of the current turn (same component as §7.3.2),
   auto-following the run.
3. **Audit**: the existing gated-call trail, now visually consistent with Activity.

For **writes**, render the before/after as a **diff card** in the approval bar (docs/07 §4
promises "Writes show a diff/preview" — the revision system of Phase 1b already snapshots
files, so the daemon can serve the diff). This is the one enhancement that needs a small server
addition (a diff endpoint or an enriched approval payload).

### 7.8 Enhancement F — multi-master identity (P2, the Masters move)

Group chat is the product's most distinctive surface; make authorship instantly scannable:

- **Deterministic per-master identity**: hash the slug into a hue and derive a
  bubble-edge/avatar color (both themes: fixed saturation/lightness per theme so contrast holds);
  a 2-letter avatar disc next to the author label; the coordinator keeps ★.
- **Roster header**: replace the plain title with the avatar row (present = addressed this
  round, dimmed otherwise), reusing the chip row's data.
- **Typing/working states per master**: the seeded-empty-bubble "…" becomes a labelled
  "@architect is working — round 2" shimmer; `MasterError` renders as a danger chip on the
  bubble instead of an inline ⚠️ string.
- **Round dividers** get the follow-up *reason* ("architect mentioned @copy-writer") — the
  mention that triggered the round is already in the prior reply's text.

### 7.9 Prioritized roadmap

| Pri | Item | Status | Files touched | New deps |
|---|---|---|---|---|
| P0 | Markdown transcript + tool-step cards + stick-to-bottom + multiline composer (§7.3) | **Done** | Chat, GroupChat, `ui/Composer`, `ui/ToolStep`, `ui/Markdown`, `lib/useStickToBottom`, `lib/clipboard` | `react-markdown`, `remark-gfm` |
| P0 | Font shipping, dark-palette de-dup, warning/info tokens, motion/focus tokens (§7.4) | **Done** | `index.css`, `tailwind.config.js`, `lib/theme.ts`, `public/fonts/`, `Badge` | none |
| P0 | AA contrast fixes + `aria-live` + Esc/⌘N + approval focus-trap (§7.5) | **Done** | `index.css`, Chat, GroupChat, Sidebar | none |
| P1 | Sidebar session list + hash routing + `DELETE /sessions/{id}` (§7.6) | **Done** | App, Sidebar, Chat, `lib/useHashRoute`, `lib/useSessions`, `routes/sessions.rs` | none |
| P1 | Diff-in-approval (§7.7) | **Done** | proto `FilePreview`, `permission/mod.rs` `file_preview`, `routes/ws.rs`, `openapi.rs`, client, Chat `DiffPreview` | none |
| P1 | Live *plan* panel (§7.7) | Deferred — needs a server-side plan event (the event log has no plan rows); live activity is already inline via tool-step cards | — | — |
| P1 | Breadcrumbs + action-forward empty states (§7.6) | Deferred (cosmetic) | screens | none |
| P2 | Command palette (§7.6) | Not started | new `ui/Palette` | none |
| P2 | Master identity system (§7.8) | Not started | GroupChat, `ui/Avatar` | none |

Everything stays bundle-only verifiable (`tsc --noEmit` + `pnpm build`); the one server-side
change (the §7.7 diff surface + the session-delete route) is additive and serde-`default`
backward-compatible. No enhancement weakens a trust surface, and several (tool cards, the diff
preview) make the gate *more* legible — the direction the ADRs point.

---

## 8. Appendix: file map

| Concern | Location |
|---|---|
| Color/radius/shadow tokens, themes | `ui/desktop/src/index.css` |
| Tailwind aliasing of tokens | `ui/desktop/tailwind.config.js` |
| Theme preference logic | `ui/desktop/src/lib/theme.ts` |
| Primitives | `ui/desktop/src/components/ui/*` |
| App shell + routing state | `ui/desktop/src/App.tsx`, `components/Sidebar.tsx` |
| Chat + audit panel + approvals | `components/Chat.tsx` |
| Group chat (rounds, chips, attributed streams) | `components/GroupChat.tsx` |
| Hubs & management screens | `components/MastersHub.tsx`, `ProjectDetail.tsx`, etc. |
| Wireframe-level flows (upstream of this doc) | `docs/07-ux-flows.md` |
