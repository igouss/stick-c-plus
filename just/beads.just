# Issue tracking over the dependency graph in .beads/.
#
# Imported by the root justfile; the variables and `require`d tools it uses are declared there.

# ---- Beads (issue tracking) ----
#
# Two tools over one dependency graph in .beads/. `br` (github.com/gastownhall/beads)
# is the CRUD + git-facing store: a SQLite db mirrored to the committed
# .beads/issues.jsonl. `bv` reads that graph and ranks it (PageRank / betweenness)
# into robot-JSON triage. Division of labour: br writes & lists, bv analyses &
# recommends. br is non-invasive: it never runs git, so flushing the db to JSONL and
# committing it is our job — `just bead-sync`, then a `beads:`-scoped commit.
#
# NON-INTERACTIVE CONTRACT (every recipe here is safe to script / run headless):
#   - `br` prints and exits — no pager, no prompt, no TTY needed. `--json` gives a
#     stable machine shape; the human recipes below rely on the plain-text output.
#   - `bv` is a TUI FIRST: bare `bv` opens a full-screen viewer that BLOCKS the
#     terminal (and any agent session). So `bv` is invoked ONLY with a `--robot-*`
#     flag, which makes it emit-and-exit. NEVER script bare `bv`.
#   - `-f json` PINS the output format: `bv`'s robot mode defaults to JSON but honours
#     BV_OUTPUT_FORMAT / TOON_DEFAULT_FORMAT, so pinning it keeps the shape
#     deterministic no matter the environment. Robot-JSON conforms to the schemas
#     `br schema <target>` emits.
#   - No recipe here carries `[confirm]` — a prompt would break the contract.
# Full workflow: kb/guides/beads-triage.md.

# Ready work: open, unblocked, not deferred (br, human list — prints and exits).
[group('beads')]
ready:
    {{ br }} ready
# Blocked work and what each waits on (br, human list — prints and exits).
[group('beads')]
blocked:
    {{ br }} blocked
# Project counts by status / type / priority (br — prints and exits).
[group('beads')]
stats:
    {{ br }} stats
# Top pick + its `br update … --status=in_progress` claim command (bv robot JSON).
[group('beads')]
next:
    {{ bv }} --robot-next -f json
# Full triage — recommendations + blockers + graph health (bv, the mega-command).
[group('beads')]
triage:
    {{ bv }} --robot-triage -f json
# Parallel execution tracks: what can run concurrently right now (bv robot JSON).
[group('beads')]
plan:
    {{ bv }} --robot-plan -f json
# Flush the db → the committed .beads/issues.jsonl so beads changes can land.
# br is non-invasive (never runs git), so stage + commit the JSONL yourself —
# the recipe prints the exact command.
[doc('flush the beads db to the committed .beads/issues.jsonl')]
[group('beads')]
bead-sync:
    {{ br }} sync --flush-only
    @echo "flushed → .beads/issues.jsonl  ·  commit: git add .beads/issues.jsonl && git commit -m 'beads: …'"
# Graph-health gate. bv's insight metrics are lazily computed and report
# Cycles:null when uncomputed (a false red), so the cycle count comes from
# `br dep cycles` instead. Runs br diagnostics + asserts zero cycles; else fails.
[doc('beads graph-health gate: br doctor + zero dependency cycles')]
[group('beads')]
[no-exit-message]
[script]
bead-check:
    {{ br }} doctor
    if {{ br }} dep cycles --json | {{ jq }} -e '.count == 0' >/dev/null; then
      echo "✅ beads: no dependency cycles"
    else
      {{ br }} dep cycles
      echo "❌ beads: dependency cycle(s) — break one edge with: br dep remove <child> <parent>"
      exit 1
    fi
