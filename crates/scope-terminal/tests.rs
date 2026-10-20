use super::*;
use std::io::Cursor;

#[test]
fn paste_policy_insertion_matrix() {
    assert_eq!(PastePolicy::default(), PastePolicy::SingleLine);
    for payload in [
        "",
        " \tα\n\nβ\t ",
        "a\r\nb\rc",
        "🦀\x1b[31m中\x1b[0m\0\x03\x04",
        "a\x1b]title\x07b",
        "a\x1b[unfinished",
    ] {
        for cursor in [0, 2, 6] {
            for policy in [PastePolicy::SingleLine, PastePolicy::NormalizedLayout] {
                let mut buffer = LineBuffer::from_parts("é🦀".into(), cursor);
                let inserted = match policy {
                    PastePolicy::SingleLine => sanitize(payload),
                    PastePolicy::NormalizedLayout => sanitize_paste(payload),
                };
                buffer.insert_paste(payload, policy);
                assert_eq!(
                    buffer.text(),
                    format!("{}{}{}", &"é🦀"[..cursor], inserted, &"é🦀"[cursor..])
                );
                assert_eq!(buffer.cursor(), cursor + inserted.len());
                if policy == PastePolicy::SingleLine {
                    let mut legacy = LineBuffer::from_parts("é🦀".into(), cursor);
                    legacy.insert(payload);
                    assert_eq!(buffer, legacy);
                }
            }
        }
    }
}

#[test]
fn layout_paste_history_completion_and_lifecycle() {
    let mut reader = Reader::new();
    let mut history = History::new();
    let text = " \tα\n\nβ\t ";
    assert_eq!(
        reader.handle_paste(" \tα\r\n\nβ\t \x03\x04", PastePolicy::NormalizedLayout),
        Outcome::Continue
    );
    assert_eq!(reader.buffer.text(), text);
    assert!(history.entries().is_empty());
    let cursor = reader.buffer.cursor();
    let mut calls = 0;
    assert_eq!(
        reader.complete(&mut |input: &str, offset: usize| {
            calls += 1;
            assert_eq!((input, offset), (text, cursor));
            vec![CompletionCandidate {
                replacement: offset..offset,
                insertion: "\n\t".into(),
                label: None,
            }]
        }),
        CompletionAction::None
    );
    assert_eq!(calls, 1);
    assert_eq!(reader.buffer.cursor(), cursor);
    assert_eq!(
        reader.handle_key(key(KeyCode::Tab), &mut history),
        Outcome::Continue
    );
    assert_eq!(reader.buffer.text(), text);
    // Home/End remain buffer-relative, and nonempty Ctrl-D still deletes.
    reader.handle_key(key(KeyCode::Home), &mut history);
    assert_eq!(reader.buffer.cursor(), 0);
    reader.handle_key(key(KeyCode::End), &mut history);
    assert_eq!(reader.buffer.cursor(), text.len());
    assert_eq!(
        reader.handle_key(ctrl('d'), &mut history),
        Outcome::Continue
    );
    assert_eq!(
        reader.handle_key(key(KeyCode::Enter), &mut history),
        Outcome::Submitted(text.into())
    );
    assert_eq!(history.entries(), &[text]);
    assert_eq!(reader.buffer, LineBuffer::default());

    reader.buffer = LineBuffer::from_parts("é\n\tdraft".into(), 2);
    let draft = reader.buffer.clone();
    reader.handle_key(key(KeyCode::Up), &mut history);
    assert_eq!(reader.buffer.text(), text);
    assert_eq!(reader.buffer.cursor(), text.len());
    reader.handle_paste("edit", PastePolicy::NormalizedLayout);
    reader.handle_key(key(KeyCode::Down), &mut history);
    assert_eq!(reader.buffer, draft);
    for cancel in [ctrl('c'), key(KeyCode::Esc)] {
        reader.handle_key(key(KeyCode::Up), &mut history);
        reader.handle_paste("\n\t", PastePolicy::NormalizedLayout);
        assert_eq!(reader.handle_key(cancel, &mut history), Outcome::Cancelled);
        reader.handle_key(key(KeyCode::Down), &mut history);
        assert_eq!(reader.buffer, LineBuffer::default());
        assert_eq!(reader.handle_key(ctrl('d'), &mut history), Outcome::Eof);
        assert_eq!(history.entries(), &[text]);
    }
    reader.handle_paste("α\nβ", PastePolicy::NormalizedLayout);
    reader.handle_key(key(KeyCode::Home), &mut history);
    assert_eq!(
        reader.handle_key(ctrl('d'), &mut history),
        Outcome::Continue
    );
    assert_eq!(reader.buffer.text(), "\nβ");
    assert_eq!(
        reader.handle_key(key(KeyCode::Enter), &mut history),
        Outcome::Submitted("\nβ".into())
    );
}

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
fn shared_history_navigation_boundaries_and_reset() {
    // A non-Default draft also verifies navigation imposes no Default bound.
    #[derive(Debug, PartialEq, Eq)]
    struct Draft(&'static str);
    let mut navigation = HistoryNavigation::default();
    assert_eq!(navigation.previous(&[], Draft("ignored")), None);
    assert_eq!(navigation.next(&[]), None);
    let entries = vec!["one".into(), "二\nλ".into()];
    assert_eq!(
        navigation.previous(&entries, Draft("original")),
        Some(entries[1].clone())
    );
    assert_eq!(
        navigation.previous(&entries, Draft("edited")),
        Some(entries[0].clone())
    );
    assert_eq!(
        navigation.previous(&entries, Draft("edited")),
        Some(entries[0].clone())
    );
    assert_eq!(
        navigation.next(&entries),
        Some(HistorySelection::Entry(entries[1].clone()))
    );
    assert_eq!(
        navigation.next(&entries),
        Some(HistorySelection::Draft(Draft("original")))
    );
    assert_eq!(navigation.next(&entries), None);
    navigation.previous(&entries, Draft("discarded"));
    navigation.reset();
    assert_eq!(navigation.next(&entries), None);
    navigation.previous(&entries, Draft("fresh"));
    assert_eq!(
        navigation.next(&entries),
        Some(HistorySelection::Draft(Draft("fresh")))
    );
}

#[test]
fn reader_restores_unicode_multiline_draft_cursor_once() {
    let mut history = History::new();
    let mut reader = Reader::new();
    reader.buffer = LineBuffer::from_parts("λ\n二🙂end".into(), "λ\n二".len());
    reader.handle_key(key(KeyCode::Up), &mut history);
    assert_eq!(
        (reader.buffer.text(), reader.buffer.cursor()),
        ("λ\n二🙂end", "λ\n二".len())
    );
    history.push("recalled\nλ");
    reader.handle_key(key(KeyCode::Up), &mut history);
    assert_eq!(reader.buffer.cursor(), "recalled\nλ".len());
    reader.buffer.insert("edit");
    reader.handle_key(key(KeyCode::Down), &mut history);
    assert_eq!(
        (reader.buffer.text(), reader.buffer.cursor()),
        ("λ\n二🙂end", "λ\n二".len())
    );
    reader.buffer.insert("!");
    reader.handle_key(key(KeyCode::Down), &mut history);
    assert_eq!(
        (reader.buffer.text(), reader.buffer.cursor()),
        ("λ\n二!🙂end", "λ\n二!".len())
    );
    assert_eq!(history.entries(), &["recalled\nλ"]);
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

#[test]
fn descriptive_labels_are_display_only() {
    let mut reader = Reader::new();
    reader.buffer = LineBuffer::from_parts("he tail".into(), 2);
    let mut provider = |_: &str, cursor: usize| {
        ["help", "hello"]
            .into_iter()
            .map(|word| CompletionCandidate {
                replacement: 0..cursor,
                insertion: word.into(),
                label: (word == "help").then(|| "help -- declared ( -- n )".into()),
            })
            .collect()
    };
    assert_eq!(reader.complete(&mut provider), CompletionAction::Applied);
    assert_eq!(
        (reader.buffer.text(), reader.buffer.cursor()),
        ("hel tail", 3)
    );
    let before = reader.buffer.clone();
    assert_eq!(
        reader.complete(&mut provider),
        CompletionAction::Display(vec!["help -- declared ( -- n )".into(), "hello".into()])
    );
    assert_eq!(reader.buffer, before);
    let mut unique = |_: &str, _: usize| {
        vec![CompletionCandidate {
            replacement: 0..3,
            insertion: "help".into(),
            label: Some("not inserted\n\x1b[2J".into()),
        }]
    };
    assert_eq!(reader.complete(&mut unique), CompletionAction::Applied);
    assert_eq!(
        (reader.buffer.text(), reader.buffer.cursor()),
        ("help tail", 4)
    );
}

#[test]
fn descriptive_display_is_escaped_ascii_and_bounded() {
    let label = "word -- \x1b[2J\n\r\t\0\u{9b}中e\u{301}🦀";
    for width in 0..120 {
        let line = completion_line(label, width);
        assert!(line.bytes().all(|b| (32..127).contains(&b)));
        assert!(line.len() <= usize::from(width.saturating_sub(1)));
        assert_eq!(
            line,
            escaped(label)
                .chars()
                .take(usize::from(width.saturating_sub(1)))
                .collect::<String>()
        );
    }
    assert_eq!(completion_line("word -- description", 12), "word -- des");
    assert_eq!(completion_line(&"x".repeat(1_000_000), 5), "xxxx");
    assert_eq!(completion_line("", 80), "");
}

fn candidates(input: &str, cursor: usize) -> Vec<CompletionCandidate> {
    ["help", "hello", "halt"]
        .into_iter()
        .filter(|word| word.starts_with(&input[..cursor]))
        .map(|word| CompletionCandidate {
            replacement: 0..cursor,
            insertion: word.into(),
            label: None,
        })
        .collect()
}

#[test]
fn completion_unique_prefix_ambiguous_and_no_match() {
    let mut r = Reader::new();
    r.buffer.insert("he");
    assert_eq!(r.complete(&mut candidates), CompletionAction::Applied);
    assert_eq!((r.buffer.text(), r.buffer.cursor()), ("hel", 3));
    assert_eq!(
        r.complete(&mut candidates),
        CompletionAction::Display(vec!["help".into(), "hello".into()])
    );
    r.buffer.insert("p");
    assert_eq!(r.complete(&mut candidates), CompletionAction::Applied);
    assert_eq!(r.buffer.text(), "help");
    r.buffer.insert("x");
    assert_eq!(r.complete(&mut candidates), CompletionAction::None);
    assert_eq!(r.buffer.text(), "helpx");
}

#[test]
fn completion_replacement_utf8_and_safety() {
    let mut r = Reader::new();
    r.buffer = LineBuffer::from_parts("éx tail".into(), 3);
    let mut provider = |_: &str, _: usize| {
        vec![CompletionCandidate {
            replacement: 2..3,
            insertion: "中".into(),
            label: None,
        }]
    };
    assert_eq!(r.complete(&mut provider), CompletionAction::Applied);
    assert_eq!((r.buffer.text(), r.buffer.cursor()), ("é中 tail", 5));
    let before = r.buffer.clone();
    for range in [1..2, 0..99, 5..2] {
        assert!(!r.buffer.replace(range, "bad"));
        assert_eq!(r.buffer, before);
    }
    assert!(!r.buffer.replace(0..2, "\x1b[31mbad"));
    let mut unicode = |_: &str, _: usize| {
        ["é中", "é串"]
            .into_iter()
            .map(|s| CompletionCandidate {
                replacement: 0..0,
                insertion: s.into(),
                label: None,
            })
            .collect()
    };
    r.buffer.clear();
    assert_eq!(r.complete(&mut unicode), CompletionAction::Applied);
    assert_eq!((r.buffer.text(), r.buffer.cursor()), ("é", 2));
}

#[test]
fn completion_recomputes_after_edit_history_cancel_and_eof() {
    let mut r = Reader::new();
    let mut h = History::new();
    h.push("ha");
    r.buffer.insert("hel");
    assert!(matches!(
        r.complete(&mut candidates),
        CompletionAction::Display(_)
    ));
    r.handle_key(key(KeyCode::Backspace), &mut h);
    r.handle_key(key(KeyCode::Char('x')), &mut h);
    assert_eq!(r.complete(&mut candidates), CompletionAction::None);
    r.handle_key(key(KeyCode::Up), &mut h);
    assert_eq!(r.complete(&mut candidates), CompletionAction::Applied);
    assert_eq!(r.buffer.text(), "halt");
    r.handle_key(key(KeyCode::Down), &mut h);
    assert_eq!(r.buffer.text(), "hex");
    assert_eq!(r.handle_key(ctrl('c'), &mut h), Outcome::Cancelled);
    assert_eq!(r.handle_key(ctrl('d'), &mut h), Outcome::Eof);
    assert_eq!(h.entries(), &["ha"]);
}

#[test]
fn incompatible_ranges_display_and_invalid_candidates_are_ignored() {
    let mut r = Reader::new();
    r.buffer.insert("ab");
    let mut provider = |_: &str, _: usize| {
        vec![
            CompletionCandidate {
                replacement: 0..2,
                insertion: "abc".into(),
                label: Some("first".into()),
            },
            CompletionCandidate {
                replacement: 1..2,
                insertion: "bcd".into(),
                label: None,
            },
            CompletionCandidate {
                replacement: 0..99,
                insertion: "bad".into(),
                label: None,
            },
        ]
    };
    assert_eq!(
        r.complete(&mut provider),
        CompletionAction::Display(vec!["first".into(), "bcd".into()])
    );
    assert_eq!(r.buffer.text(), "ab");
}
