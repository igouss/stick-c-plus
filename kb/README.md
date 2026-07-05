# kb — knowledge base for the M5StickC Plus

A lab notebook for this board: what we read or download, what we test on the
metal, and the durable knowledge we distill from both. Plain markdown + YAML
frontmatter + git. No database — greppable, diffable, survives neglect. Modelled
on `~/kbe` (see its own `README.md` for the full rationale).

## The one rule: two voices, never mixed

| Layer | Voice | Lives in | Lifecycle |
|-------|-------|----------|-----------|
| **Sources** | **Raw** — external inputs (datasheets, cloned repos, articles) | `sources/` | cited, with a regeneration recipe |
| **Experiments** | **Raw** — verbatim method + numbers from the actual board | `experiments/` | append-only, dated, frozen once closed |
| **Findings** | **Derived** — distilled, falsifiable claims about the board | `findings/` | living; superseded, never silently rewritten |
| **Guides** | **Derived** — prose how-tos synthesizing several sources | `guides/` | living; hand-indexed |

A *source* records what we read/cloned and how to get it again. An *experiment*
records what happened when we probed the hardware. A *finding* records what we now
believe, one falsifiable claim per file, pointing back to the experiment that
earns it. A *guide* is operational prose (toolchain, flashing) with no single
check. Evidence is immutable; belief is versioned — refute a finding with a new
finding that `supersedes:` it, don't quietly edit history.

## Layout

```
kb/
  README.md          this file
  INDEX.md           curated one-line pointers, grouped by topic (hand-maintained)
  templates/         source.md, finding.md, experiment.md, guide.md
  sources/
    <slug>/          the material: a cloned repo (git submodule) or fetched files
    <slug>.md        the paired note: citation, license, regeneration recipe
  experiments/
    YYYY-MM-DD-<slug>/README.md   what we did to the board + raw results + verdict
  findings/
    <slug>.md        one durable claim per file, cross-linked
  guides/
    <slug>.md        derived how-tos (toolchain, flashing, board reference)
```

## Conventions

- **Slugs.** Dated *captures* (an article, a video, a session) get `YYYY-MM-DD-<slug>`
  — they're events. Durable *references* (a datasheet set, a cloned library) get a
  bare `<slug>` so their paths stay navigable (you `cd` into cloned code often).
  This is the one deliberate divergence from `~/kbe`, which dates every source.
- **Experiment id** = `YYYY-MM-DD-<slug>` (dated event). **Finding id** = `<slug>`
  (living belief, no date).
- **Sources are cited, never claimed.** Each `sources/<slug>.md` carries a
  citation, a `license` note, and a regeneration recipe (a `git submodule update`,
  a `fetch.sh`, an `espflash` command). Reproducibility over a frozen copy.
- **Cloned repos are git submodules**, pinned to a commit — provenance is exact and
  their history never enters ours. `git submodule update --init` after a fresh clone.
- **Findings carry an executable `check:`** where possible; `manual` (needs the
  board) or `expensive` otherwise. Keep `confidence` and `scope` honest.
- **Frontmatter is the query surface.** Cross-link files by id/relative path so the
  KB reads as a lineage: source → experiment → finding.
- **Nothing here is compiled into the firmware.** The domain (`../domain`) never
  reads this tree; the firmware boundary (`../firmware`) only *mirrors* what the
  hardware sources teach. Dependencies still point inward.

## Frontmatter link vocabulary

Back-links are the query surface — keep to these keys per layer so the lineage
stays greppable and no file invents a synonym:

| Layer | Keys | Points at |
|-------|------|-----------|
| `source` | `seeds:` | experiment / finding ids this source seeded |
| `experiment` | `source:`, `findings:` | source ids probed · finding ids fed |
| `finding` | `derived-from:`, `supersedes:` | experiment ids that earn it · finding ids it replaces |
| `guide` | `distils:` | source / experiment ids it synthesizes |

## Verify

The KB checks itself. From the repo root:

```sh
just kb            # or, from anywhere: just -f kb/Justfile verify
```

Three gates, all pure-read and CI-safe: **links** (every relative `.md` cross-link
resolves; every file is reachable from `INDEX.md`), **checks** (each finding's
executable `check:`; `manual` / `expensive` are skipped and counted), and
**scripts** (`bash -n` on every `fetch.sh` + experiment script). Run one alone with
`just kb links` / `checks` / `scripts`. It assumes submodules are present
(`git submodule update --init`) — a few checks grep evidence out of the pinned
sources, and a missing one honestly reads as a FAIL.

Start at [INDEX.md](INDEX.md).
