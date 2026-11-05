//! The M10 application layer: a `TextArea` editor whose session replays
//! golden, and a clipboard write on the way out.
//!
//! This demo drives a headless `TestBackend` app (no terminal modes are
//! touched) through a short editing script — typing a note, popping a
//! mistake, splitting a line — recording every keystroke next to the
//! frames they produced. On exit it replays the session through the same
//! app logic (the golden loop: every replayed frame must match the
//! recorded one), prints the verdict, and hands the note's title to the
//! OSC 52 clipboard encoder — the sequence a real terminal would put on
//! the system clipboard.
//!
//! Run with: cargo run -p tuvix --example notes

use tuvix::prelude::*;

/// The app logic, shared by the recording phase and the replay phase —
/// the determinism contract replay verifies. Input handling lives in one
/// function so the two phases cannot drift.
fn handle(state: &mut TextAreaState, event: &Event) {
    if let Event::Key(key_event) = event {
        match key_event.code {
            KeyCode::Char(character) => state.insert(character),
            KeyCode::Enter => state.new_line(),
            KeyCode::Backspace => state.delete_backward(),
            KeyCode::Left => state.move_left(),
            KeyCode::Right => state.move_right(),
            KeyCode::Up => state.move_up(),
            KeyCode::Down => state.move_down(),
            KeyCode::Home => state.move_home(),
            KeyCode::End => state.move_end(),
            _ => {}
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = Terminal::new(TestBackend::new(Size::new(32, 6)))?;
    let session = SessionRecorder::unbounded();
    terminal.set_preview(session.clone());

    // The editing script: type a note (wide glyph included), pop the
    // mistake, split a line, keep typing.
    let key = |code: KeyCode| Event::Key(KeyEvent::new(code, KeyModifiers::empty()));
    let script: Vec<Event> = "note: "
        .chars()
        .map(|character| key(KeyCode::Char(character)))
        .chain(["汉", "!"].into_iter().map(|character| {
            key(KeyCode::Char(
                character.chars().next().expect("single char"),
            ))
        }))
        .chain([key(KeyCode::Backspace), key(KeyCode::Enter)])
        .chain(
            "second line"
                .chars()
                .map(|character| key(KeyCode::Char(character))),
        )
        .collect();

    // The recording phase: handle, record, draw — exactly what a real
    // event loop does.
    let mut state = TextAreaState::new();
    for event in &script {
        session.record_event(event.clone());
        handle(&mut state, event);
        let editor = TextArea::new().block(Block::bordered().title("notes — M10 TextArea"));
        terminal.draw(|frame| {
            let area = frame.area();
            editor.clone().render(area, frame.buffer, &mut state);
        })?;
    }

    // The replay phase: the same app logic AND the same paint
    // configuration, driven by the recording.
    let replay = SessionReplay::from_recorder(&session);
    let mut replayed = TextAreaState::new();
    let report = replay.run(|step| {
        for event in step.events {
            handle(&mut replayed, event);
        }
        let area = step.buffer.area;
        TextArea::new()
            .block(Block::bordered().title("notes — M10 TextArea"))
            .render(area, step.buffer, &mut replayed);
    })?;

    println!(
        "replay: {}/{} frames golden — {}",
        report.matched(),
        report.steps(),
        if report.is_golden() {
            "the session is its own regression test"
        } else {
            "DIVERGED: the recording names the frame"
        }
    );
    for mismatch in report.mismatches() {
        println!("  {mismatch}");
    }
    println!(
        "document after replay: {:?} ({} lines)",
        replayed.text(),
        replayed.lines().len()
    );

    // The clipboard: the note's title rides an OSC 52 sequence to the
    // terminal — the request a real terminal would honor.
    let sequence = encode_osc52(ClipboardSelection::System, "notes — M10")?;
    println!("clipboard sequence: {} bytes of OSC 52", sequence.len());

    Ok(())
}
