use super::*;
use std::io::Cursor;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}
fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

#[test]
fn unicode_editing_and_boundaries() {
    let mut b = LineBuffer::default();
    b.left();
    b.backspace();
    b.delete();
    b.right();
    b.insert("aé🦀z");
    assert_eq!(b.cursor(), 8);
    b.left();
    b.left();
    assert_eq!(b.cursor(), 3);
    b.insert("中");
    assert_eq!(b.text(), "aé中🦀z");
    b.backspace();
    b.delete();
    assert_eq!(b.text(), "aéz");
    b.home();
    b.backspace();
    b.delete();
    assert_eq!((b.text(), b.cursor()), ("éz", 0));
    b.right();
    assert_eq!(b.cursor(), 2);
    b.end();
    b.right();
    b.delete();
    assert_eq!(b.cursor(), 3);
    b.clear();
    assert_eq!(b.into_parts(), (String::new(), 0));
}

#[test]
fn insertion_at_beginning_middle_and_end() {
    let mut b = LineBuffer::from_parts("é🦀".into(), 0);
    b.insert("中");
    assert_eq!((b.text(), b.cursor()), ("中é🦀", 3));
    b.right();
    b.insert("λ");
    assert_eq!((b.text(), b.cursor()), ("中éλ🦀", 7));
    b.end();
    b.insert("終");
    assert_eq!((b.text(), b.cursor()), ("中éλ🦀終", 14));
}

#[test]
fn multiline_paste_preserves_layout_without_terminal_controls() {
    let input = "\tα\r\n\x1b[31mβ\x1b[0m\tγ\rδ\n\n\x1b]title\x07終\0\x7f\u{85}";
    assert_eq!(sanitize_paste(input), "\tα\nβ\tγ\nδ\n\n終");
    assert_eq!(sanitize(input), "α βγ δ  終");
    assert_eq!(sanitize_paste(""), "");
    assert_eq!(sanitize_paste("\t\n\r\n\r"), "\t\n\n\n");
}

#[test]
fn reader_reuse_after_cancellation_submission_and_eof() {
    for cancel in [ctrl('c'), key(KeyCode::Esc)] {
        let mut history = History::new();
        history.push("previous");
        let mut reader = Reader::new();
        reader.buffer.insert("discarded draft");
        reader.handle_key(key(KeyCode::Up), &mut history);
        assert_eq!(reader.handle_key(cancel, &mut history), Outcome::Cancelled);
        assert_eq!((reader.buffer.text(), reader.buffer.cursor()), ("", 0));
        assert_eq!(history.entries(), &["previous"]);
        // Cancellation must discard both the recalled entry and saved draft.
        reader.handle_key(key(KeyCode::Down), &mut history);
        assert_eq!(reader.buffer.text(), "");
        reader.buffer.insert("fresh");
        reader.handle_key(key(KeyCode::Up), &mut history);
        reader.handle_key(key(KeyCode::Down), &mut history);
        assert_eq!(reader.buffer.text(), "fresh");
        assert_eq!(
            reader.handle_key(key(KeyCode::Enter), &mut history),
            Outcome::Submitted("fresh".into())
        );
        assert_eq!((reader.buffer.text(), reader.buffer.cursor()), ("", 0));
        assert_eq!(reader.handle_key(ctrl('d'), &mut history), Outcome::Eof);
        assert_eq!((reader.buffer.text(), reader.buffer.cursor()), ("", 0));
        reader.buffer.insert("again");
        assert_eq!(
            reader.handle_key(key(KeyCode::Enter), &mut history),
            Outcome::Submitted("again".into())
        );
        assert_eq!(history.entries(), &["previous", "fresh", "again"]);
    }
}

#[test]
fn parts_preserve_exact_text() {
    let text = "é\n\x1b".to_owned();
    assert_eq!(
        LineBuffer::from_parts(text.clone(), 2).into_parts(),
        (text, 2)
    );
}
#[test]
#[should_panic]
fn parts_reject_inside_scalar() {
    LineBuffer::from_parts("é".into(), 1);
}
#[test]
#[should_panic]
fn parts_reject_out_of_range() {
    LineBuffer::from_parts("".into(), 1);
}

#[test]
fn sanitize_terminal_sequences_and_controls() {
    for sequence in [
        "\x1b[31m",
        "\x1b]0;title\x07",
        "\x1b]title\x1b\\",
        "\x1bPpayload\x1b\\",
        "\x1bXpayload\x1b\\",
        "\x1b^payload\x1b\\",
        "\x1b_payload\x1b\\",
        "\x1b(B",
        "\x1b7",
        "\u{9b}31m",
        "\u{9d}title\u{9c}",
        "\u{90}payload\u{9c}",
        "\u{98}payload\u{9c}",
        "\u{9e}payload\u{9c}",
        "\u{9f}payload\u{9c}",
    ] {
        let mut b = LineBuffer::default();
        b.insert(&format!("a{sequence}é"));
        assert_eq!(b.text(), "aé", "{sequence:?}");
        assert_eq!(sanitize_paste(&format!("a{sequence}é")), "aé");
    }
    assert_eq!(sanitize("a\r\nb\rc\nd\t\0\x7f\u{85}é"), "a b c dé");
    for sequence in ["\x1b", "\x1b[31", "\x1b]title", "\x1bPdata", "\x1b("] {
        assert_eq!(sanitize(&format!("a{sequence}")), "a");
        assert_eq!(sanitize_paste(&format!("a{sequence}")), "a");
    }
}

#[test]
fn keys_and_outcomes() {
    let mut b = LineBuffer::default();
    assert_eq!(b.handle_key(ctrl('d')), Outcome::Eof);
    b.handle_key(key(KeyCode::Char('é')));
    b.handle_key(KeyEvent::new(KeyCode::Char('Z'), KeyModifiers::SHIFT));
    b.handle_key(ctrl('a'));
    assert_eq!(b.cursor(), 0);
    b.handle_key(ctrl('d'));
    assert_eq!(b.text(), "Z");
    b.handle_key(ctrl('e'));
    assert_eq!(b.cursor(), 1);
    assert_eq!(b.handle_key(ctrl('d')), Outcome::Continue);
    assert_eq!(b.text(), "Z");
    assert_eq!(
        b.handle_key(key(KeyCode::Enter)),
        Outcome::Submitted("Z".into())
    );
    assert_eq!(b.handle_key(ctrl('c')), Outcome::Cancelled);
    assert_eq!((b.text(), b.cursor()), ("", 0));
    b.insert("é🦀");
    b.home();
    b.right();
    assert_eq!(b.handle_key(key(KeyCode::Esc)), Outcome::Cancelled);
    assert_eq!((b.text(), b.cursor()), ("", 0));
    b.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT));
    b.handle_key(KeyEvent::new(
        KeyCode::Char('x'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    ));
    let mut release = key(KeyCode::Enter);
    release.kind = KeyEventKind::Release;
    assert_eq!(b.handle_key(release), Outcome::Continue);
    assert_eq!(b.text(), "");
    let mut repeat = key(KeyCode::Char('!'));
    repeat.kind = KeyEventKind::Repeat;
    b.handle_key(repeat);
    assert_eq!(b.text(), "!");
    b.handle_key(key(KeyCode::Home));
    b.handle_key(key(KeyCode::Delete));
    b.handle_key(key(KeyCode::End));
    b.handle_key(key(KeyCode::Backspace));
    assert_eq!(b.text(), "");
}

#[test]
fn history_navigation_restores_draft_and_cursor() {
    let mut h = History::new();
    h.push("");
    h.push("one");
    h.push("one");
    h.push("二");
    assert_eq!(h.entries(), &["one", "二"]);
    let mut r = Reader::new();
    r.buffer.insert("draft");
    r.buffer.left();
    r.handle_key(key(KeyCode::Down), &mut h);
    r.handle_key(key(KeyCode::Up), &mut h);
    assert_eq!(r.buffer.text(), "二");
    r.handle_key(key(KeyCode::Char('!')), &mut h);
    r.handle_key(key(KeyCode::Up), &mut h);
    assert_eq!(r.buffer.text(), "one");
    r.handle_key(key(KeyCode::Up), &mut h);
    assert_eq!(r.buffer.text(), "one");
    r.handle_key(key(KeyCode::Down), &mut h);
    assert_eq!(r.buffer.text(), "二");
    r.handle_key(key(KeyCode::Down), &mut h);
    assert_eq!((r.buffer.text(), r.buffer.cursor()), ("draft", 4));
    assert_eq!(
        r.handle_key(key(KeyCode::Enter), &mut h),
        Outcome::Submitted("draft".into())
    );
    assert_eq!(h.entries(), &["one", "二", "draft"]);
    assert_eq!((r.buffer.text(), r.buffer.cursor()), ("", 0));
    r.handle_key(ctrl('c'), &mut h);
    assert_eq!(h.entries().len(), 3);
    let mut empty = History::new();
    r.handle_key(key(KeyCode::Up), &mut empty);
    assert_eq!(r.buffer.text(), "");
}

#[test]
fn fallback_preserves_data_and_handles_eof() {
    let mut input = Cursor::new(b"a\r\n\n\x1b[31m\t x \nlast\r");
    for expected in ["a", "", "\x1b[31m\t x ", "last\r"] {
        assert_eq!(
            read_from(&mut input).unwrap(),
            Outcome::Submitted(expected.into())
        );
    }
    assert_eq!(read_from(&mut input).unwrap(), Outcome::Eof);
    assert!(read_from(&mut Cursor::new([0xff, b'\n'])).is_err());
}

#[test]
fn fallback_propagates_io_error() {
    struct Broken;
    impl io::Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("broken"))
        }
    }
    impl BufRead for Broken {
        fn fill_buf(&mut self) -> io::Result<&[u8]> {
            Err(io::Error::other("broken"))
        }
        fn consume(&mut self, _: usize) {}
    }
    assert_eq!(read_from(&mut Broken).unwrap_err().to_string(), "broken");
}

#[test]
fn viewport_is_ascii_bounded_and_cursor_visible() {
    let text = "aé🦀\x1b\nlong tail".to_string();
    for cursor in (0..=text.len()).filter(|&i| text.is_char_boundary(i)) {
        let b = LineBuffer::from_parts(text.clone(), cursor);
        for width in 0..40 {
            let (line, column) = viewport("\x1b[31mλ> ", &b, width);
            assert!(line.bytes().all(|b| (32..127).contains(&b)));
            assert!(line.len() <= usize::from(width.saturating_sub(1)));
            assert!(column == 0 || column < width.saturating_sub(1));
        }
    }
}
