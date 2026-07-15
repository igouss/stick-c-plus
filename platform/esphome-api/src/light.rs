//! A controllable **Light** entity — the first read-write [`ApiEntity`], proving
//! the entity model generalises past the read-only [`SensorEntity`].
//!
//! [`SensorEntity`]: crate::SensorEntity
//!
//! ## What HA sees
//! An RGB light with a master brightness and a named effect list. Home Assistant
//! sends a `LightCommandRequest` — each field guarded by a `has_*` flag, so a
//! command that only changes brightness leaves colour and effect untouched — and
//! the entity folds it into its state and reports the new `LightStateResponse`.
//!
//! ## Mapping to led-core
//! The light's job is to drive the WS2812 strip through [`led-core`]'s effects.
//! [`LightEntity::led_render`] (behind the `light` feature) turns the current
//! HA-facing state into a [`LedRender`] directive — off, a brightness-scaled
//! solid colour, or the Rainbow effect at this brightness — which the LED-driver
//! bin feeds to its render loop. The feature keeps the plant-monitor build,
//! which wants only the [`SensorEntity`], from pulling in led-core at all.
//!
//! [`led-core`]: https://docs.rs/led-core

use crate::entity::{ApiEntity, CommandMessage, ListMessage, StateMessage};
use crate::{object_id_key, proto};

/// The effect name for "no effect" — a plain solid colour. Home Assistant's
/// ESPHome light convention; selecting it renders the commanded RGB.
pub const NO_EFFECT: &str = "None";

/// The effect name that maps to led-core's [`Rainbow`](led_core::Rainbow).
pub const RAINBOW_EFFECT: &str = "Rainbow";

/// A colour with each channel in `0.0..=1.0`, the native-API light wire form.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Rgb01 {
    red: f32,
    green: f32,
    blue: f32,
}

impl Rgb01 {
    /// Full-white — the default before HA sends a colour.
    const WHITE: Rgb01 = Rgb01 {
        red: 1.0,
        green: 1.0,
        blue: 1.0,
    };
}

/// The immutable configuration of a [`LightEntity`].
#[derive(Clone, Debug)]
pub struct LightConfig {
    /// The stable identifier; its FNV-1a hash becomes the entity key.
    pub object_id: String,
    /// The friendly name shown in HA.
    pub name: String,
    /// The effect names HA offers, e.g. `["None", "Rainbow"]`. The names are the
    /// contract [`LightEntity::led_render`] maps to led-core effects.
    pub effects: Vec<String>,
}

impl LightConfig {
    /// The plant-monitor / NightDriver default: a light offering the solid
    /// ([`NO_EFFECT`]) and [`RAINBOW_EFFECT`] effects.
    pub fn nightdriver(object_id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            object_id: object_id.into(),
            name: name.into(),
            effects: vec![NO_EFFECT.to_string(), RAINBOW_EFFECT.to_string()],
        }
    }
}

/// A read-write RGB light entity with brightness and effect selection.
///
/// State is HA-native: `on`, `brightness` and each RGB channel in `0.0..=1.0`,
/// and the current effect name. [`LightEntity::led_render`] converts it to
/// led-core terms.
pub struct LightEntity {
    key: u32,
    config: LightConfig,
    on: bool,
    brightness: f32,
    rgb: Rgb01,
    effect: String,
}

impl LightEntity {
    /// Build a light from its config, deriving the key from the object_id. It
    /// starts off, full brightness, white, no effect.
    pub fn new(config: LightConfig) -> Self {
        Self {
            key: object_id_key(&config.object_id),
            config,
            on: false,
            brightness: 1.0,
            rgb: Rgb01::WHITE,
            effect: NO_EFFECT.to_string(),
        }
    }

    /// Whether the light is on.
    pub fn is_on(&self) -> bool {
        self.on
    }

    /// The master brightness, `0.0..=1.0`.
    pub fn brightness(&self) -> f32 {
        self.brightness
    }

    /// The selected effect name.
    pub fn effect(&self) -> &str {
        &self.effect
    }
}

impl ApiEntity for LightEntity {
    fn key(&self) -> u32 {
        self.key
    }

    fn object_id(&self) -> &str {
        &self.config.object_id
    }

    fn list_message(&self) -> ListMessage {
        ListMessage::Light(proto::ListEntitiesLightResponse {
            object_id: self.config.object_id.clone(),
            key: self.key,
            name: self.config.name.clone(),
            // A WS2812 strip is an RGB light; brightness rides on the RGB mode.
            supported_color_modes: vec![proto::ColorMode::Rgb as i32],
            effects: self.config.effects.clone(),
            ..Default::default()
        })
    }

    fn state_message(&self) -> StateMessage {
        StateMessage::Light(proto::LightStateResponse {
            key: self.key,
            state: self.on,
            brightness: self.brightness,
            color_mode: proto::ColorMode::Rgb as i32,
            color_brightness: 1.0,
            red: self.rgb.red,
            green: self.rgb.green,
            blue: self.rgb.blue,
            effect: self.effect.clone(),
            ..Default::default()
        })
    }

    /// Fold a `LightCommandRequest` into the state and report the result.
    ///
    /// Each field applies only when its `has_*` flag is set, so HA can change
    /// one attribute without disturbing the others. A command that is not a
    /// light command (mis-routed) is a no-op — routing is by key, so this should
    /// never happen, but the entity stays total regardless.
    fn handle_command(&mut self, command: &CommandMessage) -> Option<StateMessage> {
        let CommandMessage::Light(cmd) = command else {
            return None;
        };
        if cmd.has_state {
            self.on = cmd.state;
        }
        if cmd.has_brightness {
            self.brightness = cmd.brightness.clamp(0.0, 1.0);
        }
        if cmd.has_rgb {
            self.rgb = Rgb01 {
                red: cmd.red.clamp(0.0, 1.0),
                green: cmd.green.clamp(0.0, 1.0),
                blue: cmd.blue.clamp(0.0, 1.0),
            };
        }
        if cmd.has_effect {
            self.effect = cmd.effect.clone();
        }
        Some(self.state_message())
    }
}

/// A led-core render directive derived from a [`LightEntity`]'s state — the
/// concrete coupling between the HA light and the WS2812 strip.
///
/// Off, a solid brightness-scaled colour, or the Rainbow effect. The LED-driver
/// bin matches on this each frame and renders through led-core.
#[cfg(feature = "light")]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LedRender {
    /// The light is off — render nothing (a black strip).
    Off,
    /// A single colour across the strip, already scaled by master brightness.
    Solid(led_core::SolidColor),
    /// The animated rainbow, its value channel set to the master brightness.
    Rainbow(led_core::Rainbow),
}

#[cfg(feature = "light")]
impl LightEntity {
    /// Default rainbow hue spread between adjacent pixels.
    const RAINBOW_SPATIAL: u8 = 8;
    /// Default rainbow rotation speed (hue steps per second).
    const RAINBOW_SPEED: u8 = 24;

    /// Map the current light state to a led-core render directive.
    ///
    /// `Off` when the light is off; the [`RAINBOW_EFFECT`] name selects the
    /// Rainbow effect with `val` set to the master brightness; any other effect
    /// (including [`NO_EFFECT`]) renders the commanded RGB scaled by brightness.
    pub fn led_render(&self) -> LedRender {
        if !self.on {
            return LedRender::Off;
        }
        let val: u8 = to_u8(self.brightness);
        if self.effect == RAINBOW_EFFECT {
            return LedRender::Rainbow(led_core::Rainbow {
                spatial: Self::RAINBOW_SPATIAL,
                speed: Self::RAINBOW_SPEED,
                sat: 255,
                val,
            });
        }
        LedRender::Solid(led_core::SolidColor::new(led_core::Rgb::new(
            to_u8(self.rgb.red * self.brightness),
            to_u8(self.rgb.green * self.brightness),
            to_u8(self.rgb.blue * self.brightness),
        )))
    }
}

/// Convert a `0.0..=1.0` fraction to an 8-bit channel, clamped and rounded.
#[cfg(feature = "light")]
fn to_u8(fraction: f32) -> u8 {
    (fraction.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Registry;

    fn light() -> LightEntity {
        LightEntity::new(LightConfig::nightdriver("strip", "LED Strip"))
    }

    /// A `LightCommandRequest` targeting `key`, with only the `has_*` flags the
    /// caller sets — every other attribute left untouched.
    #[derive(Default)]
    struct Command {
        key: u32,
        state: Option<bool>,
        brightness: Option<f32>,
        rgb: Option<(f32, f32, f32)>,
        effect: Option<String>,
    }

    impl Command {
        fn into_message(self) -> CommandMessage {
            let (has_rgb, (red, green, blue)) = match self.rgb {
                Some(rgb) => (true, rgb),
                None => (false, (0.0, 0.0, 0.0)),
            };
            CommandMessage::Light(proto::LightCommandRequest {
                key: self.key,
                has_state: self.state.is_some(),
                state: self.state.unwrap_or(false),
                has_brightness: self.brightness.is_some(),
                brightness: self.brightness.unwrap_or(0.0),
                has_rgb,
                red,
                green,
                blue,
                has_effect: self.effect.is_some(),
                effect: self.effect.unwrap_or_default(),
                ..Default::default()
            })
        }
    }

    #[test]
    fn the_key_is_derived_from_the_object_id() {
        assert_eq!(light().key(), object_id_key("strip"));
    }

    #[test]
    fn the_list_message_advertises_rgb_and_the_effects() {
        match light().list_message() {
            ListMessage::Light(m) => {
                assert_eq!(m.object_id, "strip");
                assert_eq!(m.supported_color_modes, vec![proto::ColorMode::Rgb as i32]);
                assert_eq!(m.effects, vec!["None".to_string(), "Rainbow".to_string()]);
            }
            other => panic!("expected a Light list, got {other:?}"),
        }
    }

    #[test]
    fn a_fresh_light_reports_off() {
        match light().state_message() {
            StateMessage::Light(m) => {
                assert!(!m.state);
                assert_eq!(m.effect, "None");
            }
            other => panic!("expected a Light state, got {other:?}"),
        }
    }

    #[test]
    fn a_command_turns_it_on_and_reports_the_new_state() {
        let mut light: LightEntity = light();
        let command: CommandMessage = Command {
            key: light.key(),
            state: Some(true),
            brightness: Some(0.5),
            ..Default::default()
        }
        .into_message();
        match light.handle_command(&command) {
            Some(StateMessage::Light(m)) => {
                assert!(m.state);
                assert_eq!(m.brightness, 0.5);
            }
            other => panic!("expected a Light state, got {other:?}"),
        }
        assert!(light.is_on());
        assert_eq!(light.brightness(), 0.5);
    }

    #[test]
    fn a_command_only_touches_the_fields_it_carries() {
        let mut light: LightEntity = light();
        // First set brightness + effect.
        light.handle_command(
            &Command {
                key: light.key(),
                state: Some(true),
                brightness: Some(0.8),
                effect: Some("Rainbow".to_string()),
                ..Default::default()
            }
            .into_message(),
        );
        // Now a command that ONLY changes brightness must leave the effect alone.
        light.handle_command(
            &Command {
                key: light.key(),
                brightness: Some(0.2),
                ..Default::default()
            }
            .into_message(),
        );
        assert_eq!(light.brightness(), 0.2);
        assert_eq!(
            light.effect(),
            "Rainbow",
            "effect must survive a brightness-only command"
        );
        assert!(light.is_on(), "on state must survive too");
    }

    #[test]
    fn out_of_range_command_values_clamp() {
        let mut light: LightEntity = light();
        light.handle_command(
            &Command {
                key: light.key(),
                state: Some(true),
                brightness: Some(9.0),
                rgb: Some((2.0, -1.0, 0.5)),
                ..Default::default()
            }
            .into_message(),
        );
        assert_eq!(light.brightness(), 1.0);
        match light.state_message() {
            StateMessage::Light(m) => {
                assert_eq!(m.red, 1.0);
                assert_eq!(m.green, 0.0);
                assert_eq!(m.blue, 0.5);
            }
            other => panic!("expected a Light state, got {other:?}"),
        }
    }

    #[test]
    fn a_non_light_command_is_a_no_op() {
        let mut light: LightEntity = light();
        let switch: CommandMessage = CommandMessage::Switch(proto::SwitchCommandRequest {
            key: light.key(),
            state: true,
            device_id: 0,
        });
        assert_eq!(light.handle_command(&switch), None);
        assert!(!light.is_on(), "a switch command must not toggle a light");
    }

    #[test]
    fn a_light_command_routes_through_the_registry_by_key() {
        let mut registry: Registry = Registry::new();
        let light: LightEntity = light();
        let key: u32 = light.key();
        registry.register(Box::new(light));

        let command: CommandMessage = Command {
            key,
            state: Some(true),
            ..Default::default()
        }
        .into_message();
        match registry.apply_command(&command) {
            Some(StateMessage::Light(m)) => assert!(m.state),
            other => panic!("expected the light's new state, got {other:?}"),
        }
    }

    #[test]
    fn a_registry_mixes_sensor_and_light() {
        // Many entities of different types enumerate, in order, without regressing
        // the read-only Sensor.
        use crate::{SensorConfig, SensorEntity};
        let mut registry: Registry = Registry::new();
        registry.register(Box::new(SensorEntity::new(SensorConfig {
            object_id: "soil".to_string(),
            name: "Soil".to_string(),
            unit_of_measurement: "%".to_string(),
            accuracy_decimals: 0,
            device_class: "moisture".to_string(),
            state_class: proto::SensorStateClass::StateClassMeasurement,
        })));
        registry.register(Box::new(light()));

        assert_eq!(registry.len(), 2);
        let lists: Vec<ListMessage> = registry.list_messages();
        assert!(matches!(lists[0], ListMessage::Sensor(_)));
        assert!(matches!(lists[1], ListMessage::Light(_)));
    }

    #[cfg(feature = "light")]
    mod led_mapping {
        use super::*;

        #[test]
        fn an_off_light_renders_nothing() {
            assert_eq!(light().led_render(), LedRender::Off);
        }

        #[test]
        fn a_solid_light_renders_the_scaled_rgb() {
            let mut light: LightEntity = light();
            light.handle_command(
                &Command {
                    key: light.key(),
                    state: Some(true),
                    brightness: Some(1.0),
                    rgb: Some((1.0, 0.0, 0.0)),
                    effect: Some(NO_EFFECT.to_string()),
                }
                .into_message(),
            );
            assert_eq!(
                light.led_render(),
                LedRender::Solid(led_core::SolidColor::new(led_core::Rgb::new(255, 0, 0)))
            );
        }

        #[test]
        fn half_brightness_halves_the_solid_channels() {
            let mut light: LightEntity = light();
            light.handle_command(
                &Command {
                    key: light.key(),
                    state: Some(true),
                    brightness: Some(0.5),
                    rgb: Some((1.0, 1.0, 1.0)),
                    ..Default::default()
                }
                .into_message(),
            );
            // 1.0 * 0.5 * 255 = 127.5 -> 128 (round half up).
            assert_eq!(
                light.led_render(),
                LedRender::Solid(led_core::SolidColor::new(led_core::Rgb::new(128, 128, 128)))
            );
        }

        #[test]
        fn the_rainbow_effect_selects_rainbow_at_the_master_brightness() {
            let mut light: LightEntity = light();
            light.handle_command(
                &Command {
                    key: light.key(),
                    state: Some(true),
                    brightness: Some(1.0),
                    effect: Some(RAINBOW_EFFECT.to_string()),
                    ..Default::default()
                }
                .into_message(),
            );
            match light.led_render() {
                LedRender::Rainbow(r) => assert_eq!(r.val, 255),
                other => panic!("expected Rainbow, got {other:?}"),
            }
        }
    }
}
