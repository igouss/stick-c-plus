---
id: claude-desktop-buddy-domain-model
title: "claude-desktop-buddy — the behaviour and wire model, read out of the C++ with file:line"
type: repo
author: Anthropic
publisher: anthropics (GitHub)
url: https://github.com/anthropics/claude-desktop-buddy
retrieved: 2026-07-22
license: see ../../../claude-desktop-buddy/LICENSE
material: /home/elendal/code/m5/claude-desktop-buddy   # sibling clone, not vendored here
seeds: [stick-c-plus-buddy-domain-4n5, stick-c-plus-buddy-display-214, stick-c-plus-buddy-permission-flow-es6]
---

## Citation

`anthropics/claude-desktop-buddy` — a ~3 kLOC Arduino/C++ desk pet targeting the
M5StickC Plus, the board this repo is built for. `src/main.cpp` is the state machine;
`REFERENCE.md` is the protocol spec. Ported to Rust here under epic
`stick-c-plus-claude-buddy-e3k`.

## Why this note exists

The port is specified by the C++, not by `REFERENCE.md` — the two disagree in thirteen
places, catalogued below. This note is the extracted model so the next reader does not
re-derive it from Arduino sources. Every claim carries a `file:line`. All references are
relative to `/home/elendal/code/m5/claude-desktop-buddy/src/`.

**Read this before `REFERENCE.md`.** Where they conflict, the code won.

> **Scope banner (2026-07-22).** What the port takes from upstream is **behaviour** —
> the persona FSM (§1), the sensors (§2), the stats math (§3), the clock schedule (§4),
> and the wire protocol (§5–6). Those sections are authoritative. What it does **not**
> take is the **rendering**: §7's 795 monospace text-art poses are recorded as a faithful
> reading of upstream, **not** as the render path. The buddy draws the owner's existing
> **ClaudePix 20×20 pixel sprites** (`platform-display/src/sprite/`), one creature across
> the seven states, with more species as pixel art under a later bead. Treat §7 as history
> and a personality reference, never as a transcode source. The old "795 poses / ~42 KiB
> rodata / `[[u8;12];5]`" plan is void.

---

## 1. Persona state machine

`PersonaState`, `main.cpp:32`. **Ordinals are load-bearing** — `Species::states[7]`
(`buddy.h:28`) is indexed by them:

```
0 P_SLEEP  1 P_IDLE  2 P_BUSY  3 P_ATTENTION  4 P_CELEBRATE  5 P_DIZZY  6 P_HEART
```

### `derive()` — base state, top-down, first match wins (`main.cpp:479-485`)

```
if (!connected)          return P_IDLE;       // :480  NOT sleep
if (sessionsWaiting > 0) return P_ATTENTION;  // :481
if (recentlyCompleted)   return P_CELEBRATE;  // :482
if (sessionsRunning >= 3) return P_BUSY;      // :483  three, not one
return P_IDLE;                                // :484
```

Pure over the snapshot; no time, no I/O. Called once per loop at `main.cpp:996`.
`recentlyCompleted` outranks `sessionsRunning`, so completed-and-busy renders celebrate.

Both README divergences the epic already tracks live here: **disconnected → idle**
(`:480`) and **busy needs ≥ 3** (`:483`).

Feeding inputs (`data.h`): `connected` = `_lastLiveMs != 0 && (millis()-_lastLiveMs) <= 30000`
(`:50-52`). On `!connected`, `dataPoll` zeroes the three session counts (`:178-183`).
`recentlyCompleted` is **not sticky** — `doc["completed"] | false` (`:96`) resets it on
every packet lacking the key.

### One-shot override layer (`main.cpp:487-490`)

`triggerOneShot(state, ms)` sets `activeState` and `oneShotUntil = millis() + ms`.
Resolution at `main.cpp:1002`: `if ((int32_t)(now - oneShotUntil) >= 0) activeState = baseState;`
— signed-wraparound comparison, not a raw `<`. **While a one-shot is live, `baseState` is
ignored entirely**; on expiry `activeState` snaps to the *current* `baseState`, with no
saved-prior restore.

| Trigger | State | Duration | Site | Preempts a live one-shot? |
|---|---|---|---|---|
| level-up | celebrate | 3000 ms | `main.cpp:995` | yes |
| shake | dizzy | 2000 ms | `main.cpp:1016` | **no** — guarded by `now >= oneShotUntil` (`:1014`) |
| approval answered in < 5 s | heart | 2000 ms | `main.cpp:1087` | yes |

Heart fires on **approve only**; deny (`:1109-1118`) triggers nothing. `tookS` is integer
seconds, so the heart window is strictly `{0,1,2,3,4}`.

Ordering within one loop iteration matters: level-up (`:995`) → `derive` (`:996`) →
wake-window rewrite (`:1000`) → one-shot resolution (`:1002`) → shake (`:1012`) → buttons,
heart (`:1087`).

### Two routes into sleep, not one

1. **Wake-transition window** (`main.cpp:1000`): `if (baseState == P_IDLE && now < wakeTransitionUntil) baseState = P_SLEEP;`
   Only `P_IDLE` is rewritten — attention, celebrate and busy pass through. The window is
   armed **only on a screen-off→on transition** (`main.cpp:105`, 12000 ms), not on every
   `wake()` call.
2. **The charging clock** writes `activeState = P_SLEEP` **directly** (`main.cpp:1174-1180`),
   bypassing `baseState` *and* the one-shot resolution above it.

An earlier draft of the domain bead claimed route 1 was the only one. It is not.

---

## 2. Sensors

### Shake (`main.cpp:492-499`)

```c
mag   = sqrtf(ax*ax + ay*ay + az*az);      // L2 norm in g
delta = fabsf(mag - accelBaseline);        // against the PRE-update baseline
accelBaseline = accelBaseline*0.95f + mag*0.05f;
return delta > 0.8f;                       // strict
```

`accelBaseline` seeds to `1.0f` (`main.cpp:40`) so a cold start under gravity does not
false-trigger. Sampled at **50 ms** (`:1012`), not per loop, giving the EMA a ~1 s time
constant.

**The baseline goes stale while the menu is open or the screen is off.** C++ `&&`
short-circuits `checkShake()` out of `!menuOpen && !screenOff && checkShake() && ...`
(`:1014`), so it is never called and never updates. The first sample after closing the
menu behaves differently. This is a quirk to preserve, not a bug to fix.

### Face-down nap

Predicate (`main.cpp:91-95`): `az < -0.7f && fabsf(ax) < 0.4f && fabsf(ay) < 0.4f`, all strict.

Counter (`main.cpp:1236-1241`): ±1 per loop iteration, saturating at **+20 / −10**, and
**frozen entirely while `inPrompt`** — the IMU is not even read.

Transitions (`main.cpp:1243-1253`): enter nap at **≥ 15**, leave at **≤ −8**. On leaving:
`statsOnNapEnd`, `statsOnWake`, `wake()`.

**The freeze applies to the counter, not the state machine** — the transition check sits
*outside* the `!inPrompt` guard, so a nap already latched stays latched through a prompt.

`inPrompt` = `promptId[0] && !responseSent` (`:1041`) — a prompt that is up *and
unanswered*. Answering it resumes the counter even though the panel is still drawn.

Loop cadence `delay(screenOff ? 100 : 16)` (`:1264`), so ≈240 ms to nap from zero and
≈368 ms to wake from +20.

---

## 3. Stats (`stats.h`)

`TOKENS_PER_LEVEL = 50000` (`:12`). `velocity` is a ring of 8 `uint16_t`.

**Median velocity** (`:127-139`): 0 if `velCount == 0`; else insertion-sort the first
`velCount` of 8 slots and return `tmp[velCount/2]` — the **upper** median for even counts.

**Mood tier 0..4** (`:142-158`):

```
vel == 0 -> 2 (neutral, no data);  <15 -> 4;  <30 -> 3;  <60 -> 2;  <120 -> 1;  else 0
if (approvals + denials >= 3) {
    if (denials > approvals)      tier -= 2;
    else if (denials*2 > approvals) tier -= 1;   // deny rate > 33%
}
clamp low at 0
```

**Energy tier 0..5** (`:161-171`): `energy = _energyAtNap - (hoursSince/2)`, clamped, with
`hoursSince = (millis() - _lastNapEndMs)/3600000`. Both divisions integer, so the step
lands at exactly 2 h, 4 h, 6 h. `statsOnWake` (sets `_energyAtNap = 5`) is called **only**
from the nap-exit branch (`main.cpp:1251`) — nothing else refills energy. Neither field is
persisted: a reboot resets energy to 3 and decays it from uptime.

**Level / fed** — level is `tokens / 50000`; `statsFedProgress()` = `(tokens % 50000)/5000`
(`:173-175`), range 0..9, never 10. Load-time backfill (`:43-45`): if `tokens == 0 && level > 0`,
set `tokens = level * 50000`.

### Token-delta latching — three cases (`:79-109`)

```c
if (!_tokensSynced)                { _lastBridgeTokens = bridgeTotal; _tokensSynced = true; return; }  // boot
if (bridgeTotal < _lastBridgeTokens) { _lastBridgeTokens = bridgeTotal; return; }                      // bridge restart
delta = bridgeTotal - _lastBridgeTokens; _lastBridgeTokens = bridgeTotal;                              // normal
```

`_tokensSynced` / `_lastBridgeTokens` are **RAM-only** (`:75-76`). Case 1 is exactly what
stops a device reboot re-crediting a whole desktop session; case 2 is the mirror defence
against a bridge restart. `tokens` accumulate in RAM and flush to NVS **only on a level-up
milestone** (`:101-103`), so a hard power-off can lose up to 50 000 tokens.

Called from `data.h:97-98`, and only if `doc["tokens"].is<uint32_t>()`.

---

## 4. Charging-clock mood schedule

**Gate** (`main.cpp:1150-1153`), all required: `DISP_NORMAL`, no menu/settings/reset
overlay, `!inPrompt`, `sessionsRunning == 0`, `sessionsWaiting == 0`, `dataRtcValid()`,
and `_onUsb` (`GetVBusVoltage() > 4.0f`, sampled at 1 Hz, `:359`).

**Schedule** (`main.cpp:1168-1181`), first match wins. `h = Hours`, `dow = WeekDay % 7`
(0 = Sunday), `weekend = dow==0||dow==6`, `friday = dow==5`:

```
h >= 1 && h < 7   -> Sleep                                     (unconditional)
weekend           -> (now/8000  % 6 == 0) ? Heart     : Sleep
h < 9             -> (now/6000  % 4 == 0) ? Idle      : Sleep
h == 12           -> (now/5000  % 3 == 0) ? Heart     : Idle
friday && h >= 15 -> (now/4000  % 3 == 0) ? Celebrate : Idle
h >= 22 || h == 0 -> (now/7000  % 3 == 0) ? Dizzy     : Sleep
else              -> (now/10000 % 5 == 0) ? Sleep     : Idle
```

**Not a function of (hour, weekday) alone** — every branch but the first also reads
`millis()`, giving a deterministic two-state flicker where the "special" state occupies
one slice of `N` ms out of every `N·M` ms. The signature is `f(hour, dow, now_ms)`.

**`h == 0` is dead code**: hour 0 fails `h>=1 && h<7`, then matches `h < 9`. Dizzy is
reachable only at 22:00–23:59, and midnight renders the pre-9am flicker.

Because the write is `activeState = …` and it happens *after* the one-shot resolution at
`:1002`, a live one-shot is visually suppressed while the clock is up, though
`oneShotUntil` still runs down underneath.

---

## 5. Wire protocol (NUS)

| | |
|---|---|
| Service | `6e400001-b5a3-f393-e0a9-e50e24dcca9e` (`ble_bridge.cpp:13`) |
| RX, central→device, write | `6e400002-…` (`:14`) |
| TX, device→central, notify | `6e400003-…` (`:15`) |
| Advertised name | `Claude-%02X%02X` from BT MAC bytes 4,5 (`main.cpp:13,17-18`) |
| MTU | requests 517 (`:92`), resets to 23 on disconnect (`:58`) |
| Security | `SEC_ENCRYPT_MITM`, `SC_MITM_BOND`, `IO_CAP_OUT`, key 16 (`:94,121-125`) |

### Framing

Two parallel decoders — USB serial and BLE — with different code but identical rules
(`data.h:129-143` and the hand-rolled `data.h:162-175`):

1. Terminator is `\n` **or** `\r` (`:136`, `:166`). A CRLF pair yields one line plus one empty.
2. Empty lines dropped silently (`:137`, `:167`).
3. **`line[0]` must be `{`** (`:137`, `:169`) — no trimming, so a leading space kills the message.
4. Buffer 1024, usable 1023 (`:138`, `:172`).
5. **Overflow drops the tail without resetting the buffer.** Framing resumes at the next
   terminator, so `_applyJson` receives a truncated 1023-byte prefix, which fails
   `deserializeJson` and vanishes with zero diagnostics (`:72`).
6. Upstream of framing, a 2048-byte BLE ring (`ble_bridge.cpp:20-23`); `rxPush` on full
   does a bare `return` (`:33-40`), **dropping the remainder of a GATT write mid-line**.

### Heartbeat merge — asymmetric

`_applyJson` (`data.h:70-127`) dispatches: parse fail → return; `xferCommand` handled →
return; `time` array → RTC sync, return; **otherwise** snapshot merge.

| Field | Absent behaviour | Cap |
|---|---|---|
| `total`, `running`, `waiting` | **keeps prior** (`:93-95`) | `uint8_t` |
| `completed` | **resets to false** (`:96`) | `bool` |
| `tokens` | no-op; forwarded to the latch only if `uint32_t` (`:97-98`) | — |
| `tokens_today` | keeps prior (`:99`) | `uint32_t` |
| `msg` | keeps prior (`:100-101`) | `char[24]` |
| `entries` | keeps prior (`:102-115`) | max 8, `char[92]` each |
| `prompt` | **CLEARS id/tool/hint** (`:116-124`) | id 40, tool 20, hint 44 |

Only `completed` and `prompt` are destructive-on-absent.

**Time sync**: accepted only as a top-level array of **exactly 2** elements,
`{"time":[epoch, tz_offset_seconds]}` (`data.h:77-91`). The offset is folded into the
epoch and decoded as UTC. Wrong arity falls through to the snapshot branch — **and
therefore clears the prompt**.

### Commands (`xfer.h:77-235`)

`name`, `species` (idx, `0xFF` = use GIF), `unpair`, `owner`, `status`, `char_begin`,
`file`, `chunk`, `file_end`, `char_end`. `permission` is device→central only.

Acks are `{"ack":"<what>","ok":<bool>,"n":<u32>}\n` (`xfer.h:17-22`) — **`n` is always
present**, contrary to `REFERENCE.md`'s examples.

Permission responses (`main.cpp:1080-1082`, `:1113-1115`) are
`{"cmd":"permission","id":"<promptId>","decision":"once"|"deny"}`. `sendCmd` (`:115-119`)
writes the JSON then a **separate 1-byte `\n`** — two GATT notifications per logical line.

`bleWrite` chunks at `min(mtu-3, 180)` with a blocking `delay(4)` between chunks
(`ble_bridge.cpp:162-180`), and **returns 0 silently when disconnected; no caller checks**.

---

## 6. The thirteen code↔`REFERENCE.md` divergences

Ordered by consequence. The first three are why the port fixes rather than reproduces.

| # | Divergence |
|---|---|
| D1 🔴 | **`{"evt":"turn"}` has no handler and DESTROYS a pending prompt.** Spec'd at `REFERENCE.md:82-94`; no `evt` branch exists, so it falls into the snapshot merge and `data.h:122-124` wipes the prompt. A transcript event arriving mid-decision clears the approval off the glass. |
| D2 🔴 | **4 KB events cannot fit a 1 KB buffer.** `REFERENCE.md:86` drops only above 4 KB; the line buffer is 1023 usable and the BLE ring 2047. |
| D3 🔴 | **Unknown commands get no ack.** `REFERENCE.md:124` promises one. `xfer.h:188` returns `true` (swallow); `xfer.h:234` returns `false` *and* lets the snapshot merge clobber the prompt. |
| D4 🟠 | `completed` is undocumented, and uniquely resets-on-absent. |
| D5 🟠 | `{"cmd":"species","idx":N}` is fully implemented, absent from the doc's table. |
| D6 🟠 | Status ack carries `data.owner`, `sys.fsFree`, `sys.fsTotal`, none listed. |
| D7 🟠 | `entries` ordering: doc says newest-first (`:68`); `data.h:111` compares `lines[n-1]` against `msg`, which only works oldest-first. **We write the bridge — pick oldest-first and document it.** |
| D8 🟡 | Acks always include `n`. |
| D9 🟡 | `\r` accepted as a terminator; doc says `\n` only. |
| D10 🟡 | The leading-`{` requirement is undocumented. |
| D11 🟡 | `chunk` ack `n` is per-file, reset at each `file`; cumulative is tracked and never sent. |
| D12 🟡 | `file.path` traversal is unvalidated despite `REFERENCE.md:215-218` saying to validate. |
| D13 🟡 | Permission responses are not gated on prompt state, and the device never acks its own decision. |

---

## 7. Species and sprites

`struct Species { const char* name; uint16_t bodyColor; StateFn states[7]; }` (`buddy.h:25-29`).

**`bodyColor` is dead** — `grep -rn bodyColor src/` yields exactly one hit, the
declaration. Every species passes the same literal as an argument at seven call sites
(`cat.cpp` repeats `0xC2A6` seven times, duplicating `CAT_SPECIES.bodyColor` at `:210`).

**Poses**: `static const char* const NAME[5]` — **exactly 5 lines, 795 arrays, zero
exceptions**. Width is 12 chars in 3914 of 3975 lines; **61 outliers** span 10–15 chars.
Nothing enforces it — `buddyPrintLine` (`buddy.cpp:42-53`) just `strlen`s and centres, so
a wide line silently shifts the art.

**Counts are uniform across all 18 species**: sleep 6, idle 10, busy 6, attention 6,
celebrate 6, dizzy 5, heart 5 (6 for axolotl, chonk, octopus) = 44 or 45 per species,
**795 total**.

**Sequencer**: global tick `TICK_MS = 200` (`buddy.cpp:104`); pose = `SEQ[(t/divisor) % len]`
with divisor **5**, except celebrate **3** and dizzy **4** (90 / 18 / 18 states
respectively). All 54 offset tables (`Y_SHIFT` celebrate, `X_SHIFT` dizzy, `Y_BOB` heart)
were verified to match their sibling `SEQ` length exactly, with every `SEQ` value in range
— zero out-of-bounds across 18 species.

Per-file cycle-time comments (`cat.cpp:10` "~12s" etc.) are **stale by ~2×**, consistent
with a `TICK_MS` change from 100 to 200 that never reached the comments. Do not port them.

**Registry order is the persisted NVS `species` value** (`buddy.cpp:72-98`): capybara,
duck, goose, blob, cat, dragon, octopus, owl, penguin, turtle, snail, ghost, axolotl,
cactus, robot, rabbit, mushroom, chonk. Sentinel `0xFF` = use the installed GIF.

**rodata ≈ 42.3 KiB** — analytic, per-TU pooling, ±8 %. *Not* a measured `size` figure;
PlatformIO is not installed here, so nothing was built to verify it. The epic's earlier
68 KB was an estimate made before anyone counted.

Geometry and palette: `buddy.cpp:12-29`. `X_CENTER 67`, `CANVAS_W 135`, `Y_BASE 30`,
`Y_OVERLAY 6`, `CHAR_W 6`, `CHAR_H 8`. Palette words are raw TFT_eSPI RGB565 in the
standard 5:6:5 layout — decompose to channels, **never swap**; see
[st7789-wants-rgb-colour-order](../findings/st7789-wants-rgb-colour-order.md).

Overlay particles are procedural code, not data — six archetypes with parameters
catalogued in `stick-c-plus-buddy-display-214`.

---

## What was NOT verified

- The 42 KiB rodata figure is analytic. No artifact was built.
- `character.cpp` (the GIF/LittleFS pet system) was read only far enough to confirm it is
  orthogonal to `buddies/` and out of scope.
- Nothing here was executed. This is a reading of the source, not an experiment; the
  hardware claims in the epic (BLE heap, bond persistence) come from `68280fd` and are
  measured, but everything in *this* note is static analysis.
