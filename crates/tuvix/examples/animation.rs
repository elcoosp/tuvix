//! M13 motion showcase: the clock owns the value. The gallery rows, the
//! gauge and the headline fade are all `Tween`s driven by `Animator`s;
//! the render loop only draws when something actually moved and only
//! ticks while the animators report work — the architecture's static-CPU
//! budget, demoed live. Keys:
//! - `b` starts the burst — a fleet of finite tweens that settles and
//!   leaves the engine `idle` (the loop stops scheduling frames),
//! - `space` freezes/resumes the clock without dropping any state,
//! - `q` or `Ctrl+c` quits.
//!
//! Run with: cargo run -p tuvix --example animation

use std::io::Result;
use std::time::{Duration, Instant};

use tuvix::prelude::*;
use tuvix::{CrosstermEvents, try_init};

/// The frame cadence while something is moving.
const FRAME: Duration = Duration::from_millis(16);
/// The input poll while nothing is moving: the loop sleeps.
const IDLE_POLL: Duration = Duration::from_millis(120);
/// The bar rail width, chosen to fit an 80-column terminal.
const RAIL: usize = 58;

/// The easing catalog, live: one gallery row per curve.
const CATALOG: [(&str, Easing); 16] = [
    ("linear", Easing::Linear),
    ("inQuad", Easing::InQuad),
    ("outQuad", Easing::OutQuad),
    ("inOutQuad", Easing::InOutQuad),
    ("inCubic", Easing::InCubic),
    ("outCubic", Easing::OutCubic),
    ("inOutCubic", Easing::InOutCubic),
    ("outQuart", Easing::OutQuart),
    ("inOutQuart", Easing::InOutQuart),
    ("outSine", Easing::OutSine),
    ("inOutSine", Easing::InOutSine),
    ("outExpo", Easing::OutExpo),
    ("inOutExpo", Easing::InOutExpo),
    ("outBack", Easing::OutBack),
    ("outElastic", Easing::OutElastic),
    ("outBounce", Easing::OutBounce),
];

/// What the status line reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Something is in flight.
    Animating,
    /// Everything settled: the loop may sleep.
    Idle,
}

struct App {
    paused: bool,
    quit: bool,
    mode: Mode,
    /// The finite fleet behind the gallery rows.
    gallery: Animator<usize, f64>,
}

fn main() -> Result<()> {
    main_app()
}

fn main_app() -> Result<()> {
    let mut runner = try_init()?;

    let (mut reader, mut events) = CrosstermEvents::spawn()?;
    let mut app = App {
        paused: false,
        quit: false,
        mode: Mode::Idle,
        gallery: Animator::new(),
    };
    start_burst(&mut app.gallery);

    // The headline widgets loop for as long as the app runs.
    let mut animator: Animator<&'static str, f64> = Animator::new();
    animator.start(
        "gauge",
        Tween::new(0.0, 1.0, Duration::from_secs(4))
            .easing(Easing::InOutCubic)
            .repeat(Repeat::Infinite)
            .alternate(),
    );
    animator.start(
        "fade",
        Tween::new(0.0, 1.0, Duration::from_secs(3))
            .easing(Easing::InOutSine)
            .repeat(Repeat::Infinite)
            .alternate(),
    );

    let mut last = Instant::now();
    let mut needs_frame = true;

    while !app.quit {
        let poll = if needs_frame { FRAME } else { IDLE_POLL };
        pump_events(&mut app, &mut events, poll);

        let dt = last.elapsed();
        last = Instant::now();

        if !app.paused {
            animator.advance(dt);
            let settled = app.gallery.advance(dt);
            if !settled.is_empty() {
                // Report the settle frame once, then stop asking.
                needs_frame = true;
            }
            if animator.is_idle() && app.gallery.is_idle() {
                app.mode = Mode::Idle;
            } else {
                app.mode = Mode::Animating;
                needs_frame = true;
            }
        }

        if needs_frame {
            runner.draw(|frame| draw(frame, &animator, &app))?;
            needs_frame = false;
        }
    }

    reader.stop();
    runner.restore()?;
    Ok(())
}

/// The burst: every gallery row runs its own curve with a staggered
/// delay, all finite, so the engine settles exactly once.
fn start_burst(gallery: &mut Animator<usize, f64>) {
    for (index, (_, curve)) in CATALOG.iter().enumerate() {
        let delay = u64::try_from(index).unwrap_or(0) * 60;
        gallery.start(
            index,
            Tween::new(0.0, 1.0, Duration::from_millis(1_800))
                .easing(*curve)
                .delay(Duration::from_millis(delay)),
        );
    }
}

fn pump_events(
    app: &mut App,
    events: &mut tuvix_core::runtime::queue::EventStream,
    poll: Duration,
) {
    let deadline = Instant::now() + poll;
    while Instant::now() < deadline {
        match events.try_next() {
            Some(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
                match key.code {
                    KeyCode::Char('q') => app.quit = true,
                    KeyCode::Char('c') if ctrl => app.quit = true,
                    KeyCode::Char(' ') => app.paused = !app.paused,
                    KeyCode::Char('b') => start_burst(&mut app.gallery),
                    _ => {}
                }
            }
            Some(_) => {}
            None => break,
        }
    }
}

fn draw(frame: &mut Frame, animator: &Animator<&'static str, f64>, app: &App) {
    let area = frame.area();
    let [header, status, body, gauge_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(3),
    ])
    .areas(&area);

    // The headline: RGB colors fade through the animator, no snapping.
    let fade = animator.value(&"fade").unwrap_or(0.0);
    let dusk = Color::Rgb {
        r: 40,
        g: 60,
        b: 120,
    };
    let dawn = Color::Rgb {
        r: 150,
        g: 220,
        b: 255,
    };
    let headline = Color::lerp(dusk, dawn, fade);
    frame.buffer.set_string(
        2,
        header.y,
        "tuvix M13 — the clock owns the value (b burst, space pause, q quit)",
        Style::new().fg(headline).bold(),
    );

    let mode_line = match (app.paused, app.mode) {
        (true, _) => "paused — the clock is frozen, no frames are owed",
        (false, Mode::Idle) => "idle — nothing moves, the loop stopped scheduling frames",
        (false, Mode::Animating) => "animating — advance, draw, sleep",
    };
    let mode_style = if app.paused {
        Style::new().fg(Color::Yellow)
    } else if app.mode == Mode::Idle {
        Style::new().fg(Color::Green)
    } else {
        Style::new().fg(Color::Cyan)
    };
    frame.buffer.set_string(2, status.y, mode_line, mode_style);

    // The gallery: one row per curve, each bar eased by its own tween.
    let rows = CATALOG.len() as f64;
    let name_width: usize = 11;
    for (index, (name, _)) in CATALOG.iter().enumerate() {
        let row = u16::try_from(index).unwrap_or(0);
        if row >= body.height {
            break;
        }
        let value = app.gallery.value(&index).unwrap_or(0.0);
        let hue = Color::lerp(
            Color::Rgb {
                r: 60,
                g: 140,
                b: 200,
            },
            Color::Rgb {
                r: 130,
                g: 240,
                b: 180,
            },
            f64::from(row) / rows,
        );
        frame.buffer.set_string(
            2,
            body.y + row,
            format!("{name:>name_width$} "),
            Style::new().fg(Color::Gray),
        );
        let bar_width = usize::from(
            body.width
                .saturating_sub(u16::try_from(name_width + 4).unwrap_or(58))
                .min(58),
        )
        .min(RAIL);
        // Overshoot clamps at the rail; the shape of the settle is the
        // curve's signature.
        let fill = (value.clamp(0.0, 1.0) * bar_width as f64).round() as usize;
        let rail: String = "█".repeat(fill);
        let bar_x = u16::try_from(name_width + 3).unwrap_or(58);
        frame
            .buffer
            .set_string(bar_x, body.y + row, rail, Style::new().fg(hue));
    }

    // The gauge loops forever while the app runs.
    let gauge_value = animator.value(&"gauge").unwrap_or(0.0);
    Widget::render(
        Gauge::percent((gauge_value.clamp(0.0, 1.0) * 100.0).round() as u16)
            .block(Block::bordered().title("infinite loop — InOutCubic there and back")),
        gauge_area,
        &mut *frame.buffer,
    );
}
