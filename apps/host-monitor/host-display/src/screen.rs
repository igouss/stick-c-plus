//! The monitor screen: a [`HostState`] as two labelled graphs and a creature.
//!
//! The whole of what the host monitor's glass ever says, and the only place that
//! decides it. Device-independent by construction — it draws into any [`DrawTarget`],
//! which is what lets the on-target panel and a host framebuffer render *the same code*
//! rather than two copies that drift.

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use host_core::{History, HostState, Status};
use platform_display::{sparkline, sprite, text_line, RenderError};

use crate::layout::{
    CPU_GRAPH, CPU_LABEL_Y, LABEL_WIDTH, MEM_GRAPH, MEM_LABEL_Y, SPRITE_ORIGIN, SPRITE_SCALE,
    TEXT_X,
};
use crate::scene::{self, Scene, PEGGED_AT};

/// The retention depth, and thus the width of each plot — see [`host_core::history`].
const CAPACITY: usize = host_core::history::CAPACITY;

/// The CPU graph's bars.
const CPU_INK: Rgb565 = Rgb565::CYAN;
/// The memory graph's bars.
const MEM_INK: Rgb565 = Rgb565::YELLOW;

/// Render the host monitor: two scrolling graphs, their live percentages, and the
/// creature that stands for the host's load.
///
/// `elapsed_ms` is how long the current load band has been on the glass — the creature's
/// animation clock. A calm or busy host ignores it and shows a motionless creature, so a
/// healthy monitor repaints only as its graph scrolls; see [`crate::scene`].
///
/// No full-screen clear — each graph fills its own plot, each label paints its own row
/// over an opaque background, and the creature overwrites its own box, so a redraw
/// touches only those regions and there is no flash. Percentages are right-aligned in a
/// fixed field, so a shrinking value erases the wider one it replaced.
///
/// The two graphs keep drawing the retained history **even when the host is unavailable**
/// — a stale or faulted status blanks the label to `--` and puts the creature to sleep,
/// but the trailing bars stay, because a window of what the host was doing is useful and
/// a frozen scalar would be the only lie.
pub fn render<D>(
    target: &mut D,
    state: HostState,
    elapsed_ms: u64,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    graphs(target, &state.history)?;
    labels(target, state.status)?;
    creature(target, state.status, elapsed_ms)
}

/// Plot the CPU and memory series from the history into their two graphs.
///
/// The samples are unpacked into two fixed `[u8; CAPACITY]` scratch arrays — one bar per
/// sample, oldest first — and handed to the board-generic [`sparkline`]. Only the first
/// `history.len()` columns carry a bar; the rest of each plot is the black the graph
/// scrolls into.
fn graphs<D>(target: &mut D, history: &History) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    let mut cpu: [u8; CAPACITY] = [0; CAPACITY];
    let mut mem: [u8; CAPACITY] = [0; CAPACITY];
    let samples: &[host_core::Sample] = history.samples();
    for (column, sample) in samples.iter().enumerate() {
        cpu[column] = sample.cpu().value();
        mem[column] = sample.mem().value();
    }
    let filled: usize = samples.len();

    sparkline(target, CPU_GRAPH, &cpu[..filled], CPU_INK, Rgb565::BLACK)?;
    sparkline(target, MEM_GRAPH, &mem[..filled], MEM_INK, Rgb565::BLACK)
}

/// The colour a percentage is drawn in: red once it reaches [`PEGGED_AT`], white below.
fn ink_for(percent: u8) -> Rgb565 {
    if percent >= PEGGED_AT {
        Rgb565::RED
    } else {
        Rgb565::WHITE
    }
}

/// Draw one label row: the platform [`text_line`] primitive bound to this app's left
/// column ([`TEXT_X`]) and field width ([`LABEL_WIDTH`]).
fn label<D>(
    target: &mut D,
    y: i32,
    color: Rgb565,
    content: core::fmt::Arguments<'_>,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    text_line(target, Point::new(TEXT_X, y), color, LABEL_WIDTH, content)
}

/// Paint the two label rows for `status`.
///
/// A fresh status shows each live percentage, red once it is pegged. Every unavailable
/// status shows `--` instead of a number — the graph below still carries the retained
/// history, but there is no *current* value to state, and a frozen one would be a lie.
/// A fault is red (attention); a device that simply has not finished its first interval
/// is white (not a problem). The creature beside the rows says which of the unavailable
/// states this is.
fn labels<D>(target: &mut D, status: Status) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    match status {
        Status::Fresh(sample) => {
            let cpu: u8 = sample.cpu().value();
            let mem: u8 = sample.mem().value();
            label(
                target,
                CPU_LABEL_Y,
                ink_for(cpu),
                format_args!("CPU {cpu:>3}%"),
            )?;
            label(
                target,
                MEM_LABEL_Y,
                ink_for(mem),
                format_args!("MEM {mem:>3}%"),
            )?;
        }
        Status::Faulted(_) | Status::Stale => {
            label(target, CPU_LABEL_Y, Rgb565::RED, format_args!("CPU  --"))?;
            label(target, MEM_LABEL_Y, Rgb565::RED, format_args!("MEM  --"))?;
        }
        Status::NeverSampled => {
            label(target, CPU_LABEL_Y, Rgb565::WHITE, format_args!("CPU  --"))?;
            label(target, MEM_LABEL_Y, Rgb565::WHITE, format_args!("MEM  --"))?;
        }
    }
    Ok(())
}

/// Paint the creature for `status` in the panel's right-hand region.
///
/// Drawn **opaque** (see [`sprite::draw_onto`]): each frame overwrites its own 100×100
/// box against the black background, so an animating creature never smears the previous
/// frame and never needs a clear that would flash.
fn creature<D>(target: &mut D, status: Status, elapsed_ms: u64) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    let band: scene::LoadBand = scene::band(status);
    let scene: Scene = scene::scene(band);
    let index: usize = scene::frame_index(band, elapsed_ms);
    sprite::draw_onto(
        target,
        scene.sprite,
        &scene.sprite.frames()[index],
        SPRITE_ORIGIN,
        SPRITE_SCALE,
        Rgb565::BLACK,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use host_core::{HostFault, Percent, Sample};
    use platform_display::testing::Framebuffer;
    use platform_display::SCREEN_SIZE;

    /// A history whose CPU series is `loads` (memory fixed at 0), oldest first.
    fn history_of(loads: &[u8]) -> History {
        let mut history: History = History::new();
        for &load in loads {
            history.push(Sample::new(
                Percent::new(load).expect("0..=100"),
                Percent::ZERO,
            ));
        }
        history
    }

    /// Paint `status` with `loads` of history, at the instant the band appeared.
    fn painted(status: Status, loads: &[u8]) -> Framebuffer {
        let mut fb: Framebuffer = Framebuffer::new();
        render(&mut fb, HostState::new(history_of(loads), status), 0)
            .expect("a framebuffer render cannot fail");
        fb
    }

    fn fresh(cpu: u8, mem: u8) -> Status {
        Status::Fresh(Sample::new(
            Percent::new(cpu).expect("0..=100"),
            Percent::new(mem).expect("0..=100"),
        ))
    }

    /// How many lit pixels fall inside `rect` — so a test can look at just one graph.
    fn lit_inside(fb: &Framebuffer, rect: embedded_graphics::primitives::Rectangle) -> usize {
        let width: i32 = SCREEN_SIZE.width as i32;
        fb.pixels()
            .iter()
            .enumerate()
            .filter(|(index, colour): &(usize, &Rgb565)| {
                let x: i32 = *index as i32 % width;
                let y: i32 = *index as i32 / width;
                let inside: bool = x >= rect.top_left.x
                    && y >= rect.top_left.y
                    && x < rect.top_left.x + rect.size.width as i32
                    && y < rect.top_left.y + rect.size.height as i32;
                inside && **colour != Rgb565::BLACK
            })
            .count()
    }

    /// A fresh state paints its labels and creature, all on the canvas.
    #[test]
    fn a_fresh_state_paints_and_stays_on_the_canvas() {
        let fb: Framebuffer = painted(fresh(42, 71), &[10, 42]);
        assert!(fb.lit_pixels() > 0);
        assert_eq!(fb.escaped(), 0, "nothing may be clipped off the canvas");
    }

    /// The CPU graph lights up with samples, and taller samples light more of it — the
    /// bars are wired to the CPU series, not decoration.
    #[test]
    fn taller_cpu_samples_fill_more_of_the_cpu_graph() {
        let low: Framebuffer = painted(fresh(10, 0), &[10, 10, 10]);
        let high: Framebuffer = painted(fresh(90, 0), &[90, 90, 90]);
        assert!(
            lit_inside(&high, CPU_GRAPH) > lit_inside(&low, CPU_GRAPH),
            "a busier CPU history must fill more of the CPU graph"
        );
    }

    /// The two graphs are independent: memory bars land only in the memory plot.
    #[test]
    fn the_memory_series_drives_the_memory_graph() {
        // CPU flat at 0, memory high: the CPU plot stays empty, the memory plot fills.
        let fb: Framebuffer = painted(fresh(0, 90), &[]);
        let with_mem: Framebuffer = {
            let mut history: History = History::new();
            history.push(Sample::new(Percent::ZERO, Percent::new(90).unwrap()));
            let mut fb: Framebuffer = Framebuffer::new();
            render(&mut fb, HostState::new(history, fresh(0, 90)), 0).expect("render");
            fb
        };
        assert_eq!(
            lit_inside(&fb, MEM_GRAPH),
            0,
            "no samples, empty memory plot"
        );
        assert!(
            lit_inside(&with_mem, MEM_GRAPH) > 0,
            "a memory sample fills its plot"
        );
        assert_eq!(
            lit_inside(&with_mem, CPU_GRAPH),
            0,
            "cpu 0 leaves the cpu plot empty"
        );
    }

    /// A fresh state and a fault render differently — the label and the creature both
    /// change — so a glance tells a live host from a dark one.
    #[test]
    fn fresh_and_faulted_render_differently() {
        let fresh_fb: Framebuffer = painted(fresh(30, 40), &[30]);
        let fault_fb: Framebuffer = painted(Status::Faulted(HostFault::Unreachable), &[30]);
        assert_ne!(fresh_fb.pixels(), fault_fb.pixels());
    }

    /// A faulted and a stale host differ only by their creature (startled vs asleep) —
    /// the labels are the same `--` — but they must still be distinguishable on the glass.
    #[test]
    fn faulted_and_stale_differ_by_their_creature() {
        let fault_fb: Framebuffer = painted(Status::Faulted(HostFault::Unreachable), &[50]);
        let stale_fb: Framebuffer = painted(Status::Stale, &[50]);
        assert_ne!(
            fault_fb.pixels(),
            stale_fb.pixels(),
            "an unreachable host and a dead poller must look different"
        );
    }

    /// The graph survives an unavailable status: a faulted host still shows its trailing
    /// history, unlike a scalar that must blank when stale.
    #[test]
    fn a_faulted_host_still_shows_its_history() {
        let fb: Framebuffer = painted(Status::Faulted(HostFault::Unreachable), &[80, 80, 80]);
        assert!(
            lit_inside(&fb, CPU_GRAPH) > 0,
            "the retained window must stay on the glass when the host goes dark"
        );
    }

    /// Rendering is deterministic — the same state paints the same pixels — which is what
    /// lets the render loop suppress a redundant repaint on an unchanged picture.
    #[test]
    fn the_same_state_paints_the_same_pixels() {
        let a: Framebuffer = painted(fresh(55, 60), &[20, 55]);
        let b: Framebuffer = painted(fresh(55, 60), &[20, 55]);
        assert_eq!(a.pixels(), b.pixels());
    }

    /// A receding CPU spike erases the taller bars it replaces — the graph does not smear
    /// as it scrolls. Drawn over one framebuffer, the second render must leave fewer lit
    /// pixels in the CPU plot than the first.
    #[test]
    fn a_receding_graph_erases_the_taller_bars() {
        let mut fb: Framebuffer = Framebuffer::new();
        render(
            &mut fb,
            HostState::new(history_of(&[100, 100, 100]), fresh(100, 0)),
            0,
        )
        .expect("tall render");
        let tall: usize = lit_inside(&fb, CPU_GRAPH);

        render(
            &mut fb,
            HostState::new(history_of(&[5, 5, 5]), fresh(5, 0)),
            0,
        )
        .expect("short render");
        let short: usize = lit_inside(&fb, CPU_GRAPH);

        assert!(tall > 0);
        assert!(
            short < tall,
            "the taller bars were not erased — the graph would smear"
        );
    }
}
