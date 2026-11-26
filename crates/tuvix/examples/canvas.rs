//! The float-space canvas: a themed braille plot over a live sine trace,
//! the M4 theme contract completed for the last widget family. The
//! canvas maps the `text` slot onto its labels (the theme base patches
//! under the app's span styles); shape colors stay app-owned, like chart
//! dataset colors. Press `m` to cycle the marker (braille, dot, block),
//! `q` to quit.
//!
//! Run with: cargo run -p tuvix --example canvas

use std::io::Result;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tuvix::prelude::*;
use tuvix::{CrosstermEvents, try_init};

use tuvix_widgets::canvas::{Line, Points};

/// The shared animation phase; the paint closure reads it every frame.
struct Trace {
    phase: Arc<Mutex<f64>>,
    marker: Marker,
}

impl Trace {
    fn new() -> Self {
        Self {
            phase: Arc::new(Mutex::new(0.0)),
            marker: Marker::Braille,
        }
    }

    /// Advances the wave; one full cycle every two seconds.
    fn tick(&mut self, dt: Duration) {
        let mut phase = self
            .phase
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *phase = (*phase + dt.as_secs_f64() * std::f64::consts::TAU / 2.0) % std::f64::consts::TAU;
    }

    /// The canvas: the themed base under the label spans, the wave dots
    /// in app-owned colors, the block themed by the app (the framework's
    /// split: the widget themes its own chrome, the app themes its block).
    fn canvas(&self) -> Canvas {
        let phase = Arc::clone(&self.phase);
        Canvas::new()
            .marker(self.marker)
            .x_bounds([0.0, 60.0])
            .y_bounds([-1.2, 1.2])
            .paint(move |ctx| {
                let phase = *phase
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let samples: Vec<(f64, f64)> = (0..=600)
                    .map(|step| {
                        let x = f64::from(step) * 0.1;
                        (x, (x * 0.25 + phase).sin() * 0.9)
                    })
                    .collect();
                ctx.draw(&Points {
                    coords: &samples,
                    color: Color::Cyan,
                });
                // The wave's current crest, marked in the app's own style.
                let crest_x = (phase / 0.25) % 60.0;
                ctx.draw(&Line::new(crest_x, -1.1, crest_x, 1.1, Color::DarkGray));
                ctx.print(1.0, 1.05, [Span::raw("sine (x*0.25 + phase)")]);
                ctx.print(
                    34.0,
                    1.05,
                    [Span::styled(" crest", Style::new().fg(Color::Cyan))],
                );
            })
            .theme(&Theme::dark())
    }
}

fn main() -> Result<()> {
    // One call: panic hook, raw mode, alternate screen. The runner
    // restores the terminal on drop, on error and on panic.
    let mut runner = try_init()?;

    let (mut reader, mut events) = CrosstermEvents::spawn()?;
    let mut app = Trace::new();
    let mut quit = false;
    let mut last = Instant::now();

    while !quit {
        let dt = last.elapsed();
        last = Instant::now();
        app.tick(dt);

        runner.draw(|frame| draw_plot(frame, &app))?;

        wait_for(&mut events, Duration::from_millis(16));
        while let Some(event) = events.try_next() {
            match event {
                Event::Key(key)
                    if key.kind == KeyEventKind::Press
                        && key.modifiers == KeyModifiers::empty() =>
                {
                    handle_key(&mut app, key.code, &mut quit);
                }
                _ => {}
            }
        }
    }

    reader.stop();
    runner.restore()?;
    Ok(())
}

/// The keyboard: `m` cycles the marker, `q` quits.
fn handle_key(app: &mut Trace, code: KeyCode, quit: &mut bool) {
    match code {
        KeyCode::Char('q') => *quit = true,
        KeyCode::Char('m') => {
            app.marker = match app.marker {
                Marker::Braille => Marker::Dot,
                Marker::Dot => Marker::Block,
                Marker::Block => Marker::Braille,
            };
        }
        _ => {}
    }
}

/// One frame: the header, the themed plot, the marker readout.
fn draw_plot(frame: &mut Frame<'_>, app: &Trace) {
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(&frame.area());

    frame
        .buffer
        .set_string(2, header.y, "tuvix", Style::new().fg(Color::Cyan).bold());
    frame.buffer.set_string(
        7,
        header.y,
        "- the themed canvas (M4 theme contract, every family)",
        Style::new().fg(Color::DarkGray),
    );

    let block = Block::bordered().title(" canvas ").theme(&Theme::dark());
    app.canvas().block(block).render(body, frame.buffer);

    let marker = match app.marker {
        Marker::Braille => "braille (8x sub-cell)",
        Marker::Dot => "dot (one glyph per cell)",
        Marker::Block => "block (shaded glyph)",
    };
    frame.buffer.set_string(
        4,
        footer.y,
        format!("{marker} - press m to cycle, q to quit"),
        Style::new().dim(),
    );
}

/// Waits until an event arrives or the deadline passes.
fn wait_for(events: &mut EventStream, deadline: Duration) {
    let started = Instant::now();
    while started.elapsed() < deadline {
        if !events.is_empty() {
            return;
        }
        std::thread::sleep(Duration::from_millis(4));
    }
}
