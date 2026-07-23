//! The info screen: six pages of "what is this and what does it do".
//!
//! Four are prose and two report the board. The prose pages go through [`page`] so the same
//! words re-flow onto both canvases; the device and bluetooth pages are label/value rows,
//! because a field whose value wrapped mid-address would be worse than one that is cut.
//!
//! The pages exist for the owner who picked the stick up not knowing what it was — which, for a
//! desk pet that silently gates tool calls, is the difference between a toy and something
//! alarming.

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use platform_display::{text_field, FieldAlign, RenderError, FONT};

use crate::layout::Layout;
use crate::page;
use crate::palette;
use crate::view::{DeviceView, InfoPage};
use crate::wrap::Cut;

/// The label field on the device and bluetooth pages.
const LABEL_COLS: usize = 5;

/// Draw the info screen at `page`.
pub fn render<D>(
    target: &mut D,
    layout: &Layout,
    device: &DeviceView,
    page: InfoPage,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    match page {
        InfoPage::About => prose(target, layout, "ABOUT", ABOUT),
        InfoPage::Buttons => prose(target, layout, "BUTTONS", BUTTONS),
        InfoPage::Claude => prose(target, layout, "CLAUDE", CLAUDE),
        InfoPage::Device => fields(target, layout, "DEVICE", &device_fields(device)),
        InfoPage::Bluetooth => fields(target, layout, "BLUETOOTH", &bluetooth_fields(device)),
        InfoPage::Credits => prose(target, layout, "CREDITS", CREDITS),
    }
}

const ABOUT: &str = "A desk pet that lives off your Claude Code approvals. It shows what \
                     Claude is doing and asks before a tool runs.";

const BUTTONS: &str = "A allows the pending tool call.\n\
                       B denies it.\n\
                       Hold A for the menu.\n\
                       Face down to nap.";

const CLAUDE: &str = "A hook on your machine holds each tool call and asks this stick. No \
                      answer in time and the call falls through to your normal permissions.";

const CREDITS: &str = "Creature art: ClaudePix.\n\
                       Firmware: Rust on ESP-IDF.\n\
                       Board: M5StickC Plus.";

/// A prose page, wrapped onto whichever canvas it lands on.
fn prose<D>(
    target: &mut D,
    layout: &Layout,
    title: &str,
    body: &str,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    page::render(target, layout, title, palette::TITLE, body)
}

/// One label/value row of a reporting page.
struct Row<'a> {
    label: &'a str,
    value: &'a str,
    colour: Rgb565,
}

/// What the device page reports.
fn device_fields(device: &DeviceView) -> [Row<'_>; 3] {
    [
        Row {
            label: "NAME",
            value: device.name.as_str(),
            colour: palette::PRIMARY,
        },
        Row {
            label: "FW",
            value: device.firmware.as_str(),
            colour: palette::DIM,
        },
        Row {
            label: "OWNER",
            value: device.owner.as_str(),
            colour: palette::DIM,
        },
    ]
}

/// What the bluetooth page reports.
///
/// Bonded and linked are two different facts and both are shown: a bonded stick with no link is
/// a bridge that is not running, which looks identical to a broken pairing if only one of them
/// reaches the glass.
fn bluetooth_fields(device: &DeviceView) -> [Row<'_>; 3] {
    [
        Row {
            label: "ADDR",
            value: device.address.as_str(),
            colour: palette::DIM,
        },
        Row {
            label: "BOND",
            value: if device.bonded { "yes" } else { "no" },
            colour: if device.bonded {
                palette::LINKED
            } else {
                palette::UNLINKED
            },
        },
        Row {
            label: "LINK",
            value: if device.linked { "up" } else { "down" },
            colour: if device.linked {
                palette::LINKED
            } else {
                palette::UNLINKED
            },
        },
    ]
}

/// A reporting page: a title, then label/value rows, then blank rows to the bottom edge.
fn fields<D>(
    target: &mut D,
    layout: &Layout,
    title: &str,
    rows: &[Row<'_>],
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    text_field(
        target,
        layout.row_origin(0),
        palette::TITLE,
        layout.cols,
        layout.align,
        format_args!("{title}"),
    )?;

    let value_x: i32 = layout.left + (FONT.character_size.width * LABEL_COLS as u32) as i32;
    let value_cols: usize = layout.cols - LABEL_COLS;
    let shown: usize = rows.len().min(layout.rows - 1);

    rows.iter()
        .take(shown)
        .enumerate()
        .try_for_each(|(index, row): (usize, &Row<'_>)| {
            let at: Point = layout.row_origin(1 + index);
            text_field(
                target,
                at,
                palette::LABEL,
                LABEL_COLS,
                FieldAlign::Left,
                format_args!("{}", row.label),
            )?;
            text_field(
                target,
                Point::new(value_x, at.y),
                row.colour,
                value_cols,
                FieldAlign::Left,
                format_args!("{}", Cut(row.value, value_cols)),
            )
        })?;

    page::blank(target, layout, 1 + shown, layout.rows - 1 - shown)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{LANDSCAPE, PORTRAIT};
    use crate::view::Field;
    use platform_display::testing::Framebuffer;

    fn device() -> DeviceView {
        DeviceView {
            name: Field::new("Claude-4F2A"),
            firmware: Field::new("0.1.0"),
            address: Field::new("A0:B7:65:4F:2A:11"),
            owner: Field::new("elendal"),
            bonded: true,
            linked: true,
        }
    }

    fn painted(layout: &Layout, page: InfoPage, device: &DeviceView) -> Framebuffer {
        let mut fb: Framebuffer = Framebuffer::sized(layout.canvas);
        render(&mut fb, layout, device, page).expect("a framebuffer render cannot fail");
        fb
    }

    /// One: every page paints something.
    #[test]
    fn every_page_paints_pixels() {
        let blank: usize = InfoPage::ALL
            .iter()
            .filter(|page: &&InfoPage| painted(&LANDSCAPE, **page, &device()).lit_pixels() == 0)
            .count();
        assert_eq!(blank, 0);
    }

    /// Many: the six pages are six different pictures — none is a copy of its neighbour.
    #[test]
    fn the_six_pages_are_six_different_pictures() {
        let duplicates: usize = InfoPage::ALL
            .iter()
            .enumerate()
            .filter(|(index, page): &(usize, &InfoPage)| {
                let next: InfoPage = InfoPage::ALL[(index + 1) % InfoPage::ALL.len()];
                painted(&LANDSCAPE, **page, &device()).pixels()
                    == painted(&LANDSCAPE, next, &device()).pixels()
            })
            .count();
        assert_eq!(duplicates, 0);
    }

    /// Bonded and linked are two facts, and both reach the glass: a bonded stick with no bridge
    /// running must not look like a broken pairing.
    #[test]
    fn bonded_and_linked_are_reported_separately() {
        let mut unlinked: DeviceView = device();
        unlinked.linked = false;
        let mut unbonded: DeviceView = device();
        unbonded.bonded = false;
        let base: Framebuffer = painted(&LANDSCAPE, InfoPage::Bluetooth, &device());
        assert_ne!(
            base.pixels(),
            painted(&LANDSCAPE, InfoPage::Bluetooth, &unlinked).pixels()
        );
        assert_ne!(
            base.pixels(),
            painted(&LANDSCAPE, InfoPage::Bluetooth, &unbonded).pixels()
        );
    }

    /// The device fields reach the glass rather than a static template that ignores them.
    #[test]
    fn the_device_fields_reach_the_glass() {
        let mut renamed: DeviceView = device();
        renamed.name = Field::new("Claude-0000");
        assert_ne!(
            painted(&LANDSCAPE, InfoPage::Device, &device()).pixels(),
            painted(&LANDSCAPE, InfoPage::Device, &renamed).pixels()
        );
    }

    /// Nothing escapes either canvas, on any page, with every field at its full width — the
    /// seventeen-character address on a thirteen-column canvas is the case this exists for.
    #[test]
    fn no_page_escapes_either_canvas() {
        let widest: DeviceView = DeviceView {
            name: Field::new("Claude-WWWWWWWWWWWWWWWW"),
            firmware: Field::new("0.1.0-rc1+build.99999999"),
            address: Field::new("A0:B7:65:4F:2A:11"),
            owner: Field::new("a-very-long-owner-label!"),
            bonded: false,
            linked: false,
        };
        let escaped: usize = InfoPage::ALL
            .iter()
            .map(|page: &InfoPage| {
                painted(&LANDSCAPE, *page, &widest).escaped()
                    + painted(&PORTRAIT, *page, &widest).escaped()
            })
            .sum();
        assert_eq!(escaped, 0);
    }
}
