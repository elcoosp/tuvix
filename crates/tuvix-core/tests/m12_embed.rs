//! M12-A: the mutation packet — a flat, versioned binary protocol for
//! driving a `Buffer` from outside the process.
//!
//! The parser is strict (byte-offset errors, the M9 `Screen` philosophy):
//! a packet that cannot be trusted is rejected, never guessed at. The
//! wire format is little-endian, header `TVX` + version + u32 op count.

use pretty_assertions::assert_eq;

use tuvix_core::buffer::Buffer;
use tuvix_core::embed::{Mutation, PacketErrorKind, PacketWriter, parse_packets};
use tuvix_core::geometry::Rect;
use tuvix_core::style::{Color, Modifier, Style};

/// Every color variant, so the style encoding is proven exhaustive.
fn all_colors() -> Vec<Color> {
    vec![
        Color::Reset,
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
        Color::Rgb {
            r: 0x12,
            g: 0x34,
            b: 0x56,
        },
        Color::Indexed(200),
    ]
}

#[test]
fn header_magic_is_tvx_and_version_one() {
    let packet = PacketWriter::new()
        .push(&Mutation::Resize {
            width: 4,
            height: 2,
        })
        .finish();
    assert_eq!(&packet[0..3], b"TVX");
    assert_eq!(packet[3], 1);
}

#[test]
fn empty_packet_parses_to_no_mutations() {
    let packet = PacketWriter::new().finish();
    let mutations = parse_packets(&packet).unwrap();
    assert_eq!(mutations, []);
}

#[test]
fn all_ops_round_trip_through_writer_and_parser() {
    let mutations = vec![
        Mutation::SetText {
            x: 3,
            y: 7,
            text: "汉héllo — 汉字".to_owned(),
            style: Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
        },
        Mutation::SetStyle {
            area: Rect::new(0, 0, 10, 2),
            style: Style::new().bg(Color::Rgb { r: 1, g: 2, b: 3 }),
        },
        Mutation::Fill {
            area: Rect::new(1, 1, 4, 3),
            symbol: '*',
            style: Style::new().fg(Color::Indexed(7)),
        },
        Mutation::ClearRegion {
            area: Rect::new(2, 2, 8, 4),
        },
        Mutation::SetHyperlink {
            area: Rect::new(0, 5, 6, 1),
            uri: "https://tuvix.dev".to_owned(),
        },
        Mutation::Resize {
            width: 120,
            height: 40,
        },
        Mutation::SetText {
            x: 0,
            y: 0,
            text: String::new(),
            style: Style::new(),
        },
    ];
    let packet = mutations
        .iter()
        .fold(PacketWriter::new(), PacketWriter::push)
        .finish();
    assert_eq!(parse_packets(&packet).unwrap(), mutations);
}

#[test]
fn style_round_trips_every_color_and_modifier() {
    for color in all_colors() {
        for role in 0..3 {
            let style = match role {
                0 => Style::new().fg(color),
                1 => Style::new().bg(color),
                _ => Style::new().underline_color(color),
            };
            let mutation = Mutation::SetStyle {
                area: Rect::new(0, 0, 1, 1),
                style,
            };
            let packet = PacketWriter::new().push(&mutation).finish();
            assert_eq!(
                parse_packets(&packet).unwrap(),
                vec![mutation],
                "role {role}"
            );
        }
    }
    let heavy = Style::new()
        .add_modifier(
            Modifier::BOLD | Modifier::ITALIC | Modifier::RAPID_BLINK | Modifier::CROSSED_OUT,
        )
        .remove_modifier(Modifier::DIM | Modifier::REVERSED);
    let mutation = Mutation::SetStyle {
        area: Rect::new(0, 0, 1, 1),
        style: heavy,
    };
    let packet = PacketWriter::new().push(&mutation).finish();
    assert_eq!(parse_packets(&packet).unwrap(), vec![mutation]);
}

#[test]
fn bad_magic_is_rejected_at_byte_zero() {
    let mut packet = PacketWriter::new().finish();
    packet[0] = b'T';
    packet[1] = b'X';
    let error = parse_packets(&packet).unwrap_err();
    assert_eq!(error.kind, PacketErrorKind::BadMagic);
    assert_eq!(error.offset, 0);
}

#[test]
fn unknown_version_is_rejected_with_the_found_byte() {
    let mut packet = PacketWriter::new().finish();
    packet[3] = 9;
    let error = parse_packets(&packet).unwrap_err();
    assert_eq!(error.kind, PacketErrorKind::UnknownVersion { found: 9 });
    assert_eq!(error.offset, 3);
}

#[test]
fn unknown_op_is_rejected_with_its_byte_offset() {
    // header (8 bytes) + one bogus opcode.
    let mut packet = PacketWriter::new().finish();
    packet[4] = 1; // op count = 1
    packet.extend_from_slice(&[0x7f]);
    let error = parse_packets(&packet).unwrap_err();
    assert_eq!(error.kind, PacketErrorKind::UnknownOp { op: 0x7f });
    assert_eq!(error.offset, 8);
}

#[test]
fn truncated_packet_reports_unexpected_end() {
    let full = PacketWriter::new()
        .push(&Mutation::SetText {
            x: 1,
            y: 2,
            text: "hello".to_owned(),
            style: Style::new(),
        })
        .finish();
    for cut in [8, 12, full.len() - 1] {
        let error = parse_packets(&full[..cut]).unwrap_err();
        assert_eq!(error.kind, PacketErrorKind::UnexpectedEnd, "cut at {cut}");
    }
}

#[test]
fn trailing_bytes_after_the_last_op_are_rejected() {
    let mut packet = PacketWriter::new()
        .push(&Mutation::Resize {
            width: 4,
            height: 2,
        })
        .finish();
    packet.extend_from_slice(&[0x00]);
    let error = parse_packets(&packet).unwrap_err();
    assert_eq!(error.kind, PacketErrorKind::TrailingBytes);
}

#[test]
fn control_characters_in_text_are_rejected_with_the_byte_offset() {
    // (text, expected offset of the offending byte within the text field)
    for (index, (text, bad_at)) in [("a\nb", 1usize), ("a\u{1b}[31mb", 1), ("ab\u{7f}", 2)]
        .into_iter()
        .enumerate()
    {
        let mut packet = PacketWriter::new().finish();
        packet[4] = 1;
        // SET_TEXT: op(1) x(2) y(2) len(2) then bytes.
        packet.push(0x01);
        packet.extend_from_slice(&1u16.to_le_bytes());
        packet.extend_from_slice(&1u16.to_le_bytes());
        packet.extend_from_slice(&(text.len() as u16).to_le_bytes());
        let text_offset = packet.len();
        packet.extend_from_slice(text.as_bytes());
        let error = parse_packets(&packet).unwrap_err();
        assert_eq!(
            error.kind,
            PacketErrorKind::ControlCharacter,
            "case {index}"
        );
        assert_eq!(error.offset, text_offset + bad_at, "case {index}");
    }
}

#[test]
fn invalid_utf8_in_text_is_rejected_with_the_offset() {
    let mut packet = PacketWriter::new().finish();
    packet[4] = 1;
    packet.push(0x01);
    packet.extend_from_slice(&0u16.to_le_bytes());
    packet.extend_from_slice(&0u16.to_le_bytes());
    packet.extend_from_slice(&3u16.to_le_bytes());
    packet.extend_from_slice(&[0x41, 0xff, 0x42]);
    let error = parse_packets(&packet).unwrap_err();
    assert_eq!(error.kind, PacketErrorKind::InvalidUtf8);
    assert_eq!(error.offset, 8 + 7 + 1);
}

#[test]
fn control_characters_in_uris_are_rejected() {
    let mut packet = PacketWriter::new().finish();
    packet[4] = 1;
    // HYPERLINK: op(1) x y w h (8) len(2) then bytes.
    packet.push(0x05);
    packet.extend_from_slice(&0u16.to_le_bytes());
    packet.extend_from_slice(&0u16.to_le_bytes());
    packet.extend_from_slice(&4u16.to_le_bytes());
    packet.extend_from_slice(&1u16.to_le_bytes());
    packet.extend_from_slice(&2u16.to_le_bytes());
    packet.extend_from_slice(&[0x1b, b'x']);
    let error = parse_packets(&packet).unwrap_err();
    assert_eq!(error.kind, PacketErrorKind::ControlCharacter);
    assert_eq!(error.offset, packet.len() - 2);
}

#[test]
fn unknown_color_kind_is_rejected_with_its_offset() {
    let mut packet = PacketWriter::new().finish();
    packet[4] = 1;
    // SET_STYLE: op(1) + rect(8) + style flags(1) + color kind(1).
    packet.push(0x02);
    packet.extend_from_slice(&0u16.to_le_bytes());
    packet.extend_from_slice(&0u16.to_le_bytes());
    packet.extend_from_slice(&1u16.to_le_bytes());
    packet.extend_from_slice(&1u16.to_le_bytes());
    packet.push(0x01); // fg present
    packet.push(0x09); // color kind 9: unknown
    let error = parse_packets(&packet).unwrap_err();
    assert_eq!(error.kind, PacketErrorKind::UnknownColor { kind: 9 });
    assert_eq!(error.offset, 8 + 1 + 8 + 1);
}

#[test]
fn reserved_style_bits_are_rejected() {
    let mut packet = PacketWriter::new().finish();
    packet[4] = 1;
    packet.push(0x02); // SET_STYLE
    packet.extend_from_slice(&0u16.to_le_bytes());
    packet.extend_from_slice(&0u16.to_le_bytes());
    packet.extend_from_slice(&1u16.to_le_bytes());
    packet.extend_from_slice(&1u16.to_le_bytes());
    packet.push(0b1110_0001); // fg present + reserved high bits
    packet.push(0x00); // color Reset
    let error = parse_packets(&packet).unwrap_err();
    assert_eq!(error.kind, PacketErrorKind::ReservedBits);
}

#[test]
fn fill_symbol_must_be_one_printable_ascii_column() {
    for symbol_byte in [0x20u8, 0x7e] {
        let mut packet = PacketWriter::new().finish();
        packet[4] = 1;
        packet.push(0x03); // FILL
        packet.extend_from_slice(&0u16.to_le_bytes());
        packet.extend_from_slice(&0u16.to_le_bytes());
        packet.extend_from_slice(&2u16.to_le_bytes());
        packet.extend_from_slice(&1u16.to_le_bytes());
        packet.push(symbol_byte);
        packet.push(0x00); // style: nothing set
        let mutations = parse_packets(&packet).unwrap();
        assert!(
            matches!(&mutations[0], Mutation::Fill { symbol, .. } if *symbol == char::from(symbol_byte)),
            "symbol {symbol_byte:#x}"
        );
    }
    for symbol_byte in [0x1fu8, 0x7f, 0x80] {
        let mut packet = PacketWriter::new().finish();
        packet[4] = 1;
        packet.push(0x03);
        packet.extend_from_slice(&0u16.to_le_bytes());
        packet.extend_from_slice(&0u16.to_le_bytes());
        packet.extend_from_slice(&2u16.to_le_bytes());
        packet.extend_from_slice(&1u16.to_le_bytes());
        let symbol_offset = packet.len();
        packet.push(symbol_byte);
        packet.push(0x00);
        let error = parse_packets(&packet).unwrap_err();
        assert_eq!(
            error.kind,
            PacketErrorKind::ControlCharacter,
            "symbol {symbol_byte:#x}"
        );
        assert_eq!(error.offset, symbol_offset);
    }
}

#[test]
fn reader_streams_ops_and_reports_consumption() {
    let mutations = vec![
        Mutation::Resize {
            width: 10,
            height: 5,
        },
        Mutation::SetText {
            x: 0,
            y: 0,
            text: "hi".to_owned(),
            style: Style::new(),
        },
        Mutation::ClearRegion {
            area: Rect::new(0, 0, 1, 1),
        },
    ];
    let packet = mutations
        .iter()
        .fold(PacketWriter::new(), PacketWriter::push)
        .finish();
    let mut reader = tuvix_core::embed::MutationReader::new(&packet).unwrap();
    let mut seen = Vec::new();
    while let Some(mutation) = reader.next_mutation().unwrap() {
        seen.push(mutation);
    }
    assert_eq!(seen, mutations);
    assert_eq!(reader.consumed(), packet.len());
    reader.finish().unwrap();
}

#[test]
fn reader_finish_rejects_trailing_bytes() {
    let mut packet = PacketWriter::new()
        .push(&Mutation::Resize {
            width: 1,
            height: 1,
        })
        .finish();
    packet.push(0xff);
    let mut reader = tuvix_core::embed::MutationReader::new(&packet).unwrap();
    while reader.next_mutation().unwrap().is_some() {}
    let error = reader.finish().unwrap_err();
    assert_eq!(error.kind, PacketErrorKind::TrailingBytes);
}

// ---------------------------------------------------------------------------
// M12-B: the applier and the frame snapshot
// ---------------------------------------------------------------------------

use tuvix_core::embed::{decode_snapshot, encode_snapshot};
use tuvix_core::style::Hyperlink;
use tuvix_core::stylize::Stylize;

/// Applies `mutations` through the packet boundary and compares the result
/// against the same mutations applied through the direct Buffer API.
#[test]
fn applied_packet_matches_the_direct_buffer_api() {
    let mutations = vec![
        Mutation::Resize {
            width: 40,
            height: 8,
        },
        Mutation::SetText {
            x: 2,
            y: 1,
            text: "driven from outside — 汉字".to_owned(),
            style: Style::new().fg(Color::Green).add_modifier(Modifier::BOLD),
        },
        Mutation::SetStyle {
            area: Rect::new(0, 0, 40, 1),
            style: Style::new().bg(Color::Blue),
        },
        Mutation::Fill {
            area: Rect::new(0, 4, 10, 2),
            symbol: '*',
            style: Style::new(),
        },
        Mutation::SetHyperlink {
            area: Rect::new(2, 1, 8, 1),
            uri: "https://tuvix.dev".to_owned(),
        },
        Mutation::ClearRegion {
            area: Rect::new(0, 6, 40, 2),
        },
    ];

    let mut through_packet = Buffer::empty(Rect::new(0, 0, 40, 8));
    for mutation in &mutations {
        mutation.apply(&mut through_packet);
    }

    // The same result, painted through the Buffer API directly.
    let mut direct = Buffer::empty(Rect::new(0, 0, 40, 8));
    direct.set_string(
        2,
        1,
        "driven from outside — 汉字",
        Style::new().fg(Color::Green).add_modifier(Modifier::BOLD),
    );
    direct.set_style(Rect::new(0, 0, 40, 1), Style::new().bg(Color::Blue));
    for y in 4..6 {
        direct.set_stringn(0, y, "*".repeat(10), 10, Style::new());
    }
    for x in 2..10 {
        direct.set_hyperlink(x, 1, Some(Hyperlink::new("https://tuvix.dev")));
    }
    direct.clear(Rect::new(0, 6, 40, 2));

    assert_eq!(through_packet, direct);
}

#[test]
fn empty_hyperlink_uri_clears_the_region() {
    let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 1));
    buffer.set_string_with_hyperlink(0, 0, "linked", Style::new(), Hyperlink::new("https://x"));
    Mutation::SetHyperlink {
        area: Rect::new(0, 0, 6, 1),
        uri: String::new(),
    }
    .apply(&mut buffer);
    assert!(buffer.get(0, 0).unwrap().hyperlink.is_none());
}

#[test]
fn out_of_bounds_ops_clip_like_the_buffer_api() {
    let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 4));
    Mutation::SetText {
        x: 8,
        y: 0,
        text: "clipped 汉字".to_owned(),
        style: Style::new(),
    }
    .apply(&mut buffer);
    Mutation::Fill {
        area: Rect::new(8, 2, 20, 10),
        symbol: '#',
        style: Style::new(),
    }
    .apply(&mut buffer);
    // Nothing panicked; the text stopped at the edge like set_string does —
    // only 'c' and 'l' fit before the edge, and the wide glyph never starts.
    assert_eq!(buffer.get(9, 0).unwrap().symbol.as_str(), "l");
    assert_eq!(buffer.get(9, 2).unwrap().symbol.as_str(), "#");

    Mutation::SetHyperlink {
        area: Rect::new(0, 3, 40, 4),
        uri: "https://x".to_owned(),
    }
    .apply(&mut buffer);
    Mutation::SetStyle {
        area: Rect::new(0, 0, 100, 100),
        style: Style::new().fg(Color::Red),
    }
    .apply(&mut buffer);
    assert_eq!(buffer.get(5, 3).unwrap().style.fg, Some(Color::Red));

    Mutation::ClearRegion {
        area: Rect::new(0, 0, 100, 100),
    }
    .apply(&mut buffer);
    assert_eq!(buffer, Buffer::empty(Rect::new(0, 0, 10, 4)));
}

#[test]
fn snapshot_round_trips_content_style_wide_glyphs_and_links() {
    let mut buffer = Buffer::empty(Rect::new(0, 0, 40, 6));
    buffer.set_string(0, 0, "plain row with blanks        here", Style::new());
    buffer.set_string(0, 1, "styled".red().bold(), Style::new().bg(Color::Blue));
    buffer.set_string(0, 2, "汉字 wide pair and link", Style::new());
    for x in 0..8 {
        buffer.set_hyperlink(x, 3, Some(Hyperlink::new("https://tuvix.dev")));
    }
    buffer.set_string(20, 4, "trailing", Style::new().fg(Color::Indexed(213)));

    let decoded = decode_snapshot(&encode_snapshot(&buffer)).unwrap();
    assert_eq!(decoded, buffer);
    assert_eq!(decoded.diff_runs(&buffer).count(), 0);
}

#[test]
fn snapshot_of_an_empty_buffer_is_all_empty_rows() {
    let buffer = Buffer::empty(Rect::new(0, 0, 30, 5));
    let encoded = encode_snapshot(&buffer);
    // Header + 5 empty-row markers, nothing else.
    assert_eq!(encoded.len(), 8 + 5);
    let decoded = decode_snapshot(&encoded).unwrap();
    assert_eq!(decoded, buffer);
}

#[test]
fn snapshot_lists_only_content_cells() {
    let mut buffer = Buffer::empty(Rect::new(0, 0, 200, 60));
    buffer.set_string(0, 0, "one cell of content", Style::new());
    let encoded = encode_snapshot(&buffer);
    // Far below the 12,000-cell worst case: empty rows cost one byte and
    // the one content row lists only its 19 written cells.
    assert!(
        encoded.len() < 8 + 59 + 125,
        "encoded {} bytes",
        encoded.len()
    );
}

#[test]
fn snapshot_rejects_bad_headers_and_trailing_bytes() {
    let buffer = Buffer::empty(Rect::new(0, 0, 4, 2));
    let mut encoded = encode_snapshot(&buffer);
    let decoded = decode_snapshot(&encoded).unwrap();
    assert_eq!(decoded, buffer);

    encoded[0] = b'X';
    let error = decode_snapshot(&encoded).unwrap_err();
    assert_eq!(error.kind, PacketErrorKind::BadMagic);

    let mut encoded = encode_snapshot(&buffer);
    encoded[3] = 7;
    let error = decode_snapshot(&encoded).unwrap_err();
    assert_eq!(error.kind, PacketErrorKind::UnknownVersion { found: 7 });

    let mut encoded = encode_snapshot(&buffer);
    encoded.push(0);
    let error = decode_snapshot(&encoded).unwrap_err();
    assert_eq!(error.kind, PacketErrorKind::TrailingBytes);

    let error = decode_snapshot(&encoded[..6]).unwrap_err();
    assert_eq!(error.kind, PacketErrorKind::UnexpectedEnd);
}

#[test]
fn snapshot_honors_journal_independent_equality() {
    // A buffer whose journal has recorded paints decodes equal to a buffer
    // decoded twice: the snapshot is content, not bookkeeping.
    let mut painted = Buffer::empty(Rect::new(0, 0, 10, 2));
    painted.enable_journal();
    painted.set_string(0, 0, "journalled", Style::new());
    let decoded = decode_snapshot(&encode_snapshot(&painted)).unwrap();
    assert_eq!(decoded, painted);
    assert!(!decoded.has_journal());
}
