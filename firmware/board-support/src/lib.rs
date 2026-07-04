//! # board-support
//!
//! M5StickC Plus board-support package (BSP): AXP192 PMIC power-on, the pin map,
//! and peripheral bring-up shared by all three projects (plant-monitor,
//! led-driver, rover).
//!
//! Skeleton only (qhw.2 workspace carve): the AXP192 power-on that enables the
//! LCD/TFT rails lands in qhw.20, and the pin map + peripheral bring-up follow.
//! This crate exists now so the workspace seam and its hex-arch `infra` role are
//! in place for those beads to fill.
