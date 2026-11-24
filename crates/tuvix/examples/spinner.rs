//! The M20 animated indicator: three spinners over one dashboard, the
//! M13 elapsed-time clock discipline in a real loop. Press `s` to stop
//! and resume the clocks (the frames freeze in place), `q` to quit.
//!
//! Run with: cargo run -p tuvix --example spinner

use std::io::Result;
use std::time::{Duration, Instant};

use tuvix::prelude::*;
use tuvix::{CrosstermEvents, try_init};

/// The three clocks and their pause state; the spinners are the paint.
struct Dashboard {
    upload: Spinner,
    sync: Spinner,
    scan: Spinner,
    clocks: [SpinnerState; 3],
    paused: bool,
}

impl Dashboard {
    fn new() -> Self {
        Self {
            upload: Spinner::dots().label("uploading").theme(&Theme::dark()),
            sync: Spinner::arc().label("syncing").theme(&Theme::dark()),
            scan: Spinner::toggle().label("scanning").theme(&Theme::dark()),
            clocks: [
                SpinnerState::new(),
                SpinnerState::new(),
                SpinnerState::new(),
            ],
            paused: false,
        }
    }

    /// One frame of elapsed time for every clock.
    fn tick(&mut self, dt: Duration) {
        for clock in &mut self.clocks {
            clock.advance(dt);
        }
    }

    /// Stop freezes every clock in place; resume continues the frame.
    fn toggle_pause(&mut self) {
        self.paused = !self.paused;
        for clock in &mut self.clocks {
            if self.paused {
                clock.stop();
            } else {
                clock.start();
            }
        }
    }
}

fn main() -> Result<()> {
    // One call: panic hook, raw mode, alternate screen. The runner
    // restores the terminal on drop, on error and on panic.
    let mut runner = try_init()?;

    let (mut reader, mut events) = CrosstermEvents::spawn()?;
    let mut app = Dashboard::new();
    let mut quit = false;
    let mut last = Instant::now();

    while !quit {
        let dt = last.elapsed();
        last = Instant::now();
        app.tick(dt);

        runner.draw(|frame| draw_dashboard(frame, &mut app))?;

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

/// The keyboard: `s` stops/resumes the clocks, `q` quits.
fn handle_key(app: &mut Dashboard, code: KeyCode, quit: &mut bool) {
    match code {
        KeyCode::Char('q') => *quit = true,
        KeyCode::Char('s') => app.toggle_pause(),
        _ => {}
    }
}

/// One frame: the header, the three indicator rows, the clock readout.
fn draw_dashboard(frame: &mut Frame<'_>, app: &mut Dashboard) {
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
        "- the animated indicator (M20)",
        Style::new().fg(Color::DarkGray),
    );

    for (index, row) in [4_u16, 6, 8].into_iter().enumerate() {
        let row_area = Rect::new(4, body.y + row, 30, 1);
        let painter = [&app.upload, &app.sync, &app.scan][index];
        painter
            .clone()
            .render(row_area, frame.buffer, &mut app.clocks[index]);
    }

    let elapsed = app.clocks[0].elapsed().as_secs_f32();
    frame.buffer.set_string(
        4,
        body.y + 10,
        format!("upload clock: {elapsed:.1}s"),
        Style::new().fg(Color::DarkGray),
    );
    let (banner, color) = if app.paused {
        ("stopped", Color::Yellow)
    } else {
        ("running", Color::Green)
    };
    frame
        .buffer
        .set_string(4, body.y + 12, banner, Style::new().fg(color));

    frame.buffer.set_string(
        2,
        footer.y,
        "press s to stop/resume, q to quit",
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
