# Requirement — USB power-transition chime (spool-up / spool-down), every app

**Status:** requirement, ready to implement. Specifies *what*, not *how* — the seams and
constraints are load-bearing; the code is the implementer's.

## Intent

The board should tell you, by ear, when it changes power source. Plug the USB cable in and it
plays a **spool-up** buzzer sweep; pull the cable and it runs on battery and plays a
**spool-down** sweep. This is a board-level fact, not any one app's domain, so it must be a
**shared platform capability wired into every app** (plant-monitor, host-monitor, pomodoro,
led-driver) — not a feature of one.

## How the board knows (context, already researched — don't re-derive)

The M5StickC Plus 1.1 carries an **AXP192 PMIC** at I²C `0x34` on the internal bus (SDA G21,
SCL G22, 400 kHz — see `firmware/platform/board-support/src/lib.rs`). USB power arrives on the
PMIC's **VBUS** rail. The power-input status is **register `0x00`**; **bit 5 (mask `0x20`) is
`VBUS present`** — set ⇒ on USB, clear ⇒ on battery. This is deterministic; M5Unified reads
exactly this (`AXP192_Class::isVBUS() { return readRegister8(0x00) & 0x20; }`).

Do **not** infer power source from `getVBUSVoltage()` (reg `0x5A`) thresholding — it reads
~830 mV even unplugged, so it forces magic thresholds. Use the status bit.

The existing driver (`board-support/src/axp192.rs`) has the `read(reg)`/`write(reg,val)`
primitives and already enables all ADC channels at boot (reg `0x82`) but has **no reg-`0x00`
read** — that read is the one new board primitive this feature needs. (`isCharging` = bit 2 /
`0x04` exists on the same register but is *not* required here.)

## Behaviour

- On a **rising** VBUS edge (battery → USB): play the **spool-up** melody once.
- On a **falling** VBUS edge (USB → battery): play the **spool-down** melody once.
- At **startup**, sample VBUS once as the baseline and **stay silent** — a device booted on USB
  does not greet you; only a *change* after boot chimes. (Zero case: no edge ⇒ no sound.)
- Exactly **one** chime per physical transition (see debounce).

## Architectural placement (hexagonal — this is a requirement, not a suggestion)

Dependencies point inward; the domain stays framework-free and host-tested. Add these seams:

| Layer | Crate | What lands here |
|---|---|---|
| **Driven port** | `platform/platform-core` | A `PowerSource` port — a thin read of "is the board on USB?" (`-> Result<bool, Self::Error>`), sibling to `AudioIn`/`Tone`. Adapter owns the error type; the port names no hardware. |
| **Domain** | `platform/platform-core` | The spool-up/spool-down **note tables** and the pure edge→melody decision (`prev, now -> Option<melody>`). Pure, `no_std`, host-tested. Mirror `pomodoro_core::Jingle::notes()`. |
| **Runtime loop** | `platform/platform-runtime` | A `spawn_power_watch`-style background loop, generic over `PowerSource` + `Tone`: poll → debounce → detect edge → play the melody. `std` but board-agnostic, so it runs under `cargo test` with fakes and cross-compiles unchanged — same discipline as `spawn_display`/`spawn_input`. |
| **Board primitive** | `firmware/platform/board-support` | A safe `vbus_present()` on `axp192.rs` reading reg `0x00 & 0x20`. |
| **Driven adapter** | `firmware/platform/adapters` | An AXP192-backed `PowerSource` implementation over the shared I²C bus. |
| **Composition** | every app's `bin/src/main.rs` | Retain the AXP192 past power-on, give the watcher a `PowerSource`, hand it a `Tone`, spawn it. |

The buzzer output already exists and must be **reused, not reinvented**: the `platform_core::Tone`
port (`play(&[Note])`, `Note { freq_hz, ms }`, rest = 0 Hz) and its `LedcBuzzer` adapter on G2.

## Constraints

1. **The buzzer is a passive resonant transducer, not a speaker.** It is measured loud only
   across **~2–9 kHz** and radiates almost none of its energy at the driven frequency, so a
   listener distinguishes sounds by **rhythm and rising/falling contour, not pitch**. Therefore:
   spool-up = an ascending contour of notes within 2–9 kHz; spool-down = the descending
   mirror. Every tone must sit in 2–9 kHz (below the LEDC 13-bit ~9.7 kHz ceiling). This is the
   same reality `pomodoro_core::jingle.rs` already encodes and tests — follow it.
2. **Debounce.** A plug/unplug can chatter the VBUS bit for tens of ms. Exactly one chime per
   physical transition. Debounce window is the implementer's choice but must be proven by a host
   test (a bouncing sample sequence collapses to one edge).
3. **One buzzer, one owner.** The buzzer is a single hardware resource. In apps that already
   play sound (pomodoro jingles), power-chimes and app sounds must be **serialized through one
   owner** — a chime never interleaves with or truncates a jingle mid-note, and vice-versa. In
   apps with no other sound, the watcher may own the buzzer outright. Mechanism (channel/actor/
   mutex) is the implementer's choice.
4. **Retain the PMIC.** Today the app roots bring the AXP192 up in a scoped block and drop it
   after `power_on()` (rails latch, so that was fine). Runtime VBUS reads need the PMIC and its
   I²C device kept alive and shared (the bus is already wrapped in `embedded-hal-bus`
   `RefCellDevice`). Lift the bring-up out of the drop scope in each root.
5. **Sized thread, gentle cadence.** Size the watch thread's stack explicitly (this platform
   sizes its threads, it does not default them) — an I²C read plus buzzer PWM under preemption
   must not overflow it. Poll on a modest interval, never a busy-loop; it must not starve the
   render or input threads.
6. **House rules.** `no_std` where the layer allows; **no `unsafe`**; explicit type annotations
   incl. lambda params; each new file one responsibility; hex-arch role on every new crate/module.

## Acceptance criteria (Gherkin — tests all the way down)

Host tests carry the logic; complexity-1 tests, no loops, cover zero / one / many.

```gherkin
Feature: Chime on USB power transitions

  Scenario: Plugged in -> spool up
    Given the board was running on battery
    When VBUS becomes present
    Then the spool-up melody plays exactly once
    And no other melody plays

  Scenario: Unplugged -> spool down
    Given the board was running on USB
    When VBUS becomes absent
    Then the spool-down melody plays exactly once

  Scenario: Boot is silent (zero case)
    Given the board boots with USB already present
    When the watcher takes its first sample
    Then no melody plays

  Scenario: Bounce collapses to one chime (many -> one)
    Given the board was on battery
    When VBUS reads present, absent, present within the debounce window
    Then the spool-up melody plays exactly once

  Scenario Outline: Every tone is audible on this buzzer
    Given a power melody
    Then every note frequency lies within 2000..=9000 Hz

  Scenario: Up and down are opposite contours
    Given the spool-up and spool-down melodies
    Then spool-up ascends and spool-down descends
    And the two melodies are distinct
```

Property tests to prove the rules: the edge function emits a chime **iff** the boolean state
changed; it is idempotent across repeated equal samples; up-contour is strictly the reverse
shape of down-contour.

## Verification (not optional — a green host is not a working device)

- **Host:** `cargo test` on the workspace — the pure edge/melody domain and the watch loop
  (driven by a fake `PowerSource` and a recording fake `Tone`) both prove out off-metal.
- **Device, each app:** flash it, then **plug and unplug USB** and hear spool-up / spool-down.
  Confirm the chime is *audible* the way the chime self-test does — capture the PDM mic and
  assert acoustic **level** (`platform_audio::ac_rms` / `present`, DC-removed RMS above the
  silent floor), **not** frequency; the resonant buzzer won't reproduce the commanded pitch.
- **Watch the serial log** on each app for a clean boot with no reboot loop after wiring the
  retained PMIC + new thread.

## Out of scope (separate beads)

Battery percentage / telemetry, deep-sleep and wake-reason, ACIN, charge-state surfacing — see
`docs/plans/plant-monitor-battery-deep-sleep.md`. This bead is only the audible power-source
transition, shared across every app.
