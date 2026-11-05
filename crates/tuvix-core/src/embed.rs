//! The embedding boundary: a flat, versioned binary mutation protocol.
//!
//! A host process (another language, another machine, a test harness)
//! drives a tuvix [`Buffer`] through a *mutation packet*: one little-endian
//! byte string carrying a batch of buffer mutations. The design follows the
//! architecture's §5.1 mechanism — mutations are batched into a single flat
//! packet (one boundary crossing, not one per property) — and the §8 gate 1
//! budget (more than 50,000 mutations per second through the boundary).
//!
//! # The packet format
//!
//! ```text
//! magic "TVX" | version u8 (= 1) | op count u32 LE
//! op          ::= opcode u8 + payload (all integers little-endian)
//!   0x01 SET_TEXT  x u16 | y u16 | len u16 | text bytes (UTF-8) | style
//!   0x02 SET_STYLE x u16 | y u16 | w u16 | h u16 | style
//!   0x03 FILL      x u16 | y u16 | w u16 | h u16 | symbol u8 | style
//!   0x04 CLEAR     x u16 | y u16 | w u16 | h u16
//!   0x05 HYPERLINK x u16 | y u16 | w u16 | h u16 | len u16 | uri bytes
//!   0x06 RESIZE    w u16 | h u16
//! style        ::= flags u8 + present fields
//!   flags bit 0: fg, bit 1: bg, bit 2: underline color,
//!   bit 3: add modifier u16 LE, bit 4: remove modifier u16 LE,
//!   bits 5-7: reserved (must be zero)
//! color        ::= kind u8 + payload
//!   0 Reset | 1 Indexed + u8 | 2 Rgb + 3 bytes | 3 named + u8 (0-15)
//! ```
//!
//! One op carries at most 64 KiB of text or URI bytes (the u16 length
//! prefix is the contract); longer content is the host's job to split.
//!
//! # Trust boundary discipline
//!
//! The parser is strict — the M9 [`Screen`](crate::screen::Screen)
//! philosophy applied to bytes: a packet that cannot be trusted is rejected
//! with the offending byte offset, never guessed at. Text and URIs must be
//! valid UTF-8 free of C0 control characters and DEL (the M5 framing
//! argument: no byte that crosses the boundary can break the escape
//! sequences tuvix later emits), fill symbols must be single printable
//! ASCII columns, and reserved bits must be zero so the format can grow
//! without ambiguity.
//!
//! # Examples
//!
//! ```
//! use tuvix_core::buffer::Buffer;
//! use tuvix_core::embed::{Mutation, PacketWriter, parse_packets};
//! use tuvix_core::geometry::Rect;
//! use tuvix_core::style::Style;
//!
//! // The host side: batch three mutations into one packet.
//! let packet = PacketWriter::new()
//!     .push(&Mutation::Resize { width: 20, height: 3 })
//!     .push(&Mutation::SetText {
//!         x: 0, y: 1,
//!         text: "driven from outside".to_owned(),
//!         style: Style::new(),
//!     })
//!     .push(&Mutation::ClearRegion { area: Rect::new(0, 2, 20, 1) })
//!     .finish();
//!
//! // The embed side: parse strictly, then apply.
//! let mutations = parse_packets(&packet).unwrap();
//! let mut buffer = Buffer::empty(Rect::new(0, 0, 20, 3));
//! for mutation in &mutations {
//!     mutation.apply(&mut buffer);
//! }
//! assert_eq!(buffer.get(0, 1).unwrap().symbol.as_str(), "d");
//! ```

use crate::buffer::{Buffer, Cell, Symbol};
use crate::geometry::Rect;
use crate::style::{Color, Hyperlink, Modifier, Style};

/// The packet magic: every packet and snapshot starts with these bytes.
pub const PACKET_MAGIC: [u8; 3] = *b"TVX";

/// The mutation packet version.
pub const PACKET_VERSION: u8 = 1;

/// The frame snapshot version (the outbound read-back format).
pub const SNAPSHOT_VERSION: u8 = 2;

/// Wire opcodes.
mod op {
    pub const SET_TEXT: u8 = 0x01;
    pub const SET_STYLE: u8 = 0x02;
    pub const FILL: u8 = 0x03;
    pub const CLEAR: u8 = 0x04;
    pub const HYPERLINK: u8 = 0x05;
    pub const RESIZE: u8 = 0x06;
}

/// Style flag bits (bits 5-7 are reserved and must be zero on the wire).
mod style_flags {
    pub const FG: u8 = 1 << 0;
    pub const BG: u8 = 1 << 1;
    pub const UNDERLINE: u8 = 1 << 2;
    pub const ADD_MODIFIER: u8 = 1 << 3;
    pub const REMOVE_MODIFIER: u8 = 1 << 4;
    pub const RESERVED: u8 = 0b1110_0000;
}

/// Snapshot cell flag bits.
mod cell_flags {
    /// The cell is the covered half of a wide glyph.
    pub const SKIP: u8 = 1 << 0;
    /// The cell carries an OSC 8 hyperlink (u16 length + bytes follow).
    pub const HYPERLINK: u8 = 1 << 1;
}

/// Color kind discriminants on the wire.
mod color_kind {
    pub const RESET: u8 = 0;
    pub const INDEXED: u8 = 1;
    pub const RGB: u8 = 2;
    pub const NAMED: u8 = 3;
}

/// The named colors, in wire order for kind 3. [`Color::Reset`] has its
/// own kind (0), so the table holds the remaining sixteen.
const NAMED_COLORS: [Color; 16] = [
    Color::Black,
    Color::Red,
    Color::Green,
    Color::Yellow,
    Color::Blue,
    Color::Magenta,
    Color::Cyan,
    Color::Gray,
    Color::DarkGray,
    Color::LightRed,
    Color::LightGreen,
    Color::LightYellow,
    Color::LightBlue,
    Color::LightMagenta,
    Color::LightCyan,
    Color::White,
];

/// Why a packet was rejected. The paired byte offset in
/// [`PacketError`] points at the evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PacketErrorKind {
    /// The packet does not start with the `TVX` magic.
    BadMagic,
    /// The version byte is not one this parser understands.
    UnknownVersion {
        /// The version byte found on the wire.
        found: u8,
    },
    /// The packet ends mid-header or mid-op.
    UnexpectedEnd,
    /// The opcode is not part of the format.
    UnknownOp {
        /// The unknown opcode.
        op: u8,
    },
    /// A color byte is not part of the format: either the color kind byte
    /// itself or a named-color index beyond the table.
    UnknownColor {
        /// The offending color byte.
        kind: u8,
    },
    /// Text or URI bytes are not valid UTF-8.
    InvalidUtf8,
    /// Text, URI or fill symbol bytes contain a C0 control character or
    /// DEL — bytes that could break the escape sequences tuvix emits.
    ControlCharacter,
    /// Bytes follow the last declared op.
    TrailingBytes,
    /// Reserved bits (style flags 5-7) are set; they must be zero so the
    /// format can grow without ambiguity.
    ReservedBits,
    /// A snapshot cell column lies outside the declared row width.
    CellOutOfRange,
}

/// A rejected packet: what was wrong and where.
///
/// The offset is the byte position of the evidence — the offending byte
/// itself, or the position where the packet ended prematurely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PacketError {
    /// What was wrong.
    pub kind: PacketErrorKind,
    /// Byte offset of the evidence.
    pub offset: usize,
}

impl core::fmt::Display for PacketError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "invalid packet at byte {}: ", self.offset)?;
        match self.kind {
            PacketErrorKind::BadMagic => f.write_str("bad magic (expected \"TVX\")"),
            PacketErrorKind::UnknownVersion { found } => {
                write!(f, "unknown version {found:#04x}")
            }
            PacketErrorKind::UnexpectedEnd => f.write_str("unexpected end of packet"),
            PacketErrorKind::UnknownOp { op } => write!(f, "unknown op {op:#04x}"),
            PacketErrorKind::UnknownColor { kind } => write!(f, "unknown color byte {kind:#04x}"),
            PacketErrorKind::InvalidUtf8 => f.write_str("text is not valid UTF-8"),
            PacketErrorKind::ControlCharacter => {
                f.write_str("control character in text, URI or symbol")
            }
            PacketErrorKind::TrailingBytes => f.write_str("trailing bytes after the last op"),
            PacketErrorKind::ReservedBits => f.write_str("reserved style bits are set"),
            PacketErrorKind::CellOutOfRange => f.write_str("cell column outside the row"),
        }
    }
}

impl std::error::Error for PacketError {}

/// One batched buffer mutation, decoded or waiting to be encoded.
///
/// The vocabulary is the `Buffer`'s own write surface: text with a style,
/// a style layer over a region, a single-cell fill, a region clear, a
/// region hyperlink and a resize. Operations clip exactly like their
/// `Buffer` counterparts — an op that lands partly outside the buffer
/// paints the part that lands inside. A hyperlink op with an empty URI
/// clears the links in its region (the M10 clipboard's "empty means
/// clear" convention).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Mutation {
    /// Writes `text` at `(x, y)` with `style` (see [`Buffer::set_string`]).
    SetText {
        /// Column of the first written cell.
        x: u16,
        /// Row of the write.
        y: u16,
        /// The text to write.
        text: String,
        /// Style for every written cell.
        style: Style,
    },
    /// Layers `style` over every cell in `area` (see [`Buffer::set_style`]).
    SetStyle {
        /// The region to restyle.
        area: Rect,
        /// The style to layer on.
        style: Style,
    },
    /// Fills `area` with one printable ASCII symbol and `style`.
    Fill {
        /// The region to fill.
        area: Rect,
        /// The symbol: one printable ASCII byte, one display column.
        symbol: char,
        /// Style for every filled cell.
        style: Style,
    },
    /// Clears `area` back to blank (see [`Buffer::clear`]).
    ClearRegion {
        /// The region to clear.
        area: Rect,
    },
    /// Attaches an OSC 8 hyperlink to every cell in `area`, or clears the
    /// links when `uri` is empty.
    SetHyperlink {
        /// The region to link.
        area: Rect,
        /// The URI; C0 and DEL bytes are rejected at the boundary.
        uri: String,
    },
    /// Resizes the buffer (see [`Buffer::resize`]).
    Resize {
        /// The new width in columns.
        width: u16,
        /// The new height in rows.
        height: u16,
    },
}

impl Mutation {
    /// Encodes the mutation onto the end of `out` (without any packet
    /// header — use [`PacketWriter`] to build whole packets).
    pub fn write(&self, out: &mut Vec<u8>) {
        match self {
            Self::SetText { x, y, text, style } => {
                out.push(op::SET_TEXT);
                out.extend_from_slice(&x.to_le_bytes());
                out.extend_from_slice(&y.to_le_bytes());
                out.extend_from_slice(&wire_len(text).to_le_bytes());
                out.extend_from_slice(text.as_bytes());
                write_style(*style, out);
            }
            Self::SetStyle { area, style } => {
                out.push(op::SET_STYLE);
                write_rect(*area, out);
                write_style(*style, out);
            }
            Self::Fill {
                area,
                symbol,
                style,
            } => {
                out.push(op::FILL);
                write_rect(*area, out);
                out.push(*symbol as u8);
                write_style(*style, out);
            }
            Self::ClearRegion { area } => {
                out.push(op::CLEAR);
                write_rect(*area, out);
            }
            Self::SetHyperlink { area, uri } => {
                out.push(op::HYPERLINK);
                write_rect(*area, out);
                out.extend_from_slice(&wire_len(uri).to_le_bytes());
                out.extend_from_slice(uri.as_bytes());
            }
            Self::Resize { width, height } => {
                out.push(op::RESIZE);
                out.extend_from_slice(&width.to_le_bytes());
                out.extend_from_slice(&height.to_le_bytes());
            }
        }
    }

    /// Applies the mutation to `buffer`, clipping exactly like the
    /// `Buffer` method the variant maps onto.
    pub fn apply(&self, buffer: &mut Buffer) {
        match self {
            Self::SetText { x, y, text, style } => {
                buffer.set_string(*x, *y, text, *style);
            }
            Self::SetStyle { area, style } => {
                buffer.set_style(*area, *style);
            }
            Self::Fill {
                area,
                symbol,
                style,
            } => {
                let Some(area) = buffer.area.intersection(*area) else {
                    return;
                };
                let row = symbol.to_string();
                for y in area.y..area.bottom() {
                    buffer.set_stringn(
                        area.x,
                        y,
                        row.repeat(area.width as usize),
                        area.width,
                        *style,
                    );
                }
            }
            Self::ClearRegion { area } => {
                buffer.clear(*area);
            }
            Self::SetHyperlink { area, uri } => {
                let Some(area) = buffer.area.intersection(*area) else {
                    return;
                };
                let link = if uri.is_empty() {
                    None
                } else {
                    Some(Hyperlink::new(uri))
                };
                for y in area.y..area.bottom() {
                    for x in area.x..area.right() {
                        buffer.set_hyperlink(x, y, link.clone());
                    }
                }
            }
            Self::Resize { width, height } => {
                buffer.resize(Rect::new(0, 0, *width, *height));
            }
        }
    }
}

/// The u16 wire length of a text or URI field: content above 64 KiB must
/// be split by the host, so the writer saturates rather than truncating
/// silently mid-packet (see the module docs).
fn wire_len(text: &str) -> u16 {
    u16::try_from(text.len()).unwrap_or(u16::MAX)
}

fn write_rect(area: Rect, out: &mut Vec<u8>) {
    out.extend_from_slice(&area.x.to_le_bytes());
    out.extend_from_slice(&area.y.to_le_bytes());
    out.extend_from_slice(&area.width.to_le_bytes());
    out.extend_from_slice(&area.height.to_le_bytes());
}

fn write_color(color: Color, out: &mut Vec<u8>) {
    match color {
        Color::Reset => out.push(color_kind::RESET),
        Color::Indexed(index) => {
            out.push(color_kind::INDEXED);
            out.push(index);
        }
        Color::Rgb { r, g, b } => {
            out.push(color_kind::RGB);
            out.extend_from_slice(&[r, g, b]);
        }
        named => {
            out.push(color_kind::NAMED);
            let index = NAMED_COLORS.iter().position(|c| *c == named).unwrap_or(0);
            out.push(index as u8);
        }
    }
}

fn write_style(style: Style, out: &mut Vec<u8>) {
    let mut flags = 0u8;
    if style.fg.is_some() {
        flags |= style_flags::FG;
    }
    if style.bg.is_some() {
        flags |= style_flags::BG;
    }
    if style.underline_color.is_some() {
        flags |= style_flags::UNDERLINE;
    }
    if !style.add_modifier.is_empty() {
        flags |= style_flags::ADD_MODIFIER;
    }
    if !style.remove_modifier.is_empty() {
        flags |= style_flags::REMOVE_MODIFIER;
    }
    out.push(flags);
    if let Some(color) = style.fg {
        write_color(color, out);
    }
    if let Some(color) = style.bg {
        write_color(color, out);
    }
    if let Some(color) = style.underline_color {
        write_color(color, out);
    }
    if !style.add_modifier.is_empty() {
        out.extend_from_slice(&style.add_modifier.bits().to_le_bytes());
    }
    if !style.remove_modifier.is_empty() {
        out.extend_from_slice(&style.remove_modifier.bits().to_le_bytes());
    }
}

/// Builds a packet: a `TVX` header with a patched op count around the
/// encoded mutations.
///
/// # Examples
///
/// ```
/// use tuvix_core::embed::{Mutation, PacketWriter};
///
/// let packet = PacketWriter::new().push(&Mutation::Resize { width: 8, height: 2 }).finish();
/// assert_eq!(&packet[0..3], b"TVX");
/// assert_eq!(packet[3], 1); // version
/// assert_eq!(u32::from_le_bytes(packet[4..8].try_into().unwrap()), 1); // op count
/// ```
#[derive(Debug, Default)]
pub struct PacketWriter {
    bytes: Vec<u8>,
    count: u32,
}

impl PacketWriter {
    /// Starts a packet with the header written and the op count at zero.
    pub fn new() -> Self {
        Self {
            bytes: Self::header(),
            count: 0,
        }
    }

    fn header() -> Vec<u8> {
        let mut bytes = Vec::with_capacity(64);
        bytes.extend_from_slice(&PACKET_MAGIC);
        bytes.push(PACKET_VERSION);
        bytes.extend_from_slice(&[0, 0, 0, 0]); // op count, patched by finish
        bytes
    }

    /// Encodes one more mutation into the packet.
    pub fn push(mut self, mutation: &Mutation) -> Self {
        mutation.write(&mut self.bytes);
        self.count = self.count.saturating_add(1);
        self
    }

    /// Patches the op count and returns the finished packet.
    pub fn finish(self) -> Vec<u8> {
        let mut bytes = self.bytes;
        bytes[4..8].copy_from_slice(&self.count.to_le_bytes());
        bytes
    }
}

/// Reads a packet stream one mutation at a time.
///
/// The reader is the streaming half of the parser: `next_mutation` decodes
/// one mutation per call, `consumed` reports how many bytes were read so far,
/// and `finish` enforces that nothing trails the last declared op.
/// [`parse_packets`] is the collect-everything convenience.
#[derive(Debug)]
pub struct MutationReader<'a> {
    data: &'a [u8],
    position: usize,
    remaining: u32,
}

impl<'a> MutationReader<'a> {
    /// Validates the header and prepares to read the declared mutations.
    pub fn new(data: &'a [u8]) -> Result<Self, PacketError> {
        let mut reader = Self {
            data,
            position: 0,
            remaining: 0,
        };
        reader.read_envelope(PACKET_VERSION)?;
        reader.remaining = reader.read_u32()?;
        Ok(reader)
    }

    /// Validates the `TVX` envelope (magic + version byte) shared by
    /// packets and snapshots.
    fn read_envelope(&mut self, expected_version: u8) -> Result<(), PacketError> {
        if self.data.len() < 4 {
            return Err(self.truncated());
        }
        if self.data[0..3] != PACKET_MAGIC {
            return Err(PacketError {
                kind: PacketErrorKind::BadMagic,
                offset: 0,
            });
        }
        let version = self.data[3];
        if version != expected_version {
            return Err(PacketError {
                kind: PacketErrorKind::UnknownVersion { found: version },
                offset: 3,
            });
        }
        self.position = 4;
        Ok(())
    }

    fn truncated(&self) -> PacketError {
        PacketError {
            kind: PacketErrorKind::UnexpectedEnd,
            offset: self.data.len(),
        }
    }

    fn read_array<const N: usize>(&mut self) -> Result<[u8; N], PacketError> {
        let end = self.position + N;
        if end > self.data.len() {
            return Err(self.truncated());
        }
        let mut array = [0; N];
        array.copy_from_slice(&self.data[self.position..end]);
        self.position = end;
        Ok(array)
    }

    fn read_u32(&mut self) -> Result<u32, PacketError> {
        Ok(u32::from_le_bytes(self.read_array()?))
    }

    fn read_u16(&mut self) -> Result<u16, PacketError> {
        Ok(u16::from_le_bytes(self.read_array()?))
    }

    fn read_u8(&mut self) -> Result<u8, PacketError> {
        let byte = *self
            .data
            .get(self.position)
            .ok_or_else(|| self.truncated())?;
        self.position += 1;
        Ok(byte)
    }

    fn read_bytes(&mut self, len: usize) -> Result<&[u8], PacketError> {
        let end = self.position + len;
        if end > self.data.len() {
            return Err(self.truncated());
        }
        let bytes = &self.data[self.position..end];
        self.position = end;
        Ok(bytes)
    }

    fn read_rect(&mut self) -> Result<Rect, PacketError> {
        let x = self.read_u16()?;
        let y = self.read_u16()?;
        let width = self.read_u16()?;
        let height = self.read_u16()?;
        Ok(Rect::new(x, y, width, height))
    }

    /// Reads a length-prefixed UTF-8 field (text or URI), rejecting
    /// invalid UTF-8 and control characters with their byte offsets.
    fn read_text(&mut self) -> Result<String, PacketError> {
        let len = usize::from(self.read_u16()?);
        self.read_text_of_len(len)
    }

    /// Reads a UTF-8 field of a known length with the same strictness.
    fn read_text_of_len(&mut self, len: usize) -> Result<String, PacketError> {
        let start = self.position;
        let bytes = self.read_bytes(len)?;
        let text = match str::from_utf8(bytes) {
            Ok(text) => text.to_owned(),
            Err(error) => {
                return Err(PacketError {
                    kind: PacketErrorKind::InvalidUtf8,
                    offset: start + error.valid_up_to(),
                });
            }
        };
        check_no_control_characters(&text, start)?;
        Ok(text)
    }

    /// Reads a snapshot cell symbol: one u8 length prefix plus UTF-8 bytes.
    fn read_short_text(&mut self) -> Result<String, PacketError> {
        let len = usize::from(self.read_u8()?);
        self.read_text_of_len(len)
    }

    fn read_color(&mut self) -> Result<Color, PacketError> {
        let kind_offset = self.position;
        let kind = self.read_u8()?;
        match kind {
            color_kind::RESET => Ok(Color::Reset),
            color_kind::INDEXED => Ok(Color::Indexed(self.read_u8()?)),
            color_kind::RGB => {
                let bytes = self.read_array::<3>()?;
                Ok(Color::Rgb {
                    r: bytes[0],
                    g: bytes[1],
                    b: bytes[2],
                })
            }
            color_kind::NAMED => {
                let index_offset = self.position;
                let index = self.read_u8()?;
                NAMED_COLORS
                    .get(usize::from(index))
                    .copied()
                    .ok_or(PacketError {
                        kind: PacketErrorKind::UnknownColor { kind: index },
                        offset: index_offset,
                    })
            }
            unknown => Err(PacketError {
                kind: PacketErrorKind::UnknownColor { kind: unknown },
                offset: kind_offset,
            }),
        }
    }

    fn read_style(&mut self) -> Result<Style, PacketError> {
        let flags = self.read_u8()?;
        if flags & style_flags::RESERVED != 0 {
            return Err(PacketError {
                kind: PacketErrorKind::ReservedBits,
                offset: self.position - 1,
            });
        }
        let mut style = Style::new();
        if flags & style_flags::FG != 0 {
            style.fg = Some(self.read_color()?);
        }
        if flags & style_flags::BG != 0 {
            style.bg = Some(self.read_color()?);
        }
        if flags & style_flags::UNDERLINE != 0 {
            style.underline_color = Some(self.read_color()?);
        }
        if flags & style_flags::ADD_MODIFIER != 0 {
            style.add_modifier = Modifier::from_bits_truncate(self.read_u16()?);
        }
        if flags & style_flags::REMOVE_MODIFIER != 0 {
            style.remove_modifier = Modifier::from_bits_truncate(self.read_u16()?);
        }
        Ok(style)
    }

    fn read_fill_symbol(&mut self) -> Result<char, PacketError> {
        let offset = self.position;
        let byte = self.read_u8()?;
        if !(0x20..0x7f).contains(&byte) {
            return Err(PacketError {
                kind: PacketErrorKind::ControlCharacter,
                offset,
            });
        }
        Ok(char::from(byte))
    }

    /// Decodes the next mutation, or `None` once the declared op count has
    /// been consumed.
    ///
    /// (Named `next_mutation`, not `next`: the result carries parse errors,
    /// so this is not an [`Iterator::next`].)
    pub fn next_mutation(&mut self) -> Result<Option<Mutation>, PacketError> {
        if self.remaining == 0 {
            return Ok(None);
        }
        self.remaining -= 1;
        let opcode = self.read_u8()?;
        let mutation = match opcode {
            op::SET_TEXT => {
                let x = self.read_u16()?;
                let y = self.read_u16()?;
                let text = self.read_text()?;
                let style = self.read_style()?;
                Mutation::SetText { x, y, text, style }
            }
            op::SET_STYLE => {
                let area = self.read_rect()?;
                let style = self.read_style()?;
                Mutation::SetStyle { area, style }
            }
            op::FILL => {
                let area = self.read_rect()?;
                let symbol = self.read_fill_symbol()?;
                let style = self.read_style()?;
                Mutation::Fill {
                    area,
                    symbol,
                    style,
                }
            }
            op::CLEAR => {
                let area = self.read_rect()?;
                Mutation::ClearRegion { area }
            }
            op::HYPERLINK => {
                let area = self.read_rect()?;
                let uri = self.read_text()?;
                Mutation::SetHyperlink { area, uri }
            }
            op::RESIZE => {
                let width = self.read_u16()?;
                let height = self.read_u16()?;
                Mutation::Resize { width, height }
            }
            unknown => {
                return Err(PacketError {
                    kind: PacketErrorKind::UnknownOp { op: unknown },
                    offset: self.position - 1,
                });
            }
        };
        Ok(Some(mutation))
    }

    /// How many bytes of the stream have been consumed so far.
    pub fn consumed(&self) -> usize {
        self.position
    }

    /// How many declared ops have not been read yet.
    pub fn remaining(&self) -> u32 {
        self.remaining
    }

    /// Enforces that the packet ends exactly at the last declared op.
    pub fn finish(self) -> Result<(), PacketError> {
        if self.position < self.data.len() {
            return Err(PacketError {
                kind: PacketErrorKind::TrailingBytes,
                offset: self.position,
            });
        }
        Ok(())
    }
}

/// Encodes a buffer's visible content as a frame snapshot packet.
///
/// The outbound half of the embedding boundary: the same `TVX` envelope at
/// version 2, then the grid row by row. A row whose every cell equals
/// [`Cell::empty`] costs exactly one byte, so sparse screens stay small;
/// content cells are listed explicitly with symbol, style, hyperlink and
/// the covered-half flag, and decode into a fresh buffer cell-for-cell —
/// the differential gate proves `decode(encode(buffer)) == buffer`.
///
/// The snapshot is content, not bookkeeping: a buffer with a journal
/// decodes into one without, and the two compare equal (journal state is
/// never part of [`Buffer`] equality).
///
/// # Examples
///
/// ```
/// use tuvix_core::buffer::Buffer;
/// use tuvix_core::embed::{decode_snapshot, encode_snapshot};
/// use tuvix_core::geometry::Rect;
///
/// let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 2));
/// buffer.set_string(0, 0, "hello", tuvix_core::style::Style::new());
/// let decoded = decode_snapshot(&encode_snapshot(&buffer)).unwrap();
/// assert_eq!(decoded, buffer);
/// ```
pub fn encode_snapshot(buffer: &Buffer) -> Vec<u8> {
    let mut out = Vec::with_capacity(64);
    out.extend_from_slice(&PACKET_MAGIC);
    out.push(SNAPSHOT_VERSION);
    out.extend_from_slice(&buffer.area.width.to_le_bytes());
    out.extend_from_slice(&buffer.area.height.to_le_bytes());
    for y in 0..buffer.area.height {
        let row_start = y as usize * buffer.area.width as usize;
        let row_end = row_start + buffer.area.width as usize;
        let row = &buffer.content[row_start..row_end];
        if row.iter().all(|cell| *cell == Cell::empty()) {
            out.push(0); // empty row: one byte
            continue;
        }
        out.push(1);
        let listed = row.iter().filter(|cell| **cell != Cell::empty()).count();
        out.extend_from_slice(&(listed as u16).to_le_bytes());
        for (x, cell) in row.iter().enumerate() {
            if *cell == Cell::empty() {
                continue;
            }
            out.extend_from_slice(&(x as u16).to_le_bytes());
            let symbol = cell.symbol.as_str();
            out.push(symbol.len() as u8);
            out.extend_from_slice(symbol.as_bytes());
            let mut flags = 0u8;
            if cell.skip {
                flags |= cell_flags::SKIP;
            }
            if cell.hyperlink.is_some() {
                flags |= cell_flags::HYPERLINK;
            }
            out.push(flags);
            write_style(cell.style, &mut out);
            if let Some(hyperlink) = &cell.hyperlink {
                out.extend_from_slice(&wire_len(hyperlink.as_str()).to_le_bytes());
                out.extend_from_slice(hyperlink.as_str().as_bytes());
            }
        }
    }
    out
}

/// Decodes a frame snapshot packet back into a [`Buffer`].
///
/// Strict, like every parser at a tuvix trust boundary: bad magic,
/// unknown version, truncated streams, unknown cell flags (reserved bits
/// are rejected) and out-of-range columns all fail with a byte offset.
///
/// # Examples
///
/// ```
/// use tuvix_core::embed::{PacketErrorKind, decode_snapshot};
///
/// let error = decode_snapshot(&[]).unwrap_err();
/// assert_eq!(error.kind, PacketErrorKind::UnexpectedEnd);
/// ```
pub fn decode_snapshot(data: &[u8]) -> Result<Buffer, PacketError> {
    let mut reader = MutationReader {
        data,
        position: 0,
        remaining: 0,
    };
    reader.read_envelope(SNAPSHOT_VERSION)?;
    let width = reader.read_u16()?;
    let height = reader.read_u16()?;
    let mut buffer = Buffer::empty(Rect::new(0, 0, width, height));
    for y in 0..u32::from(height) {
        let kind = reader.read_u8()?;
        if kind == 0 {
            continue; // empty row
        }
        if kind != 1 {
            return Err(PacketError {
                kind: PacketErrorKind::UnknownOp { op: kind },
                offset: reader.consumed() - 1,
            });
        }
        let count = usize::from(reader.read_u16()?);
        for _ in 0..count {
            let x_offset = reader.read_u16()?;
            let symbol = reader.read_short_text()?;
            let flags = reader.read_u8()?;
            if flags & !cell_flags::SKIP & !cell_flags::HYPERLINK != 0 {
                return Err(PacketError {
                    kind: PacketErrorKind::ReservedBits,
                    offset: reader.consumed() - 1,
                });
            }
            let style = reader.read_style()?;
            let hyperlink = if flags & cell_flags::HYPERLINK != 0 {
                Some(Hyperlink::new(reader.read_text()?))
            } else {
                None
            };
            let index = y as usize * width as usize + x_offset as usize;
            let Some(slot) = buffer.content.get_mut(index) else {
                return Err(PacketError {
                    kind: PacketErrorKind::CellOutOfRange,
                    offset: reader.position,
                });
            };
            *slot = Cell {
                symbol: Symbol::new(&symbol),
                style,
                hyperlink,
                skip: flags & cell_flags::SKIP != 0,
            };
        }
    }
    reader.finish()?;
    Ok(buffer)
}

fn check_no_control_characters(text: &str, offset: usize) -> Result<(), PacketError> {
    let bad = text
        .char_indices()
        .find(|&(_, c)| c.is_ascii_control() || c == '\u{7f}');
    match bad {
        None => Ok(()),
        Some((byte_index, _)) => Err(PacketError {
            kind: PacketErrorKind::ControlCharacter,
            offset: offset + byte_index,
        }),
    }
}

/// Parses a whole packet into its mutations, enforcing that nothing trails
/// the last declared op.
///
/// # Examples
///
/// ```
/// use tuvix_core::embed::{PacketErrorKind, PacketWriter, parse_packets};
///
/// let packet = PacketWriter::new().finish();
/// assert!(parse_packets(&packet).unwrap().is_empty());
///
/// let mut corrupted = packet.clone();
/// corrupted.push(0x7f);
/// corrupted[4] = 1; // one declared op, a bogus one
/// let error = parse_packets(&corrupted).unwrap_err();
/// assert_eq!(error.kind, PacketErrorKind::UnknownOp { op: 0x7f });
/// assert_eq!(error.offset, 8);
/// ```
pub fn parse_packets(data: &[u8]) -> Result<Vec<Mutation>, PacketError> {
    let mut reader = MutationReader::new(data)?;
    let mut mutations = Vec::new();
    while let Some(mutation) = reader.next_mutation()? {
        mutations.push(mutation);
    }
    reader.finish()?;
    Ok(mutations)
}
