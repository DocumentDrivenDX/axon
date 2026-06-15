---
ddx:
  id: helix.design-system
  depends_on:
    - helix.product-vision
    - helix.prd
    - helix.architecture
    - helix.concerns
  review:
    self_hash: c4f8d7506a25f115dd0acf1e289fccc108ea3c8f33843e9abe08d062baeee15e
    deps:
      helix.architecture: 8a39d93af466d83f02851986a0a935ddb0b8b12d1552ee3bc7e7af3c91fb145a
      helix.concerns: d22f8007944e442262ef2de1021079482f0c6ded29af8fed6f460cb6540055f3
      helix.prd: dff98156a6cc934f406611b78b513892d85cee1bd7b4c011f045146fcdfd23e1
      helix.product-vision: 60bf8c5d6260533c125c3b69308b4dcac72d317437ba60d1b1c6e4ea34105298
    reviewed_at: "2026-06-15T00:50:24Z"
kind: design
---

# Axon Design System

**System**: Axon website, documentation, sample projects, demo reels, and
future admin/operator surfaces
**Status**: Accepted baseline for the 0.7.1 documentation and microsite pass
**Date**: 2026-06-15

## Purpose

Axon's visual system must make a technical promise visible: agents and humans
can change durable business records through one governed, inspectable data
layer. The design should feel like an operational control surface rather than
a marketing site. It should help developers scan requirements, examples,
coverage, policies, schemas, mutation previews, and audit history without
losing context.

The system applies to:

- The Hugo/Hextra microsite under `website/`.
- Generated HELIX coverage, example, and demo-reel pages.
- Future admin UI views that inspect schemas, policies, mutation intents,
  approvals, and audit trails.
- Product diagrams, sample-project screenshots, and release documentation.

## Design Principles

### Governed Before Flashy

Use restraint. Axon is infrastructure for high-trust writes, so the default
surface should be dense, legible, and calm. Visual emphasis should clarify
authority, state, and evidence rather than decorate the page.

### State Is Always Inspectable

Schemas, policies, actors, approvals, version checks, and audit records should
look like first-class product objects. UI and docs should show the request
path instead of hiding it behind generic feature cards.

### Agents And Humans Share The Same Truth

Designs should show both the agent-facing tool surface and the human/operator
review surface. Avoid visuals that imply agents bypass governance.

### Coverage Is A Product Signal

The website should treat HELIX feature/story/scenario coverage as proof of
product completeness. Coverage counts, mapped examples, and demo reels should
be easy to find and visibly tied to the same design language as the rest of
the site.

## Brand Voice

Axon's brand voice is **precise, operational, trusted, developer-native, and
governance-first**. It should sound like infrastructure built for production
risk: direct, specific, and calm under pressure.

### Voice Attributes

| Attribute | Meaning | Copy Test |
| --- | --- | --- |
| Precise | Name the actual control or surface instead of using broad claims | Does the sentence name schema, policy, intent, approval, audit, version, repair, GraphQL, MCP, CLI, or storage? |
| Operational | Talk about what a developer or operator can inspect, run, reject, approve, or repair | Can the reader connect the sentence to a concrete workflow? |
| Trusted | Use evidence and boundaries instead of hype | Does the claim point to tests, examples, demo reels, specs, audit records, or explicit non-goals? |
| Developer-native | Prefer command, API, schema, and integration language over abstract platform language | Would a backend engineer know where to start? |
| Governance-first | Agents are capable participants, but policy and review remain on the request path | Does the copy avoid implying that agents bypass human or data-layer controls? |

### Message Pillars

Axon copy should return to these product promises:

- **One governed request path**: humans, agents, apps, CLI, GraphQL, and MCP
  use the same schema, policy, version, approval, and audit controls.
- **Business records are durable and inspectable**: entity data, links,
  schemas, policies, mutation previews, and audit history are first-class.
- **Agent writes need repair-grade evidence**: every mutation should preserve
  enough context to understand, reject, approve, replay, or repair it.
- **Coverage is proof**: examples, demo reels, HELIX mappings, and tests are
  part of how Axon earns trust.

### Vocabulary

Prefer:

- governed state
- transactional entity store
- business records
- schema validation
- policy decision
- mutation intent
- approval routing
- version check
- audit evidence
- repair-grade history
- GraphQL, MCP, CLI, SDK
- sample project, demo reel, HELIX coverage

Avoid:

- vague automation claims
- generic AI productivity claims
- copy that frames agents as unchecked actors
- "trust us" language without evidence
- metaphors that obscure the data path
- feature lists that do not name the control surface

### Copy Rules

- Lead with the category: "governed state" or "transactional entity store,"
  not a generic data platform label.
- Pair every agent capability with the control that contains it.
- Prefer verbs that match the product surface: define, validate, preview,
  approve, reject, commit, audit, replay, repair.
- Keep sentences short when describing risk, policy, or audit behavior.
- Use numbers only when they are generated or otherwise traceable.
- Treat docs, examples, and demo reels as evidence, not promotional extras.

### Copy Patterns

Good homepage line:

> Governed state for agents that write business records.

Good supporting copy:

> Axon gives developers one request path for schema validation, policy
> decisions, mutation previews, approval routing, and audit evidence.

Good example/reel copy:

> This sample exercises the invoice approval path: an agent previews a risky
> write, finance reviews the intent, and Axon records the committed audit
> trail.

Poor-fit copy:

> Let agents automate your data layer with next-generation intelligence.

Why it fails: it does not name the control surface, evidence, policy boundary,
or concrete workflow.

## Information Architecture

The microsite navigation keeps the current Hextra structure, with these
semantic roles:

| Navigation Item | Role | Design Treatment |
| --- | --- | --- |
| Docs | Orientation, concepts, and getting started | Default entry point, concise descriptions, strong next-step links |
| CLI Reference | Concrete command surface | Monospace-heavy pages, compact examples, clear command/result distinction |
| Coverage | HELIX proof matrix | Metric-forward, table-friendly, link-rich |
| Examples | Fully worked sample projects | Scenario cards, setup commands, expected artifacts |
| Demo Reels | Walkthrough evidence | Timeline or terminal-reel presentation with visible scenario IDs |
| GitHub | Source and installation authority | External-link affordance, lower visual priority than product docs |

### Active State

Every persistent navigation surface must expose an unambiguous active state.
The implementation target is:

- The active link has `aria-current="page"` when it points at the current
  page.
- Top navigation active links use primary text plus a 2 px bottom inset cue.
- Sidebar active links use primary text, a left inset cue, and a quiet tinted
  surface.
- Hover states are lighter than active states and never rely on color alone.
- Focus states use a 2 px focus ring with enough offset to remain visible on
  white, muted, and dark surfaces.

## Visual Hierarchy

### Homepage

The first viewport should communicate Axon's category and proof in three
seconds:

1. Axon is governed state for agents that write business records.
2. The product path is schema -> policy -> mutation intent -> approval or
   commit -> audit.
3. The documentation has complete HELIX coverage with examples and demo reels.

The hero should use a product-like command/control surface rather than a
generic illustration. The visual should show a request path, state checks, and
audit evidence.

### Documentation Pages

Docs optimize for scanning:

- One clear page title and a short summary before detailed sections.
- Tables for coverage matrices and command references.
- Monospace examples for CLI, GraphQL, MCP, policy, and schema artifacts.
- Status labels for previewed, approved, rejected, committed, audited, and
  repaired states.
- Links to examples and demo reels near the concepts they exercise.

### Sample Projects And Demo Reels

Examples and reels should look like executable evidence, not screenshots of
requirements. Each page should highlight:

- Scenario or story IDs.
- Setup commands.
- Product surfaces exercised.
- Expected outcome.
- Audit or policy evidence produced by the run.

## Design Tokens

These tokens are implemented in `website/assets/css/custom.css` and should be
mirrored by future app surfaces.

### Color

| Token | Value | Use |
| --- | --- | --- |
| `--axon-ink` | `#111827` | Primary text |
| `--axon-muted` | `#536171` | Secondary text |
| `--axon-surface` | `#ffffff` | Main surface |
| `--axon-surface-muted` | `#f8fafc` | Page bands and table backgrounds |
| `--axon-surface-strong` | `#eef4f6` | Raised control surfaces |
| `--axon-line` | `#d6e0e8` | Borders and separators |
| `--axon-primary` | `#0f766e` | Primary actions, active navigation, governed path |
| `--axon-primary-strong` | `#0b5f59` | Primary hover and strong text |
| `--axon-accent` | `#2563eb` | Links, graph edges, secondary proof points |
| `--axon-audit` | `#b45309` | Approval, guardrail, audit, and review states |
| `--axon-success` | `#15803d` | Passed checks, committed writes |
| `--axon-danger` | `#b42318` | Rejected writes and destructive states |
| `--axon-violet` | `#6d28d9` | Rare accent for MCP/tooling, never the dominant hue |
| `--axon-focus` | `#f59e0b` | Keyboard focus ring |

Dark surfaces keep the same semantic assignments with adjusted values rather
than inverting the whole palette. The dominant page colors remain neutral,
with teal for governance, blue for graph/API movement, and amber for audit.

### Typography

Use system sans-serif text and system monospace code. Letter spacing is `0`.

| Role | Desktop | Mobile | Notes |
| --- | --- | --- | --- |
| Hero display | 56 px / 60 px | 40 px / 46 px | Homepage only |
| Page H1 | 36 px / 44 px | 32 px / 40 px | Documentation and examples |
| Section H2 | 28 px / 36 px | 24 px / 32 px | Major sections |
| Body | 16 px / 26 px | 16 px / 26 px | Default |
| Dense body | 14 px / 22 px | 14 px / 22 px | Tables, sidebars, metadata |
| Code | 13 px / 20 px | 13 px / 20 px | CLI, schema, policy, logs |

### Spacing

Use a 4 px base grid:

| Token | Value | Use |
| --- | --- | --- |
| `--axon-space-1` | 4 px | Tight icon and label gaps |
| `--axon-space-2` | 8 px | Compact control padding |
| `--axon-space-3` | 12 px | Table and badge padding |
| `--axon-space-4` | 16 px | Default stack gap |
| `--axon-space-6` | 24 px | Section internals |
| `--axon-space-8` | 32 px | Grid gaps |
| `--axon-space-12` | 48 px | Major section padding |
| `--axon-space-16` | 64 px | Homepage section separation |

### Shape And Elevation

- Border radius is `8px` for cards, panels, buttons, badges, and code blocks.
- Repeated cards may use a light border and subtle shadow.
- Page sections are full-width bands or unframed layouts, not floating cards.
- Avoid nested cards. If a card needs internal grouping, use separators,
  metadata rows, or compact badges instead.
- Avoid decorative orbs and bokeh backgrounds. Use grid lines, schema rows,
  command surfaces, and request-path diagrams when a visual is needed.

## Components

### Buttons

Primary actions use teal fill, white text, and a darker teal hover state.
Secondary actions use a white or muted surface, primary text, and a visible
border. Buttons must have `:hover`, `:active`, `:disabled`, and
`:focus-visible` states. Button text should not wrap awkwardly; use responsive
stacking rather than shrinking text below body scale.

### Badges And Status Labels

Badges should be compact and semantic:

| State | Color Role |
| --- | --- |
| `schema` | Primary |
| `policy` | Accent |
| `approval` | Audit |
| `committed` | Success |
| `rejected` | Danger |
| `MCP` or `tool` | Violet accent |

### Tables

Tables should use sticky-readable hierarchy:

- Header row in muted surface.
- Row separators using `--axon-line`.
- Monospace for IDs, commands, and field names.
- No zebra striping unless the table is very wide.
- Links remain visibly links without overpowering body text.

### Command And Audit Surfaces

Command surfaces should resemble product evidence:

- Header row with the command, surface, or actor.
- Monospace command and output lines.
- Inline status labels for schema, policy, version, approval, and audit
  checks.
- Distinct left-border or icon treatment for warnings and rejected writes.
- Copyable command support when implemented in interactive surfaces.

### Request Path Diagram

Use a left-to-right or top-to-bottom path:

1. Agent or app request.
2. Schema validation.
3. Policy and visibility decision.
4. Mutation intent preview.
5. Approval routing or direct commit.
6. Audit and CDC evidence.

Each stage must have a label and a visible state. Do not represent Axon as a
black box between an agent and a database.

## Interaction States

| State | Requirement |
| --- | --- |
| Hover | Surface tint, border emphasis, or underline. Do not rely on color alone. |
| Active/current | `aria-current="page"` plus visible active cue. |
| Focus | 2 px focus ring using `--axon-focus`, with at least 2 px offset. |
| Disabled | Lower contrast, no pointer cursor, preserved layout size. |
| Loading | Skeleton row or stable reserved space, never layout jump. |
| Empty | Short factual message plus one next action when available. |
| Error | Danger color plus specific failure reason and recovery path. |

## Accessibility

- All text and UI controls must meet WCAG AA contrast.
- Keyboard focus must be visible on every interactive element.
- Demo reels and terminal casts need text alternatives or adjacent command
  summaries.
- Do not encode scenario state by color alone.
- Links that leave the site should have an external-link indicator in text or
  icon treatment.
- Generated pages should preserve stable headings so table-of-contents and
  deep links remain predictable.

## Implementation Guidance

The current website uses Hugo with Hextra. The source-level design pass lives
in:

- `website/content/_index.md` for product-specific homepage structure.
- `website/assets/css/custom.css` for tokens, components, navigation states,
  and generated-page polish.
- `website/content/docs/*` for page content generated from HELIX coverage.

Build gate:

```bash
cd website && hugo --gc --minify
```

Repository gate:

```bash
cargo test
```

## Non-Goals

- This document does not define the Rust API, storage engine, or policy
  semantics.
- This document does not replace HELIX feature specs, user stories, or ADRs.
- This document does not require a custom JavaScript framework for the website.
- This document does not mandate final brand assets such as a logo, typeface,
  or marketing illustration.
- This document does not authorize editing generated coverage content by hand.
