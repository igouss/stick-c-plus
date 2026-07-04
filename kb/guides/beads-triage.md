---
id: beads-triage
title: "Beads issue tracking: br for the store, bv for triage"
kind: guide           # derived, prose; no single executable check
scope: project:stick-c-plus
reviewed: 2026-07-04
distils: []           # tool workflow, not a distilled source
---

This project tracks work in **beads** — a dependency graph of issues living in
`.beads/`, committed to git. Two command-line tools sit over that one graph:

| Tool | Role | Reads/writes | Output |
|------|------|--------------|--------|
| **`br`** ([gastownhall/beads](https://github.com/gastownhall/beads); Rust client wrapper: `~/code/beads-client`) | the store — create / update / close / query, and the git-facing JSONL | writes the SQLite db + the committed `issues.jsonl` | human text, or `--json` |
| **`bv`** (beads viewer) | triage — ranks the graph by PageRank / betweenness into recommendations | reads only | interactive TUI, or `--robot-*` JSON |

Division of labour: **`br` writes and lists** (for a human), **`bv` analyses and
recommends** (for an agent). `bv` never mutates anything.

## Non-interactive contract (this matters for agents)

Every command below is safe to run headless — it prints and exits, no pager, no
prompt, no TTY. The one trap is `bv`.

1. **Never run bare `bv`.** It launches a full-screen TUI that BLOCKS the terminal
   (and any agent session). Always pass a `--robot-*` flag; that makes it
   emit-and-exit. Pair it with **`-f json`** to pin the shape — robot mode defaults
   to JSON but honours `BV_OUTPUT_FORMAT` / `TOON_DEFAULT_FORMAT`, so an env default
   could otherwise flip it to TOON.
2. **`br` is non-interactive by default** — it never opens a pager or prompts, so
   `br ready` / `br show <id>` / `br update …` are all script-safe. Pass `--json`
   when a machine parses the output.
3. **`br` never runs git.** By design it only touches `.beads/`. Flushing the db
   to `issues.jsonl` and committing it is *your* job (`just bead-sync`, then a
   `beads:`-scoped commit). Import back into the db is automatic on the next `br`
   read; a merge after `git pull` is the exception (`br sync --merge`).

## The loop

```sh
just triage                 # bv: what should I work on? (recommendations + health)
br update <id> --status=in_progress   # claim it
# … do the work …
br close <id> --reason "Implemented in <sha>"
just bead-sync              # flush db → issues.jsonl, ready to commit
git add .beads/issues.jsonl && git commit -m "beads: close <id>, …"
```

## `just` recipes

The [justfile](../../justfile) wraps the common calls (`just --list` to see all):

| Recipe | Runs | For |
|--------|------|-----|
| `just ready` | `br ready` | open, unblocked, not-deferred work (human list) |
| `just blocked` | `br blocked` | what's blocked and on what |
| `just stats` | `br stats` | counts by status / type / priority |
| `just next` | `bv --robot-next` | the single top pick + its claim command |
| `just triage` | `bv --robot-triage` | the mega-command: recommendations + blockers + health |
| `just plan` | `bv --robot-plan` | parallel execution tracks (what can run concurrently) |
| `just bead-sync` | `br sync --flush-only` | flush db → `issues.jsonl` before committing |
| `just bead-check` | `br doctor` + cycle assert | graph-health gate (see below) |

## `br` essentials

```sh
br create "Title" -d "desc" -t task -p 2   # -t task|bug|feature|epic|chore|docs -p 0..4
br q "Quick title"                          # quick-capture, prints the id only
br show <id> [--json]                        # full detail + dependencies
br search "keyword"                          # full-text search
br comments add <id> "text"                  # append a comment — NOTE: `comments add`, not `br comment`
br dep add <child> <parent>                  # child depends on (is blocked by) parent
br dep tree <id>                             # visualise the subtree
br dep remove <child> <parent>               # break an edge (e.g. to kill a cycle)
```

Priority is a number, **0 = critical … 4 = backlog** (default 2), never a word.

Agents should pass `--json` to every query — the output is a stable, parseable
shape (below).

## `bv` triage (robot mode)

```sh
bv --robot-next   -f json             # single top pick + claim command
bv --robot-triage -f json             # recommendations + blockers + health
bv --robot-plan   -f json             # dependency-respecting parallel tracks
bv --robot-triage -f json --robot-by-label esphome-api   # scope to one domain
```

`-f json` is belt-and-braces (robot mode already defaults to JSON) but keeps the
shape deterministic regardless of `BV_OUTPUT_FORMAT` / `TOON_DEFAULT_FORMAT`.

The scores are graph metrics: **PageRank** = how much everything depends on this
(foundations rank high); **betweenness** = bottleneck (blocks many paths). High on
both ⇒ drop everything and do it. Slice the JSON with `jq`:

```sh
just triage | jq '.triage.recommendations[0]'   # the top recommendation
just triage | jq '.triage.project_health'        # counts + cycle/health summary
just next   | jq -r '.claim_command'             # just the claim command
```

## Graph health

`just bead-check` runs `br doctor` (db/JSONL integrity, sync state) and asserts
the graph is acyclic. A dependency **cycle** is a corrupt backlog — nothing in it
can ever become "ready". Break one edge with `br dep remove <child> <parent>`.

The cycle count comes from **`br dep cycles --json`** (`{"cycles":[],"count":0}`),
which is authoritative. Do *not* gate on `bv --robot-insights | jq '.Cycles'`:
that field is `null` until bv's Phase-2 metrics finish computing, so an empty
graph reads as `null ≠ []` — a **false red**. `br dep cycles` has no such lag.

## The JSON contract

`br --json` and `bv --robot-*` emit stable shapes agents can parse without
scraping text. `br` documents its own schemas:

```sh
br schema all          # bundle of every br output schema
br schema ready-issue   # e.g. the shape of a `br ready --json` row
```

The upstream reference set lives at
[`beads_rust/agent_baseline/schemas`](https://github.com/Dicklesworthstone/beads_rust/tree/main/agent_baseline/schemas)
(`schema_all.json`, `schema_issue_details.json`, `schema_error.json`,
`cli_schema.json`). `br schema` is explicitly *not* a stable API — pin behaviour
to a `br` version if you build tooling on it.

## Where the state lives

```
.beads/
  beads.db          SQLite — the working store (gitignored)
  issues.jsonl      the git-tracked export — the source of truth in git
  config.yaml       issue prefix (stick-c-plus), default priority/type
```

Only `issues.jsonl` (and `config.yaml`, `metadata.json`) are committed; the db and
its lock/WAL files are gitignored and rebuilt from the JSONL on demand.
