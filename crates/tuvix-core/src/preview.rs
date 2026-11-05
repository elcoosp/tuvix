//! IPC frame previews and frame exports: watch a running tuvix app from
//! another process, or dump frames as structured data.
//!
//! Three pieces, all std-only (zero new dependencies):
//!
//! 1. [`serialize_ansi`] — renders a [`Buffer`] to a full-frame ANSI string
//!    (cursor addressing, memoized SGR, OSC 8 hyperlinks, erase-line for
//!    blank tails). Pure and byte-assertable; also handy for snapshots and
//!    debug dumps.
//! 2. [`serialize_json`] — the structured frame export: a lossless JSON
//!    document of the cell grid (symbols, colors, modifiers, hyperlinks,
//!    covered cells) for tooling, golden tests and accessibility layers.
//! 3. [`PreviewTap`] — a named, stop-flagged, joinable broadcast thread.
//!    [`PreviewTap::tcp`] listens on `127.0.0.1:<port>` (works on every
//!    platform, Windows included); `unix_socket` (unix only) listens on a
//!    path. Any number of clients can connect; each receives every frame.
//!    Its [`PreviewSink`] handle implements [`FrameSink`], so the
//!    [`Terminal`](crate::terminal::Terminal) can stream frames to it
//!    through [`Terminal::set_preview`](crate::terminal::Terminal::set_preview).
//!
//! Watching an app is then just: `socat -,raw TCP:127.0.0.1:4223` or a
//! `telnet 127.0.0.1 4223` — whatever arrives is the app's screen.
//!
//! # Examples
//!
//! ```
//! use tuvix_core::buffer::Buffer;
//! use tuvix_core::geometry::Rect;
//! use tuvix_core::preview::serialize_ansi;
//! use tuvix_core::style::Style;
//!
//! let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 1));
//! buffer.set_string(0, 0, "hi", Style::new());
//! let ansi = serialize_ansi(&buffer);
//! // Cursor positioned at row 1, column 1, then the glyphs.
//! assert!(ansi.starts_with("\x1b[1;1H"));
//! assert!(ansi.contains("hi"));
//! ```

use std::fmt::Write as _;
use std::io;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::buffer::Buffer;
use crate::geometry::Position;
use crate::style::{Color, Hyperlink, Modifier, Style};

/// The ANSI control prefix: cursor position (`ESC[<row>;<col>H`).
fn cup(row: u16, col: u16) -> String {
    format!("\x1b[{};{}H", row + 1, col + 1)
}

/// Erase to end of line (`ESC[K`).
const EL: &str = "\x1b[K";

/// Full SGR reset (`ESC[0m`).
const SGR_RESET: &str = "\x1b[0m";

/// SGR parameters for a style's foreground/background/underline colors.
fn color_params(color: Color, foreground: bool) -> String {
    let base = if foreground { 30 } else { 40 };
    match color {
        Color::Reset => "39".to_string(), // resolved by the caller per role
        Color::Black => (base).to_string(),
        Color::Red => (base + 1).to_string(),
        Color::Green => (base + 2).to_string(),
        Color::Yellow => (base + 3).to_string(),
        Color::Blue => (base + 4).to_string(),
        Color::Magenta => (base + 5).to_string(),
        Color::Cyan => (base + 6).to_string(),
        Color::Gray => (base + 7).to_string(),
        Color::DarkGray => (base + 60).to_string(),
        Color::LightRed => (base + 61).to_string(),
        Color::LightGreen => (base + 62).to_string(),
        Color::LightYellow => (base + 63).to_string(),
        Color::LightBlue => (base + 64).to_string(),
        Color::LightMagenta => (base + 65).to_string(),
        Color::LightCyan => (base + 66).to_string(),
        Color::White => (base + 67).to_string(),
        Color::Indexed(index) => format!("{};5;{index}", if foreground { 38 } else { 48 }),
        Color::Rgb { r, g, b } => {
            format!("{};2;{r};{g};{b}", if foreground { 38 } else { 48 })
        }
    }
}

/// SGR parameter for the "default" color of a role (39 fg, 49 bg, 59
/// underline).
fn default_param(role: u8) -> String {
    (39 + u16::from(role) * 10).to_string()
}

/// SGR parameters for a modifier bit, on and off.
fn modifier_params(modifier: Modifier) -> (&'static str, &'static str) {
    if modifier.contains(Modifier::BOLD) {
        ("1", "22")
    } else if modifier.contains(Modifier::DIM) {
        ("2", "22")
    } else if modifier.contains(Modifier::ITALIC) {
        ("3", "23")
    } else if modifier.contains(Modifier::UNDERLINED) {
        ("4", "24")
    } else if modifier.contains(Modifier::SLOW_BLINK) {
        ("5", "25")
    } else if modifier.contains(Modifier::RAPID_BLINK) {
        ("6", "25")
    } else if modifier.contains(Modifier::REVERSED) {
        ("7", "27")
    } else if modifier.contains(Modifier::HIDDEN) {
        ("8", "28")
    } else if modifier.contains(Modifier::CROSSED_OUT) {
        ("9", "29")
    } else {
        ("", "")
    }
}

/// SGR sequence that sets exactly `style` from a clean slate: reset, then
/// every attribute the style carries.
fn sgr_for(style: Style) -> String {
    let mut params: Vec<String> = Vec::with_capacity(6);
    match style.fg {
        Some(color) => params.push(color_params(color, true)),
        None => params.push(default_param(0)),
    }
    match style.bg {
        Some(color) => params.push(color_params(color, false)),
        None => params.push(default_param(1)),
    }
    if let Some(color) = style.underline_color {
        params.push(match color {
            Color::Reset => "59".to_string(),
            other => color_params(other, false).replace("48;", "58;"),
        });
    }
    // The effective modifier set: additions minus explicit removals.
    let modifier = style.add_modifier.difference(style.remove_modifier);
    for bit in [
        Modifier::BOLD,
        Modifier::DIM,
        Modifier::ITALIC,
        Modifier::UNDERLINED,
        Modifier::SLOW_BLINK,
        Modifier::RAPID_BLINK,
        Modifier::REVERSED,
        Modifier::HIDDEN,
        Modifier::CROSSED_OUT,
    ] {
        if modifier.contains(bit) {
            let (on, _) = modifier_params(bit);
            params.push(on.to_string());
        }
    }
    format!("{SGR_RESET}\x1b[{}m", params.join(";"))
}

/// Opens an OSC 8 hyperlink (`ESC ] 8 ;; URI ST`): every glyph written
/// until the close sequence is clickable as `uri`.
fn hyperlink_open(uri: &str) -> String {
    format!("\x1b]8;;{uri}\x1b\\")
}

/// Closes the open OSC 8 hyperlink (`ESC ] 8 ;; ST`).
fn hyperlink_close() -> &'static str {
    "\x1b]8;;\x1b\\"
}

/// Serializes the buffer to a full-frame ANSI string: one cursor address per
/// row, glyphs written back to back with memoized SGR (reset+set on change),
/// OSC 8 hyperlink transitions emitted on change, covered (`skip`) cells
/// never written, and one erase-to-end-of-line for blank row tails.
///
/// This is a *snapshot* encoder — simpler and more verbose than the diff
/// encoder the crossterm backend uses for live output — because preview
/// frames are a cold path: one per draw, read by humans and debug tools.
pub fn serialize_ansi(buffer: &Buffer) -> String {
    let width = buffer.area.width;
    let height = buffer.area.height;
    let mut out = String::with_capacity(buffer.content.len() * 2);
    let mut current_style = Style::new();
    let mut current_link: Option<&Hyperlink> = None;
    for row in 0..height {
        out.push_str(&cup(row, 0));
        let row_offset = row as usize * width as usize;
        let mut col = 0;
        while col < width {
            let cell = &buffer.content[row_offset + col as usize];
            if cell.skip {
                // Covered by the wide glyph before it: never written.
                col += 1;
                continue;
            }
            // Blank tail: erase the rest of the row in one control. A
            // hyperlink is content (click state), so a linked tail is not
            // blank and must be walked cell by cell.
            let tail_blank = buffer.content[row_offset + col as usize..row_offset + width as usize]
                .iter()
                .all(|c| {
                    c.skip
                        || (c.symbol.is_empty() && c.style == Style::new() && c.hyperlink.is_none())
                });
            if tail_blank && cell.symbol.is_empty() {
                out.push_str(EL);
                break;
            }
            if cell.style != current_style {
                out.push_str(&sgr_for(cell.style));
                current_style = cell.style;
            }
            if cell.hyperlink.as_ref() != current_link {
                match &cell.hyperlink {
                    Some(uri) => {
                        if current_link.is_some() {
                            out.push_str(hyperlink_close());
                        }
                        out.push_str(&hyperlink_open(uri.as_str()));
                    }
                    None => out.push_str(hyperlink_close()),
                }
                current_link = cell.hyperlink.as_ref();
            }
            out.push_str(cell.symbol.as_str());
            col += 1;
        }
    }
    if current_link.is_some() {
        out.push_str(hyperlink_close());
    }
    out.push_str(SGR_RESET);
    out
}

/// Escapes `text` as the body of a JSON string (no surrounding quotes):
/// the two mandatory escapes, the short escapes and `\u00xx` for every
/// other control character. Non-ASCII text passes through as UTF-8.
fn json_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            character if (character as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", character as u32);
            }
            character => out.push(character),
        }
    }
    out
}

/// JSON representation of an optional color: `null`, a named string, or an
/// indexed/RGB object.
fn color_json(color: Option<Color>) -> String {
    let Some(color) = color else {
        return "null".to_string();
    };
    match color {
        Color::Reset => "\"reset\"".to_string(),
        Color::Black => "\"black\"".to_string(),
        Color::Red => "\"red\"".to_string(),
        Color::Green => "\"green\"".to_string(),
        Color::Yellow => "\"yellow\"".to_string(),
        Color::Blue => "\"blue\"".to_string(),
        Color::Magenta => "\"magenta\"".to_string(),
        Color::Cyan => "\"cyan\"".to_string(),
        Color::Gray => "\"gray\"".to_string(),
        Color::DarkGray => "\"dark_gray\"".to_string(),
        Color::LightRed => "\"light_red\"".to_string(),
        Color::LightGreen => "\"light_green\"".to_string(),
        Color::LightYellow => "\"light_yellow\"".to_string(),
        Color::LightBlue => "\"light_blue\"".to_string(),
        Color::LightMagenta => "\"light_magenta\"".to_string(),
        Color::LightCyan => "\"light_cyan\"".to_string(),
        Color::White => "\"white\"".to_string(),
        Color::Indexed(index) => format!("{{\"indexed\":{index}}}"),
        Color::Rgb { r, g, b } => format!("{{\"rgb\":[{r},{g},{b}]}}"),
    }
}

/// JSON representation of a modifier set: the effective bits (additions
/// minus removals) as uppercase names joined by `|`, in bit order.
fn modifiers_json(style: Style) -> String {
    let modifier = style.add_modifier.difference(style.remove_modifier);
    let names: Vec<&str> = [
        (Modifier::BOLD, "BOLD"),
        (Modifier::DIM, "DIM"),
        (Modifier::ITALIC, "ITALIC"),
        (Modifier::UNDERLINED, "UNDERLINED"),
        (Modifier::SLOW_BLINK, "SLOW_BLINK"),
        (Modifier::RAPID_BLINK, "RAPID_BLINK"),
        (Modifier::REVERSED, "REVERSED"),
        (Modifier::HIDDEN, "HIDDEN"),
        (Modifier::CROSSED_OUT, "CROSSED_OUT"),
    ]
    .into_iter()
    .filter(|(bit, _)| modifier.contains(*bit))
    .map(|(_, name)| name)
    .collect();
    names.join("|")
}

/// Serializes the buffer to a **structured frame export**: a compact,
/// deterministic, lossless JSON document.
///
/// Unlike [`serialize_ansi`] (a terminal-ready snapshot), the JSON export is
/// for tooling: golden tests, diffing frames, feeding a renderer or an
/// accessibility layer. The format is versioned by documentation and
/// pinned by tests:
///
/// ```json
/// {"area":{"x":0,"y":0,"width":2,"height":1},"cells":[
///   {"x":0,"y":0,"symbol":"汉","fg":"red","bg":null,
///    "underline_color":null,"modifiers":"","hyperlink":null},
///   {"x":1,"y":0,"covered":true}]}
/// ```
///
/// Cells are row-major with absolute (buffer-space) coordinates matching
/// [`Buffer::get`]. Non-covered cells carry every field (`null` when
/// unset, `""` for no modifiers); covered cells — the trailing half of a
/// wide glyph, invisible by definition — reduce to
/// `{"x":…,"y":…,"covered":true}`. Colors export as named strings,
/// `{"indexed":n}` or `{"rgb":[r,g,b]}`; hyperlinks as plain URI strings.
///
/// # Examples
///
/// ```
/// use tuvix_core::buffer::Buffer;
/// use tuvix_core::geometry::Rect;
/// use tuvix_core::preview::serialize_json;
/// use tuvix_core::style::{Color, Style};
///
/// let mut buffer = Buffer::empty(Rect::new(0, 0, 2, 1));
/// buffer.set_string(0, 0, "h", Style::new().fg(Color::Red));
/// let json = serialize_json(&buffer);
/// assert!(json.starts_with("{\"area\":{\"x\":0,\"y\":0,\"width\":2,\"height\":1}"));
/// assert!(json.contains("\"symbol\":\"h\",\"fg\":\"red\""));
/// ```
pub fn serialize_json(buffer: &Buffer) -> String {
    let mut out = String::with_capacity(buffer.content.len() * 64 + 48);
    out.push_str("{\"area\":{\"x\":");
    out.push_str(&buffer.area.x.to_string());
    out.push_str(",\"y\":");
    out.push_str(&buffer.area.y.to_string());
    out.push_str(",\"width\":");
    out.push_str(&buffer.area.width.to_string());
    out.push_str(",\"height\":");
    out.push_str(&buffer.area.height.to_string());
    out.push_str("},\"cells\":[");
    let width = usize::from(buffer.area.width.max(1));
    for (index, cell) in buffer.content.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let position = Position::new(
            buffer.area.x + u16::try_from(index % width).unwrap_or(u16::MAX),
            buffer.area.y + u16::try_from(index / width).unwrap_or(u16::MAX),
        );
        if cell.skip {
            let _ = write!(
                out,
                "{{\"x\":{},\"y\":{},\"covered\":true}}",
                position.x, position.y
            );
            continue;
        }
        out.push_str("{\"x\":");
        out.push_str(&position.x.to_string());
        out.push_str(",\"y\":");
        out.push_str(&position.y.to_string());
        out.push_str(",\"symbol\":\"");
        out.push_str(&json_escape(cell.symbol.as_str()));
        out.push_str("\",\"fg\":");
        out.push_str(&color_json(cell.style.fg));
        out.push_str(",\"bg\":");
        out.push_str(&color_json(cell.style.bg));
        out.push_str(",\"underline_color\":");
        out.push_str(&color_json(cell.style.underline_color));
        out.push_str(",\"modifiers\":\"");
        out.push_str(&modifiers_json(cell.style));
        out.push_str("\",\"hyperlink\":");
        match &cell.hyperlink {
            Some(uri) => {
                out.push('"');
                out.push_str(&json_escape(uri.as_str()));
                out.push('"');
            }
            None => out.push_str("null"),
        }
        out.push('}');
    }
    out.push_str("]}");
    out
}

/// Receives serialized preview frames. Implement this to forward frames
/// anywhere: a socket, a file, a test recorder.
pub trait FrameSink: Send {
    /// Called after each flushed frame with the full-frame ANSI snapshot.
    fn on_frame(&mut self, ansi: &str);
}

enum Command {
    Frame(String),
    Stop,
}

/// A broadcast tap: every connected client receives every frame.
///
/// The tap owns a named, stop-flagged worker thread (the M2 thread
/// contract: named, stoppable, joinable). Dropping the tap sends the stop
/// command and joins the thread; dropping the [`PreviewSink`] handle alone
/// only disconnects that sender.
pub struct PreviewTap {
    commands: Sender<Command>,
    worker: Option<JoinHandle<()>>,
    local_addr: Option<SocketAddr>,
}

impl std::fmt::Debug for PreviewTap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreviewTap")
            .field("local_addr", &self.local_addr)
            .finish_non_exhaustive()
    }
}

impl PreviewTap {
    /// Listens on `127.0.0.1:port` (pass `0` for an ephemeral port) and
    /// serves frames to any client that connects. Works on every platform
    /// the std supports, Windows included.
    ///
    /// # Errors
    ///
    /// Fails when the listener cannot bind the address.
    pub fn tcp(port: u16) -> io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        listener.set_nonblocking(true)?;
        let local_addr = listener.local_addr().ok();
        let mut tap = Self::spawn(Box::new(listener));
        tap.local_addr = local_addr;
        Ok(tap)
    }

    /// Listens on a unix domain socket at `path` (unix only).
    ///
    /// # Errors
    ///
    /// Fails when the listener cannot bind the path (an existing socket
    /// file at the same path also fails the bind).
    #[cfg(unix)]
    pub fn unix_socket<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        use std::os::unix::net::UnixListener;
        let listener = UnixListener::bind(path)?;
        listener.set_nonblocking(true)?;
        Ok(Self::spawn(Box::new(listener)))
    }

    /// The bound address (TCP only; `None` for unix sockets).
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.local_addr
    }

    /// A cloneable [`FrameSink`] that forwards frames into the tap's
    /// broadcast thread. Hand this to
    /// [`Terminal::set_preview`](crate::terminal::Terminal::set_preview).
    pub fn sink(&self) -> PreviewSink {
        PreviewSink {
            commands: self.commands.clone(),
        }
    }

    /// Spawns the broadcast worker over any non-blocking listener.
    fn spawn(listener: Box<dyn NonBlockingListener>) -> Self {
        let (commands, inbox) = mpsc::channel::<Command>();
        let worker = thread::Builder::new()
            .name("tuvix-preview".to_string())
            .spawn(move || preview_worker(listener, &inbox))
            .expect("failed to spawn the preview worker thread");
        Self {
            commands,
            worker: Some(worker),
            local_addr: None,
        }
    }

    /// Stops the worker and joins it. Called on drop; also callable to
    /// stop early and observe completion.
    pub fn stop(&mut self) {
        let _ = self.commands.send(Command::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for PreviewTap {
    fn drop(&mut self) {
        self.stop();
    }
}

/// A client-agnostic non-blocking listener: TCP on all platforms, unix
/// sockets on unix.
trait NonBlockingListener: Send {
    /// Accepts one pending connection, if any (non-blocking).
    fn accept(&mut self) -> io::Result<Box<dyn BroadcastTarget>>;
}

trait BroadcastTarget: Send {
    /// Writes the bytes; an error drops the client.
    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()>;
}

impl NonBlockingListener for TcpListener {
    fn accept(&mut self) -> io::Result<Box<dyn BroadcastTarget>> {
        // Fully qualified: inside this impl the trait method shadows the
        // inherent one in name resolution.
        let accepted: io::Result<(TcpStream, SocketAddr)> = TcpListener::accept(self);
        match accepted {
            Ok((stream, _)) => {
                let _ = stream.set_nonblocking(true);
                let _ = stream.set_nodelay(true);
                Ok(Box::new(stream))
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Err(error),
            Err(error) => Err(error),
        }
    }
}

impl BroadcastTarget for TcpStream {
    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        // Preview frames are small; a blocking write with a generous
        // timeout keeps slow terminals from dropping frames entirely.
        self.set_nonblocking(false)?;
        let _ = self.set_write_timeout(Some(Duration::from_millis(500)));
        std::io::Write::write_all(self, bytes)?;
        std::io::Write::flush(self)
    }
}

#[cfg(unix)]
impl NonBlockingListener for std::os::unix::net::UnixListener {
    fn accept(&mut self) -> io::Result<Box<dyn BroadcastTarget>> {
        let accepted = io::Result::from(std::os::unix::net::UnixListener::accept(self));
        match accepted {
            Ok((stream, _)) => {
                let _ = stream.set_nonblocking(true);
                Ok(Box::new(stream))
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Err(error),
            Err(error) => Err(error),
        }
    }
}

#[cfg(unix)]
impl BroadcastTarget for std::os::unix::net::UnixStream {
    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.set_nonblocking(false)?;
        let _ = self.set_write_timeout(Some(Duration::from_millis(500)));
        std::io::Write::write_all(self, bytes)?;
        std::io::Write::flush(self)
    }
}

/// Accepts every pending connection into `clients`.
fn accept_pending(
    listener: &mut Box<dyn NonBlockingListener>,
    clients: &mut Vec<Box<dyn BroadcastTarget>>,
) {
    while let Ok(client) = listener.accept() {
        clients.push(client);
    }
}

/// The worker loop: serve queued frames until the stop command arrives.
/// Pending accepts are drained before every broadcast, so a client that
/// connected just before a frame is sent still receives it.
fn preview_worker(mut listener: Box<dyn NonBlockingListener>, inbox: &mpsc::Receiver<Command>) {
    let mut clients: Vec<Box<dyn BroadcastTarget>> = Vec::new();
    loop {
        match inbox.recv_timeout(Duration::from_millis(250)) {
            Ok(Command::Frame(frame)) => {
                // Connect-and-send races resolve here: anything pending is
                // accepted before the bytes go out.
                accept_pending(&mut listener, &mut clients);
                let bytes = frame.as_bytes();
                // Best-effort broadcast; a failed (slow, closed) client is
                // dropped without disturbing the others.
                clients.retain_mut(|client| client.write_all(bytes).is_ok());
            }
            Ok(Command::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => accept_pending(&mut listener, &mut clients),
        }
    }
}

/// A cloneable [`FrameSink`] forwarding frames to a [`PreviewTap`].
#[derive(Debug, Clone)]
pub struct PreviewSink {
    commands: Sender<Command>,
}

impl FrameSink for PreviewSink {
    fn on_frame(&mut self, ansi: &str) {
        // A dead tap simply stops receiving; never panic from a preview.
        let _ = self.commands.send(Command::Frame(ansi.to_string()));
    }
}

/// A recording sink for tests and debugging: appends every frame to the
/// shared log.
#[derive(Debug, Clone, Default)]
pub struct RecordingSink {
    frames: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl RecordingSink {
    /// Creates a recorder with an empty log.
    pub fn new() -> Self {
        Self::default()
    }

    /// The frames recorded so far, in order.
    pub fn frames(&self) -> std::sync::Arc<std::sync::Mutex<Vec<String>>> {
        std::sync::Arc::clone(&self.frames)
    }
}

impl FrameSink for RecordingSink {
    fn on_frame(&mut self, ansi: &str) {
        self.frames.lock().unwrap().push(ansi.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Rect;
    use crate::style::Color;

    #[test]
    fn plain_text_snapshot_is_cup_then_glyphs() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 2));
        buffer.set_string(0, 0, "hi", Style::new());
        let ansi = serialize_ansi(&buffer);
        assert!(ansi.starts_with("\x1b[1;1H"));
        assert!(ansi.contains("hi"));
        // Row 2 addressed too.
        assert!(ansi.contains("\x1b[2;1H"));
        // Ends with a full SGR reset.
        assert!(ansi.ends_with("\x1b[0m"));
    }

    #[test]
    fn styled_cells_emit_sgr_and_reset() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 1));
        buffer.set_string(
            0,
            0,
            "x",
            Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
        );
        let ansi = serialize_ansi(&buffer);
        // Reset, fg red (31), bold (1).
        assert!(ansi.contains("\x1b[0m\x1b[39;49;1m") || ansi.contains("\x1b[0m\x1b[31;49;1m"));
    }

    #[test]
    fn blank_tails_collapse_to_one_erase() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 1));
        buffer.set_string(0, 0, "ab", Style::new());
        let ansi = serialize_ansi(&buffer);
        // The 8 blank cells after "ab" become one EL, not 8 writes.
        assert_eq!(ansi.matches(EL).count(), 1);
        assert!(ansi.contains("ab"));
        // No blank glyph writes at all.
        assert!(!ansi.contains("          "));
    }

    #[test]
    fn covered_cells_are_never_written() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 1));
        buffer.set_string(0, 0, "汉", Style::new());
        let ansi = serialize_ansi(&buffer);
        assert!(ansi.contains("汉"));
        // The wide glyph is written exactly once.
        assert_eq!(ansi.matches("汉").count(), 1);
    }

    #[test]
    fn indexed_and_rgb_colors_map_to_sgr() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 4, 1));
        buffer.set_string(0, 0, "a", Style::new().fg(Color::Indexed(196)));
        buffer.set_string(1, 0, "b", Style::new().fg(Color::Rgb { r: 1, g: 2, b: 3 }));
        buffer.set_string(2, 0, "c", Style::new().bg(Color::Indexed(21)));
        let ansi = serialize_ansi(&buffer);
        assert!(ansi.contains("38;5;196"));
        assert!(ansi.contains("38;2;1;2;3"));
        assert!(ansi.contains("48;5;21"));
    }

    #[test]
    fn underline_color_maps_to_58() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 4, 1));
        buffer.set_string(0, 0, "u", Style::new().underline_color(Color::Indexed(9)));
        assert!(serialize_ansi(&buffer).contains("58;5;9"));
    }

    #[test]
    fn hyperlinks_emit_osc8_transitions() {
        use crate::style::Hyperlink;
        let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 1));
        buffer.set_string_with_hyperlink(
            0,
            0,
            "ab",
            Style::new(),
            Hyperlink::new("https://tuvix.dev"),
        );
        buffer.set_string(2, 0, "c", Style::new());
        let ansi = serialize_ansi(&buffer);
        let open = "\x1b]8;;https://tuvix.dev\x1b\\";
        let close = "\x1b]8;;\x1b\\";
        assert!(ansi.contains(open), "open missing: {ansi:?}");
        // The link opens before the glyphs and closes after the segment.
        assert!(
            ansi.contains(&format!("{open}ab{close}c")),
            "transition wrong: {ansi:?}"
        );
    }

    mod json {
        use super::*;
        use crate::style::Hyperlink;

        #[test]
        fn empty_buffer_exports_area_and_blank_cells() {
            let buffer = Buffer::empty(Rect::new(0, 0, 2, 1));
            assert_eq!(
                serialize_json(&buffer),
                "{\"area\":{\"x\":0,\"y\":0,\"width\":2,\"height\":1},\"cells\":[".to_string()
                    + "{\"x\":0,\"y\":0,\"symbol\":\"\",\"fg\":null,\"bg\":null,"
                    + "\"underline_color\":null,\"modifiers\":\"\",\"hyperlink\":null},"
                    + "{\"x\":1,\"y\":0,\"symbol\":\"\",\"fg\":null,\"bg\":null,"
                    + "\"underline_color\":null,\"modifiers\":\"\",\"hyperlink\":null}]}"
            );
        }

        #[test]
        fn styled_cell_exports_colors_and_modifiers() {
            let mut buffer = Buffer::empty(Rect::new(0, 0, 1, 1));
            buffer.set_string(
                0,
                0,
                "x",
                Style::new()
                    .fg(Color::Red)
                    .bg(Color::Indexed(21))
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            );
            assert_eq!(
                serialize_json(&buffer),
                "{\"area\":{\"x\":0,\"y\":0,\"width\":1,\"height\":1},\"cells\":[".to_string()
                    + "{\"x\":0,\"y\":0,\"symbol\":\"x\",\"fg\":\"red\","
                    + "\"bg\":{\"indexed\":21},\"underline_color\":null,"
                    + "\"modifiers\":\"BOLD|UNDERLINED\",\"hyperlink\":null}]}"
            );
        }

        #[test]
        fn rgb_colors_export_as_arrays() {
            let mut buffer = Buffer::empty(Rect::new(0, 0, 1, 1));
            buffer.set_string(0, 0, "x", Style::new().fg(Color::Rgb { r: 1, g: 2, b: 3 }));
            let json = serialize_json(&buffer);
            assert!(json.contains("\"fg\":{\"rgb\":[1,2,3]}"), "{json}");
        }

        #[test]
        fn covered_cells_export_as_covered_only() {
            let mut buffer = Buffer::empty(Rect::new(0, 0, 2, 1));
            buffer.set_string(0, 0, "汉", Style::new());
            let json = serialize_json(&buffer);
            assert!(
                json.contains("{\"x\":1,\"y\":0,\"covered\":true}"),
                "covered cell missing: {json}"
            );
            // The covered cell carries no content fields at all.
            assert!(!json.contains("{\"x\":1,\"y\":0,\"symbol\""), "{json}");
        }

        #[test]
        fn hyperlinks_export_as_strings() {
            let mut buffer = Buffer::empty(Rect::new(0, 0, 1, 1));
            buffer.set_string_with_hyperlink(
                0,
                0,
                "x",
                Style::new(),
                Hyperlink::new("https://tuvix.dev"),
            );
            let json = serialize_json(&buffer);
            assert!(
                json.contains("\"hyperlink\":\"https://tuvix.dev\""),
                "{json}"
            );
        }

        #[test]
        fn strings_are_json_escaped() {
            // set_string filters control characters, so inject one straight
            // into the cell: the export must still be valid JSON.
            let mut buffer = Buffer::empty(Rect::new(0, 0, 1, 1));
            buffer.get_mut(0, 0).unwrap().set_symbol("\u{1}");
            let json = serialize_json(&buffer);
            assert!(json.contains("\\u0001"), "{json}");

            let mut buffer = Buffer::empty(Rect::new(0, 0, 3, 1));
            buffer.set_string(0, 0, "\"\\/", Style::new());
            let json = serialize_json(&buffer);
            assert!(json.contains("\"symbol\":\"\\\"\""), "{json}"); // quote
            assert!(json.contains("\"symbol\":\"\\\\\""), "{json}"); // backslash
            assert!(json.contains("\"symbol\":\"/\""), "{json}"); // passthrough
        }

        #[test]
        fn area_offsets_are_preserved() {
            let buffer = Buffer::empty(Rect::new(3, 4, 2, 1));
            let json = serialize_json(&buffer);
            assert!(
                json.contains("\"area\":{\"x\":3,\"y\":4,\"width\":2,\"height\":1}"),
                "{json}"
            );
            // Cell coordinates are absolute (buffer-space), matching get().
            assert!(json.contains("{\"x\":3,\"y\":4,\"symbol\":\"\""), "{json}");
        }
    }

    #[test]
    fn hyperlink_tails_are_not_elided_in_previews() {
        use crate::style::Hyperlink;
        let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 1));
        buffer.set_string(0, 0, "ab", Style::new());
        buffer
            .set_hyperlink(4, 0, Some(Hyperlink::new("https://tuvix.dev")))
            .unwrap();
        let ansi = serialize_ansi(&buffer);
        // The linked blank cell is written after an open, not swallowed by
        // an EL: the row tail is content.
        assert!(ansi.contains("\x1b]8;;https://tuvix.dev\x1b\\"));
        // Exactly one EL can still appear for the truly blank cells after
        // the link cell — but only *after* it (or none at all).
        if let Some(el_index) = ansi.find(EL) {
            let link_index = ansi.find("\x1b]8;;https://tuvix.dev").unwrap();
            assert!(el_index > link_index, "EL must follow the link: {ansi:?}");
        }
    }

    #[test]
    fn tcp_tap_broadcasts_to_connected_clients() {
        let mut tap = PreviewTap::tcp(0).unwrap();
        let port = tap.local_addr().unwrap().port();

        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        // Give the worker a moment to accept the client.
        thread::sleep(Duration::from_millis(150));

        let mut sink = tap.sink();
        sink.on_frame("\x1b[1;1Hhello");
        // The frame reaches the client.
        let mut buffer = [0u8; 32];
        let read = std::io::Read::read(&mut client, &mut buffer).unwrap();
        assert_eq!(&buffer[..read], b"\x1b[1;1Hhello");

        tap.stop();
    }

    #[test]
    fn recording_sink_collects_frames_in_order() {
        let mut sink = RecordingSink::new();
        sink.on_frame("one");
        sink.on_frame("two");
        let frames = sink.frames().lock().unwrap().clone();
        assert_eq!(frames, vec!["one".to_string(), "two".to_string()]);
    }

    #[cfg(unix)]
    #[test]
    fn unix_socket_tap_serves_frames() {
        use std::os::unix::net::UnixStream;
        let path = std::env::temp_dir().join("tuvix-preview-test.sock");
        let _ = std::fs::remove_file(&path);
        let mut tap = PreviewTap::unix_socket(&path).unwrap();

        let mut client = UnixStream::connect(&path).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        thread::sleep(Duration::from_millis(150));

        let mut sink = tap.sink();
        sink.on_frame("\x1b[1;1Hunix");
        let mut buffer = [0u8; 32];
        let read = std::io::Read::read(&mut client, &mut buffer).unwrap();
        assert_eq!(&buffer[..read], b"\x1b[1;1Hunix");

        tap.stop();
        let _ = std::fs::remove_file(&path);
    }
}
