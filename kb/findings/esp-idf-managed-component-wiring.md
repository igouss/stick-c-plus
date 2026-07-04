---
id: esp-idf-managed-component-wiring
title: "Adding an ESP-IDF managed component (e.g. espressif/mdns) needs ESP_IDF_SYS_ROOT_CRATE in a virtual workspace"
confidence: high
scope: project:stick-c-plus firmware (std/ESP-IDF, esp-idf-sys 0.37)
derived-from: []
supersedes: []
reviewed: 2026-07-04
check: manual   # `cargo build -p plant-monitor`; grep target/.../managed_components for the component
---

**Claim:** Some `esp-idf-svc` services (mDNS, and anything else the IDF moved out of
its core tree in 5.0) are gated behind an **ESP-IDF managed component** that is not
in the tree by default. `esp_idf_svc::mdns` is `#[cfg(any(esp_idf_comp_mdns_enabled,
esp_idf_comp_espressif__mdns_enabled))]`, so `use esp_idf_svc::mdns::EspMdns` fails
with `could not find 'mdns' in 'esp_idf_svc'` until the `espressif/mdns` component is
pulled into the build. You pull it via **two** Cargo settings — and in *this* repo
the second one is mandatory, not optional.

**The recipe (both parts required here):**

1. Declare the component on the crate that uses it (`firmware-infra`, a direct dep of
   the `plant-monitor` binary). esp-idf-sys synthesizes the `idf_component.yml`,
   downloads it into `target/…/esp-idf-sys-*/out/managed_components/`, and links it:
   ```toml
   # firmware/firmware-infra/Cargo.toml
   [[package.metadata.esp-idf-sys.extra_components]]
   remote_component = { name = "espressif/mdns", version = "1.8.2" }
   ```
   The component's `idf_component.yml` declares `idf: ">=5.0"`, so any 1.x works on our
   pinned v5.3.3 (latest is 1.11.x; 1.8.2 is the tested pin). The `MDNS_*` Kconfig
   symbols (`CONFIG_MDNS_MAX_SERVICES`, default 10) are provided *by the component* —
   inert in `sdkconfig.defaults` until it is present.

2. **Name the root crate.** esp-idf-sys reads `extra_components` from
   `cargo_metadata.root_package()` **and its direct deps**. Our `firmware/Cargo.toml`
   is a **virtual workspace** (a `members` list, no root `[package]`), so
   `root_package()` is `None`; esp-idf-sys then `bail!`s *"could not identify the root
   crate"* — but the caller wraps it in `.into_warning()`, so it is **silently
   swallowed** and no crate's `extra_components` are read. The component never gets
   added and the build fails only later, at the Rust `use`. Fix it in the env:
   ```toml
   # firmware/.cargo/config.toml  →  [env]
   ESP_IDF_SYS_ROOT_CRATE = "plant-monitor"
   ```

**Evidence:** esp-idf-sys 0.37.2 `build/config.rs:115`
(`match (metadata.root_package(), &self.esp_idf_sys_root_crate)` → `(None, None) =>
bail!(…)`) and `build/native/cargo_driver/config.rs:240-324` (reads root **and** each
direct dependency's `package.metadata.esp-idf-sys.extra_components`). Confirmed
on-device 2026-07-04 (qhw.8): after setting the env var the generated
`out/managed_components/espressif__mdns/` appears, `esp_idf_svc::mdns` compiles, and
the board advertises `_esphomelib._tcp:6053` (`avahi-browse -rt _esphomelib._tcp`).

**Gotchas:**
- Changing `extra_components` on a dependent crate does **not** invalidate the
  `esp-idf-sys` build-script fingerprint, so a warm rebuild silently keeps the old
  (component-less) build. Force a re-resolve: remove
  `target/…/<profile>/.fingerprint/esp-idf-sys-*` (light — keeps compiled objects,
  CMake adds just the new component incrementally) or `cargo clean` (heavy).
- The `/` in a component name becomes `__` in the cfg: `espressif/mdns` →
  `esp_idf_comp_espressif__mdns_enabled`.
- A hand-written `idf_component.yml` in the crate root is **not** auto-discovered by
  esp-idf-sys — only the `extra_components` metadata route works.

See [[rust-on-esp-idf]] for the pinned toolchain.
