#!/usr/bin/env bash
# Re-run the panel colour-order measurement (see README.md).
#
# Flashes `display-colour-check`, a bench bin that paints RED / GREEN / BLUE bands
# through the PRODUCTION display init (adapters::st7789::St7789Display::new), each
# labelled in white. Look at the glass; the bands either match their labels or they
# do not. No instrument required, and no way to fool yourself.
#
# Rig: M5StickC Plus on /dev/ttyUSB0 (FT232, 115200 — see
# ../../guides/flashing-and-serial-access.md).
#
# Green is invariant under a red/blue swap and inverts to magenta under a wrong
# ColorInversion, so one glance separates the two failure modes.
#
# Afterwards, restore the monitor:  just run
set -euo pipefail
cd "$(dirname "$0")/../../.."

exec just run-bin display-colour-check
