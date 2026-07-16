//! The monitor screen: a [`HostState`] as three host rows, each a header and two sparklines.
//!
//! The whole of what the host monitor's glass ever says, and the only place that decides it.
//! Device-independent by construction — it draws into any [`DrawTarget`], which is what lets
//! the on-target panel and a host framebuffer render *the same code* rather than two copies
//! that drift.
//!
//! ## One row per host, the frame outlives the reading
//!
//! Each of the endpoint's hosts gets a row: its name, its two current percentages, and two
//! side-by-side sparklines (CPU cyan on the left, memory yellow on the right) drawn from the
//! ready-to-plot series the frame carries. A host the endpoint reports as down (all-`null`
//! arrays) keeps its row and shows "no data" rather than vanishing.
//!
//! When the *endpoint* goes stale or faults, the last good frame is still drawn — a window
//! of what the hosts were doing is useful, not a lie — but the host names are tinted and a
//! status token (`DOWN` / `BAD` / `OLD`) appears top-right, so a glance says the numbers are
//! no longer live. Before the first fetch there is no frame, so the glass shows a short
//! "waiting" hint instead of empty rows.
//!
//! ## What a host render can and cannot prove
//!
//! It **can** prove the layout, the wording, the alignment, the colour each state is drawn
//! in, that a receding graph erases the taller bars it replaces, and that nothing is
//! clipped. It **cannot** prove anything below [`DrawTarget`]: the panel's colour order,
//! offset, inversion, or backlight — see [`platform_display`].

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::Rectangle;
use host_core::{HostFault, HostSeries, HostState, Percent, Pulse, Series, Status};
use platform_display::{sparkline, text_line, RenderError};

use crate::layout::{
    cpu_graph, header_origin, mem_graph, row_top, CPU_NUM_X, GRAPH_WIDTH, MEM_NUM_X, NAME_CHARS,
    NODATA_CHARS, NUM_CHARS, ROWS, STATUS_CHARS, STATUS_X,
};

/// The load at or above which a percentage is drawn in red — the host is pegged.
pub const PEGGED_AT: u8 = 85;

/// The CPU sparkline's bars, and its percentage.
const CPU_INK: Rgb565 = Rgb565::CYAN;
/// The memory sparkline's bars, and its percentage.
const MEM_INK: Rgb565 = Rgb565::YELLOW;
/// A dimmed grey — a stale host's name, and a down host's "no data".
const DIM: Rgb565 = Rgb565::new(12, 24, 12);

/// Render the host monitor: one row per host, plus the endpoint's status.
///
/// `_elapsed_ms` is the render loop's animation clock. This screen has no animated creature
/// — three hosts fill the glass, leaving no room for one — so a healthy monitor is *still*
/// and the loop repaints only when the frame or the status changes; the parameter is kept
/// for the board-generic render signature.
///
/// No full-screen clear — each field paints its own row over an opaque background, each
/// sparkline fills its own plot, so a redraw touches only those regions and there is no
/// flash. Percentages are right-aligned in a fixed field, so a shrinking value erases the
/// wider one it replaced.
pub fn render<D>(
    target: &mut D,
    state: HostState,
    _elapsed_ms: u64,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    let frame: Option<&Pulse> = state.frame.as_ref();
    let hosts: &[HostSeries] = frame.map(|pulse: &Pulse| pulse.hosts()).unwrap_or(&[]);
    let name_ink: Rgb565 = name_ink(state.status);

    for row in 0..ROWS {
        match hosts.get(row) {
            Some(host) => host_row(target, row, host, name_ink)?,
            None => empty_row(target, row)?,
        }
    }

    frame_status(target, state)
}

/// The colour a host name is drawn in, given the endpoint's status: white when fresh, red on
/// a fault, dimmed when stale — so the whole board reads "these numbers are old" at a glance.
fn name_ink(status: Status) -> Rgb565 {
    match status {
        Status::Fresh | Status::NeverSampled => Rgb565::WHITE,
        Status::Faulted(_) => Rgb565::RED,
        Status::Stale => DIM,
    }
}

/// Draw one host's row: its name, its two percentages (or "no data"), and its two sparklines.
fn host_row<D>(
    target: &mut D,
    row: usize,
    host: &HostSeries,
    name_ink: Rgb565,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    label(
        target,
        header_origin(row),
        name_ink,
        NAME_CHARS,
        format_args!("{}", host.name()),
    )?;

    if host.is_down() {
        // The endpoint sent this host as all-null: keep the row, say so, draw no bars.
        label(
            target,
            Point::new(CPU_NUM_X, row_top(row)),
            DIM,
            NODATA_CHARS,
            format_args!("no data"),
        )?;
        clear_graph(target, cpu_graph(row))?;
        clear_graph(target, mem_graph(row))?;
    } else {
        percent(
            target,
            Point::new(CPU_NUM_X, row_top(row)),
            host.cpu().latest(),
            CPU_INK,
        )?;
        percent(
            target,
            Point::new(MEM_NUM_X, row_top(row)),
            host.mem().latest(),
            MEM_INK,
        )?;
        graph(target, cpu_graph(row), host.cpu(), CPU_INK)?;
        graph(target, mem_graph(row), host.mem(), MEM_INK)?;
    }
    Ok(())
}

/// Blank a row that has no host — every field to spaces, both plots to background — so it
/// erases whatever a taller frame drew there before.
fn empty_row<D>(target: &mut D, row: usize) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    label(
        target,
        header_origin(row),
        DIM,
        NAME_CHARS,
        format_args!(""),
    )?;
    label(
        target,
        Point::new(CPU_NUM_X, row_top(row)),
        DIM,
        NUM_CHARS,
        format_args!(""),
    )?;
    label(
        target,
        Point::new(MEM_NUM_X, row_top(row)),
        DIM,
        NUM_CHARS,
        format_args!(""),
    )?;
    clear_graph(target, cpu_graph(row))?;
    clear_graph(target, mem_graph(row))
}

/// Paint one percentage field, right-aligned: the value in `ink` (red once pegged), or `--`
/// in `ink` when the series has no present reading.
fn percent<D>(
    target: &mut D,
    at: Point,
    latest: Option<Percent>,
    ink: Rgb565,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    match latest {
        Some(value) => {
            let v: u8 = value.value();
            let colour: Rgb565 = if v >= PEGGED_AT { Rgb565::RED } else { ink };
            label(target, at, colour, NUM_CHARS, format_args!("{v:>3}%"))
        }
        None => label(target, at, ink, NUM_CHARS, format_args!("  --")),
    }
}

/// Plot a series into `rect` as a bar sparkline, stretched to fill the plot's width.
///
/// A present sample draws a bar in `ink`; a gap draws a [`DIM`] baseline tick (the
/// sparkline primitive keeps them apart). An empty series (no samples at all) draws
/// nothing — passing zero columns, not a plot full of gap ticks.
fn graph<D>(
    target: &mut D,
    rect: Rectangle,
    series: &Series,
    ink: Rgb565,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    let bars: [Option<u8>; GRAPH_WIDTH] = stretch(series);
    // An empty series carries neither values nor gaps, so it draws as a blank plot — not
    // `GRAPH_WIDTH` gap ticks. A non-empty series fills every column (nearest-neighbour).
    let columns: usize = if series.samples().is_empty() {
        0
    } else {
        GRAPH_WIDTH
    };
    sparkline(target, rect, &bars[..columns], ink, Rgb565::BLACK, DIM)
}

/// Fill `rect` with background — an empty sparkline — erasing whatever a live host drew there.
fn clear_graph<D>(target: &mut D, rect: Rectangle) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    sparkline(target, rect, &[], CPU_INK, Rgb565::BLACK, DIM)
}

/// Map a series onto the plot's [`GRAPH_WIDTH`] columns by nearest-neighbour, so the whole
/// plot is filled whatever the window length: oldest at the left, newest at the right.
///
/// A gap ([`None`]) stays a `None` column, which the sparkline draws as a baseline tick —
/// so a missing scrape reads as "no data here", distinct from a `0%` floor. A present
/// sample becomes `Some(value)`. An empty series maps to all-`None`; its caller draws it as
/// a blank plot rather than a row of gap ticks (see [`graph`]).
fn stretch(series: &Series) -> [Option<u8>; GRAPH_WIDTH] {
    let samples: &[Option<Percent>] = series.samples();
    let n: usize = samples.len();
    let mut bars: [Option<u8>; GRAPH_WIDTH] = [None; GRAPH_WIDTH];
    if n > 0 {
        for (column, bar) in bars.iter_mut().enumerate() {
            // column < GRAPH_WIDTH, so index = column*n/GRAPH_WIDTH is in 0..n — never n.
            let index: usize = column * n / GRAPH_WIDTH;
            *bar = samples[index].map(Percent::value);
        }
    }
    bars
}

/// Paint the top-right corner, and, when there is no frame yet, a hint in the top row's
/// name field.
///
/// The corner escalates: a health token (`DOWN` / `BAD` / `OLD`) whenever the endpoint is
/// not fresh, otherwise the frame's window span (e.g. `15m`) so the sparklines' time reach
/// is legible from the payload's own grid — not an assumption. Every arm repaints the whole
/// field, so a token erases a span and vice versa as the status changes.
fn frame_status<D>(target: &mut D, state: HostState) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    let corner: Point = Point::new(STATUS_X, row_top(0));
    match state.status {
        Status::Faulted(HostFault::Unreachable) => label(
            target,
            corner,
            Rgb565::RED,
            STATUS_CHARS,
            format_args!("DOWN"),
        )?,
        Status::Faulted(HostFault::Malformed) => label(
            target,
            corner,
            Rgb565::RED,
            STATUS_CHARS,
            format_args!("BAD"),
        )?,
        Status::Stale => label(target, corner, DIM, STATUS_CHARS, format_args!("OLD"))?,
        // Fresh (or, defensively, a never-sampled slot that somehow holds a frame): show the
        // window span the payload declared. No frame yet → blank the corner.
        Status::Fresh | Status::NeverSampled => match state.frame {
            Some(frame) => {
                let (span, unit): (u32, char) = window_span(frame.window_s());
                label(
                    target,
                    corner,
                    DIM,
                    STATUS_CHARS,
                    format_args!("{span}{unit}"),
                )?
            }
            None => label(target, corner, DIM, STATUS_CHARS, format_args!(""))?,
        },
    }

    if state.frame.is_none() {
        // No good frame ever fetched: the rows are blank, so say why in the top name field.
        let (message, ink): (&str, Rgb565) = match state.status {
            Status::NeverSampled => ("waiting", Rgb565::WHITE),
            _ => ("no pulse", Rgb565::RED),
        };
        label(
            target,
            header_origin(0),
            ink,
            NAME_CHARS,
            format_args!("{message}"),
        )?;
    }
    Ok(())
}

/// A window's width, as a compact `(value, unit)` for the corner label: whole minutes when
/// the seconds divide evenly (the endpoint's `900` → `15m`), else raw seconds.
///
/// Exact, never lossy — a span that is not a whole number of minutes stays in seconds rather
/// than rounding — and read from the payload's `window_s`, so the label reflects the grid the
/// endpoint actually sent instead of a hard-coded assumption.
fn window_span(window_s: u32) -> (u32, char) {
    if window_s >= 60 && window_s.is_multiple_of(60) {
        (window_s / 60, 'm')
    } else {
        (window_s, 's')
    }
}

/// Draw one baseline-top text field: the platform [`text_line`] primitive with an opaque
/// background, so it erases its whole field in place.
fn label<D>(
    target: &mut D,
    at: Point,
    color: Rgb565,
    width: usize,
    content: core::fmt::Arguments<'_>,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    text_line(target, at, color, width, content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use host_core::PulseBuilder;
    use platform_display::testing::Framebuffer;
    use platform_display::SCREEN_SIZE;

    /// One host's raw wire series for a test frame: `(name, cpu, mem)`.
    type HostSpec<'a> = (&'a str, &'a [Option<i32>], &'a [Option<i32>]);

    /// A frame with the given hosts, on the contract's `30 s / 900 s` grid.
    fn frame(hosts: &[HostSpec<'_>]) -> Pulse {
        frame_win(900, hosts)
    }

    /// A frame with the given hosts on a `window_s`-wide window, for the corner span label.
    fn frame_win(window_s: u32, hosts: &[HostSpec<'_>]) -> Pulse {
        let mut b: PulseBuilder = PulseBuilder::new(30, window_s);
        for (name, cpu, mem) in hosts {
            b.push(name, cpu, mem);
        }
        b.build()
    }

    /// The three-host homelab frame, all lightly loaded.
    fn homelab() -> Pulse {
        frame(&[
            (
                "fedora",
                &[Some(11), Some(13), Some(10)],
                &[Some(41), Some(44)],
            ),
            ("oracle-arm", &[Some(3), Some(4)], &[Some(58), Some(60)]),
            ("oracle-amd", &[Some(1), Some(2)], &[Some(22), Some(24)]),
        ])
    }

    /// Paint `state` into a fresh framebuffer.
    fn painted(state: HostState) -> Framebuffer {
        let mut fb: Framebuffer = Framebuffer::new();
        render(&mut fb, state, 0).expect("a framebuffer render cannot fail");
        fb
    }

    /// Lit pixels inside `rect` — so a test can look at just one graph.
    fn lit_inside(fb: &Framebuffer, rect: Rectangle) -> usize {
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

    /// A fresh three-host frame paints, and nothing is clipped off the canvas. Each row is
    /// checked via its memory graph — memory sits at 40–60%, always visible, whereas a host
    /// idling at a few percent of CPU renders (correctly) as a near-empty graph on a plot
    /// whose resolution is 5%/px.
    #[test]
    fn a_fresh_frame_paints_all_three_rows_on_the_canvas() {
        let fb: Framebuffer = painted(HostState::new(Some(homelab()), Status::Fresh));
        assert!(fb.lit_pixels() > 0);
        assert_eq!(fb.escaped(), 0, "nothing may be clipped off the canvas");
        for row in 0..ROWS {
            assert!(
                lit_inside(&fb, mem_graph(row)) > 0,
                "row {row} memory graph is empty"
            );
        }
    }

    /// A busier CPU history fills more of its graph — the bars are wired to the series.
    #[test]
    fn taller_cpu_samples_fill_more_of_the_cpu_graph() {
        let low: Framebuffer = painted(HostState::new(
            Some(frame(&[("h", &[Some(10), Some(10)], &[Some(0)])])),
            Status::Fresh,
        ));
        let high: Framebuffer = painted(HostState::new(
            Some(frame(&[("h", &[Some(90), Some(90)], &[Some(0)])])),
            Status::Fresh,
        ));
        assert!(
            lit_inside(&high, cpu_graph(0)) > lit_inside(&low, cpu_graph(0)),
            "a busier CPU series must fill more of the CPU graph"
        );
    }

    /// The two graphs are independent: memory bars land only in the memory plot.
    #[test]
    fn the_memory_series_drives_the_memory_graph() {
        let fb: Framebuffer = painted(HostState::new(
            Some(frame(&[("h", &[Some(0), Some(0)], &[Some(90), Some(90)])])),
            Status::Fresh,
        ));
        assert!(
            lit_inside(&fb, mem_graph(0)) > 0,
            "a memory sample fills its plot"
        );
        assert_eq!(
            lit_inside(&fb, cpu_graph(0)),
            0,
            "cpu 0 leaves the cpu plot empty"
        );
    }

    /// A down host keeps its row but draws no bars — the "no data" case.
    #[test]
    fn a_down_host_keeps_its_row_with_no_bars() {
        let fb: Framebuffer = painted(HostState::new(
            Some(frame(&[
                ("fedora", &[Some(50)], &[Some(50)]),
                ("oracle-arm", &[None, None], &[None, None]),
                ("oracle-amd", &[Some(1)], &[Some(22)]),
            ])),
            Status::Fresh,
        ));
        assert_eq!(
            lit_inside(&fb, cpu_graph(1)),
            0,
            "a down host must draw no CPU bars"
        );
        assert_eq!(lit_inside(&fb, mem_graph(1)), 0, "no memory bars either");
        // Its neighbours still draw — checked via memory, which is always visible.
        assert!(lit_inside(&fb, mem_graph(0)) > 0);
        assert!(lit_inside(&fb, mem_graph(2)) > 0);
    }

    /// A gap is not a zero. The middle sample is a gap in one frame and a `0%` reading in
    /// the other; everything else is identical. The gap draws a dim baseline tick where the
    /// `0%` draws nothing, so the gap lights pixels the `0%` column does not — "no data" is
    /// visibly distinct from "zero", which is the whole reason a gap is carried through as
    /// `None` instead of being flattened to `0`.
    #[test]
    fn a_gap_renders_differently_from_a_zero() {
        let zeroed: Framebuffer = painted(HostState::new(
            Some(frame(&[("h", &[Some(50), Some(0), Some(50)], &[Some(0)])])),
            Status::Fresh,
        ));
        let gapped: Framebuffer = painted(HostState::new(
            Some(frame(&[("h", &[Some(50), None, Some(50)], &[Some(0)])])),
            Status::Fresh,
        ));
        assert!(
            lit_inside(&gapped, cpu_graph(0)) > lit_inside(&zeroed, cpu_graph(0)),
            "the gap's baseline ticks light pixels the bare 0% column leaves dark"
        );
    }

    /// A fresh frame and a faulted one render differently — the names tint and a token
    /// appears — so a glance tells a live endpoint from a dark one.
    #[test]
    fn fresh_and_faulted_render_differently() {
        let fresh: Framebuffer = painted(HostState::new(Some(homelab()), Status::Fresh));
        let faulted: Framebuffer = painted(HostState::new(
            Some(homelab()),
            Status::Faulted(HostFault::Unreachable),
        ));
        assert_ne!(fresh.pixels(), faulted.pixels());
    }

    /// A stale endpoint still shows its last good frame — the frame outlives the reading.
    #[test]
    fn a_stale_endpoint_still_shows_its_last_frame() {
        let fb: Framebuffer = painted(HostState::new(Some(homelab()), Status::Stale));
        assert!(
            lit_inside(&fb, cpu_graph(0)) > 0,
            "the retained frame must stay on the glass when the endpoint goes dark"
        );
    }

    /// Before the first fetch there is no frame, and the glass says so rather than lying with
    /// empty graphs read as zero.
    #[test]
    fn never_sampled_shows_a_waiting_hint_and_no_bars() {
        let fb: Framebuffer = painted(HostState::new(None, Status::NeverSampled));
        assert!(fb.lit_pixels() > 0, "the waiting hint must be drawn");
        for row in 0..ROWS {
            assert_eq!(lit_inside(&fb, cpu_graph(row)), 0, "no frame, no bars");
        }
    }

    /// Rendering is deterministic — the same state paints the same pixels — which is what
    /// lets the render loop suppress a redundant repaint on an unchanged picture.
    #[test]
    fn the_same_state_paints_the_same_pixels() {
        let a: Framebuffer = painted(HostState::new(Some(homelab()), Status::Fresh));
        let b: Framebuffer = painted(HostState::new(Some(homelab()), Status::Fresh));
        assert_eq!(a.pixels(), b.pixels());
    }

    /// A receding spike erases the taller bars it replaces — the graph does not smear as the
    /// window is replaced. Drawn over one framebuffer, the second render must leave fewer lit
    /// pixels in the CPU plot than the first.
    #[test]
    fn a_receding_graph_erases_the_taller_bars() {
        let mut fb: Framebuffer = Framebuffer::new();
        render(
            &mut fb,
            HostState::new(
                Some(frame(&[("h", &[Some(100), Some(100)], &[Some(0)])])),
                Status::Fresh,
            ),
            0,
        )
        .expect("tall render");
        let tall: usize = lit_inside(&fb, cpu_graph(0));

        render(
            &mut fb,
            HostState::new(
                Some(frame(&[("h", &[Some(5), Some(5)], &[Some(0)])])),
                Status::Fresh,
            ),
            0,
        )
        .expect("short render");
        let short: usize = lit_inside(&fb, cpu_graph(0));

        assert!(tall > 0);
        assert!(
            short < tall,
            "the taller bars were not erased — the graph would smear"
        );
    }

    /// The window span is read from the payload's `window_s`, exact and unit-picking.
    #[test]
    fn the_window_span_reads_whole_minutes_or_falls_back_to_seconds() {
        assert_eq!(window_span(900), (15, 'm'), "the contract's 900 s is 15m");
        assert_eq!(window_span(600), (10, 'm'));
        assert_eq!(window_span(120), (2, 'm'));
        assert_eq!(
            window_span(45),
            (45, 's'),
            "a sub-minute span stays in seconds"
        );
        assert_eq!(
            window_span(90),
            (90, 's'),
            "1.5 min is not rounded — exact seconds"
        );
        assert_eq!(window_span(0), (0, 's'));
    }

    /// The grid is not cosmetic: a frame's `window_s` reaches the glass. Two frames that
    /// differ *only* in their window span paint different pixels (the corner label), so the
    /// display genuinely consumes the payload's grid rather than merely storing it.
    #[test]
    fn a_different_window_span_paints_a_different_corner() {
        let host: &[HostSpec<'_>] = &[("h", &[Some(10)], &[Some(20)])];
        let fifteen: Framebuffer =
            painted(HostState::new(Some(frame_win(900, host)), Status::Fresh));
        let ten: Framebuffer = painted(HostState::new(Some(frame_win(600, host)), Status::Fresh));
        assert_ne!(
            fifteen.pixels(),
            ten.pixels(),
            "a different window_s must change the picture — the span label reads it"
        );
    }
}
