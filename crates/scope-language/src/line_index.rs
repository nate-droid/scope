use std::ops::Range;

use crate::{Position, TextRange};

/// Checked adapters for parser byte offsets and half-open byte spans.
///
/// The index borrows one immutable document snapshot; rebuild it when text
/// changes. Construction takes O(bytes) time and O(lines) space. Offset lookup
/// takes O(log lines) time; position lookup takes O(1) time.
///
/// Lines start at byte zero and immediately after each LF (`\n`). Columns are
/// zero-based UTF-8 byte offsets, not character, grapheme, or UTF-16 indices.
/// LF itself is at the preceding line's final column; the byte after LF is
/// exclusively column zero of the next line. CR (`\r`) is an ordinary byte,
/// including in CRLF: both the position before CR and between CR and LF are
/// valid. No newline normalization is performed.
///
/// EOF is valid. A trailing LF creates an empty final line; an empty document
/// has only position `(0, 0)`. Columns cannot spill into the next line, so every
/// accepted offset/position roundtrips exactly. All conversions return `None`
/// for out-of-bounds values, non-UTF-8 boundaries, or reversed ranges; they never
/// clamp. Empty and multiline ranges are accepted, preserving `[start, end)`.
///
/// # Adapting parser output
///
/// Parsers commonly return `Range<usize>` (or start/end byte fields). Convert
/// spans against the exact source snapshot passed to the parser:
///
/// ```
/// use scope_language::{Diagnostic, LineIndex, Symbol, SymbolKind};
///
/// let source = "def café\n";
/// let index = LineIndex::new(source);
/// let parser_name_span = 4..9; // UTF-8 bytes of café
/// let range = index.range_for_span(parser_name_span.clone())
///     .expect("parser span must be valid for this source");
/// let diagnostic = Diagnostic::warning(range, "unused definition");
/// let symbol = Symbol {
///     name: source[parser_name_span.clone()].to_owned(),
///     kind: SymbolKind::Function,
///     range,
/// };
/// assert_eq!(index.span_for_range(symbol.range), Some(parser_name_span));
/// assert_eq!(diagnostic.range, symbol.range);
/// ```
#[derive(Debug, Clone)]
pub struct LineIndex<'a> {
    text: &'a str,
    line_starts: Vec<usize>,
}

impl<'a> LineIndex<'a> {
    /// Index an immutable UTF-8 source snapshot.
    pub fn new(text: &'a str) -> Self {
        let mut line_starts = vec![0];
        line_starts.extend(
            text.bytes()
                .enumerate()
                .filter_map(|(offset, byte)| (byte == b'\n').then_some(offset + 1)),
        );
        Self { text, line_starts }
    }

    /// Convert a byte offset, including EOF, to its canonical position.
    pub fn position_for_byte(&self, byte: usize) -> Option<Position> {
        if !self.text.is_char_boundary(byte) {
            return None;
        }
        let line = self.line_starts.partition_point(|&start| start <= byte) - 1;
        Some(Position::new(line, byte - self.line_starts[line]))
    }

    /// Convert a canonical position to a byte offset, including EOF.
    pub fn byte_offset(&self, position: Position) -> Option<usize> {
        let start = *self.line_starts.get(position.line)?;
        let end = self
            .line_starts
            .get(position.line + 1)
            .map_or(self.text.len(), |next| next - 1);
        if position.column > end - start {
            return None;
        }
        let byte = start + position.column;
        self.text.is_char_boundary(byte).then_some(byte)
    }

    /// Adapt a parser's half-open `Range<usize>` into a checked text range.
    pub fn range_for_span(&self, span: Range<usize>) -> Option<TextRange> {
        if span.start > span.end {
            return None;
        }
        Some(TextRange::new(
            self.position_for_byte(span.start)?,
            self.position_for_byte(span.end)?,
        ))
    }

    /// Convert a checked half-open text range back to a byte span.
    pub fn span_for_range(&self, range: TextRange) -> Option<Range<usize>> {
        if range.start > range.end {
            return None;
        }
        Some(self.byte_offset(range.start)?..self.byte_offset(range.end)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newline_and_eof_positions_are_canonical() {
        for (text, positions) in [
            ("", vec![(0, 0)]),
            ("a", vec![(0, 0), (0, 1)]),
            ("a\nb", vec![(0, 0), (0, 1), (1, 0), (1, 1)]),
            ("a\n", vec![(0, 0), (0, 1), (1, 0)]),
            ("\n\n", vec![(0, 0), (1, 0), (2, 0)]),
            ("a\r\nb", vec![(0, 0), (0, 1), (0, 2), (1, 0), (1, 1)]),
            ("\r\n", vec![(0, 0), (0, 1), (1, 0)]),
            ("a\r", vec![(0, 0), (0, 1), (0, 2)]),
        ] {
            let index = LineIndex::new(text);
            for (byte, (line, column)) in positions.into_iter().enumerate() {
                let position = Position::new(line, column);
                assert_eq!(index.position_for_byte(byte), Some(position), "{text:?}");
                assert_eq!(index.byte_offset(position), Some(byte), "{text:?}");
            }
            assert_eq!(index.position_for_byte(text.len() + 1), None);
        }
    }

    #[test]
    fn unicode_boundaries_and_all_valid_spans_roundtrip() {
        let text = "é\r\n🦀\n";
        let index = LineIndex::new(text);
        assert_eq!(index.position_for_byte(4), Some(Position::new(1, 0)));
        assert_eq!(index.position_for_byte(8), Some(Position::new(1, 4)));
        for start in 0..=text.len() {
            assert_eq!(
                index.position_for_byte(start).is_some(),
                text.is_char_boundary(start)
            );
            for end in start..=text.len() {
                let span = start..end;
                let converted = index.range_for_span(span.clone());
                if text.is_char_boundary(start) && text.is_char_boundary(end) {
                    assert_eq!(index.span_for_range(converted.unwrap()), Some(span));
                } else {
                    assert_eq!(converted, None);
                }
            }
        }
        assert_eq!(index.byte_offset(Position::new(0, 1)), None);
        assert_eq!(index.byte_offset(Position::new(1, 1)), None);
    }

    #[test]
    fn invalid_positions_and_ranges_are_rejected_without_overflow() {
        let index = LineIndex::new("é\n");
        for position in [
            Position::new(0, 3),
            Position::new(1, 1),
            Position::new(2, 0),
            Position::new(usize::MAX, 0),
            Position::new(0, usize::MAX),
        ] {
            assert_eq!(index.byte_offset(position), None);
            assert_eq!(
                index.span_for_range(TextRange::new(position, position)),
                None
            );
        }
        assert_eq!(index.position_for_byte(usize::MAX), None);
        let reversed = std::ops::Range { start: 2, end: 1 };
        for span in [reversed, 0..4, 1..2, usize::MAX..usize::MAX] {
            assert_eq!(index.range_for_span(span), None);
        }
        assert_eq!(
            index.span_for_range(TextRange::new(Position::new(1, 0), Position::new(0, 0))),
            None
        );
        let empty = LineIndex::new("");
        assert_eq!(
            empty.range_for_span(0..0),
            Some(TextRange::new(Position::new(0, 0), Position::new(0, 0)))
        );
        assert_eq!(empty.byte_offset(Position::new(0, 1)), None);
        assert_eq!(empty.byte_offset(Position::new(1, 0)), None);
    }
}
