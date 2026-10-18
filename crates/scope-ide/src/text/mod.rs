use std::ops::Range as ByteRange;

pub use scope_language::{Position, TextRange};

/// A half-open byte span in a UTF-8 text document: `[start, end)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub const fn len(self) -> usize {
        self.end - self.start
    }

    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }

    pub fn into_range(self) -> ByteRange<usize> {
        self.start..self.end
    }
}

impl From<ByteRange<usize>> for Span {
    fn from(range: ByteRange<usize>) -> Self {
        Self::new(range.start, range.end)
    }
}

impl From<Span> for ByteRange<usize> {
    fn from(span: Span) -> Self {
        span.into_range()
    }
}

/// A replacement edit against a text range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub range: TextRange,
    pub replacement: String,
}

impl TextEdit {
    pub fn replace(range: TextRange, replacement: impl Into<String>) -> Self {
        Self {
            range,
            replacement: replacement.into(),
        }
    }

    pub fn insert(position: Position, text: impl Into<String>) -> Self {
        Self::replace(TextRange::new(position, position), text)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextBufferError {
    InvalidRange { range: TextRange },
    InvalidSpan { span: Span },
    PositionOutOfBounds { position: Position },
    SpanOutOfBounds { span: Span },
    ColumnNotOnCharBoundary { position: Position },
    SpanNotOnCharBoundary { span: Span },
}

impl std::fmt::Display for TextBufferError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRange { range } => write!(formatter, "invalid text range: {range:?}"),
            Self::InvalidSpan { span } => write!(formatter, "invalid byte span: {span:?}"),
            Self::PositionOutOfBounds { position } => {
                write!(formatter, "position is out of bounds: {position:?}")
            }
            Self::SpanOutOfBounds { span } => {
                write!(formatter, "byte span is out of bounds: {span:?}")
            }
            Self::ColumnNotOnCharBoundary { position } => {
                write!(
                    formatter,
                    "position does not lie on a UTF-8 character boundary: {position:?}"
                )
            }
            Self::SpanNotOnCharBoundary { span } => {
                write!(
                    formatter,
                    "byte span does not lie on UTF-8 character boundaries: {span:?}"
                )
            }
        }
    }
}

impl std::error::Error for TextBufferError {}

/// An owned UTF-8 text document with line/column based edit operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextBuffer {
    text: String,
}

impl TextBuffer {
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn into_string(self) -> String {
        self.text
    }

    pub fn line_count(&self) -> usize {
        self.text.lines().count().max(1)
    }

    pub fn line(&self, line: usize) -> Option<&str> {
        self.text.lines().nth(line)
    }

    pub fn apply_edit(&mut self, edit: TextEdit) -> Result<(), TextBufferError> {
        let span = self.span_for_range(edit.range)?;
        self.text
            .replace_range(span.into_range(), &edit.replacement);
        Ok(())
    }

    pub fn apply_edits(
        &mut self,
        edits: impl IntoIterator<Item = TextEdit>,
    ) -> Result<(), TextBufferError> {
        let mut ranges = edits
            .into_iter()
            .map(|edit| {
                self.span_for_range(edit.range)
                    .map(|span| (span.into_range(), edit.replacement))
            })
            .collect::<Result<Vec<_>, _>>()?;

        ranges.sort_by_key(|(range, _)| range.start);

        for pair in ranges.windows(2) {
            if pair[0].0.end > pair[1].0.start {
                return Err(TextBufferError::InvalidRange {
                    range: TextRange::new(
                        self.position_for_valid_byte(pair[1].0.start),
                        self.position_for_valid_byte(pair[0].0.end),
                    ),
                });
            }
        }

        for (range, replacement) in ranges.into_iter().rev() {
            self.text.replace_range(range, &replacement);
        }

        Ok(())
    }

    pub fn span_for_range(&self, range: TextRange) -> Result<Span, TextBufferError> {
        if range.start > range.end {
            return Err(TextBufferError::InvalidRange { range });
        }

        let start = self.byte_offset(range.start)?;
        let end = self.byte_offset(range.end)?;

        Ok(Span::new(start, end))
    }

    pub fn range_for_span(&self, span: Span) -> Result<TextRange, TextBufferError> {
        if span.start > span.end {
            return Err(TextBufferError::InvalidSpan { span });
        }

        Ok(TextRange::new(
            self.position_for_byte(span.start, span)?,
            self.position_for_byte(span.end, span)?,
        ))
    }

    fn byte_offset(&self, position: Position) -> Result<usize, TextBufferError> {
        let mut line_start = 0;

        for (line_number, line) in self.text.split_inclusive('\n').enumerate() {
            let line_without_newline = line.strip_suffix('\n').unwrap_or(line);

            if line_number == position.line {
                if position.column > line_without_newline.len() {
                    return Err(TextBufferError::PositionOutOfBounds { position });
                }

                let byte = line_start + position.column;
                if !self.text.is_char_boundary(byte) {
                    return Err(TextBufferError::ColumnNotOnCharBoundary { position });
                }

                return Ok(byte);
            }

            line_start += line.len();
        }

        if position.line == self.text.bytes().filter(|byte| *byte == b'\n').count()
            && position.column == 0
            && line_start == self.text.len()
        {
            return Ok(self.text.len());
        }

        Err(TextBufferError::PositionOutOfBounds { position })
    }

    fn position_for_byte(&self, byte: usize, span: Span) -> Result<Position, TextBufferError> {
        if byte > self.text.len() {
            return Err(TextBufferError::SpanOutOfBounds { span });
        }

        if !self.text.is_char_boundary(byte) {
            return Err(TextBufferError::SpanNotOnCharBoundary { span });
        }

        Ok(self.position_for_valid_byte(byte))
    }

    fn position_for_valid_byte(&self, byte: usize) -> Position {
        let mut line = 0;
        let mut line_start = 0;

        for (index, character) in self.text.char_indices() {
            if index >= byte {
                break;
            }

            if character == '\n' {
                line += 1;
                line_start = index + character.len_utf8();
            }
        }

        Position::new(line, byte - line_start)
    }
}

impl From<String> for TextBuffer {
    fn from(text: String) -> Self {
        Self::new(text)
    }
}

impl From<&str> for TextBuffer {
    fn from(text: &str) -> Self {
        Self::new(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eof_ranges_round_trip_and_allow_replacement() {
        for source in ["", "é", "é\n", "\n\n"] {
            let mut buffer = TextBuffer::new(source);
            let span = Span::new(0, source.len());
            let range = buffer.range_for_span(span).unwrap();
            assert_eq!(buffer.span_for_range(range).unwrap(), span);
            buffer
                .apply_edit(TextEdit::replace(range, "replacement\n"))
                .unwrap();
            assert_eq!(buffer.as_str(), "replacement\n");
        }
    }

    #[test]
    fn applies_insert_edit() {
        let mut buffer = TextBuffer::new("hello world");
        buffer
            .apply_edit(TextEdit::insert(Position::new(0, 5), ","))
            .unwrap();

        assert_eq!(buffer.as_str(), "hello, world");
    }

    #[test]
    fn applies_replace_edit_across_lines() {
        let mut buffer = TextBuffer::new("one\ntwo\nthree");
        buffer
            .apply_edit(TextEdit::replace(
                TextRange::new(Position::new(0, 1), Position::new(2, 2)),
                "---",
            ))
            .unwrap();

        assert_eq!(buffer.as_str(), "o---ree");
    }

    #[test]
    fn applies_multiple_edits_from_original_positions() {
        let mut buffer = TextBuffer::new("alpha beta gamma");
        buffer
            .apply_edits([
                TextEdit::replace(
                    TextRange::new(Position::new(0, 0), Position::new(0, 5)),
                    "A",
                ),
                TextEdit::replace(
                    TextRange::new(Position::new(0, 11), Position::new(0, 16)),
                    "G",
                ),
            ])
            .unwrap();

        assert_eq!(buffer.as_str(), "A beta G");
    }

    #[test]
    fn rejects_column_inside_multibyte_character() {
        let mut buffer = TextBuffer::new("é");
        let error = buffer
            .apply_edit(TextEdit::insert(Position::new(0, 1), "x"))
            .unwrap_err();

        assert_eq!(
            error,
            TextBufferError::ColumnNotOnCharBoundary {
                position: Position::new(0, 1)
            }
        );
    }

    #[test]
    fn converts_text_ranges_to_byte_spans() {
        let buffer = TextBuffer::new("aé\nbc");

        assert_eq!(
            buffer
                .span_for_range(TextRange::new(Position::new(0, 1), Position::new(1, 1)))
                .unwrap(),
            Span::new(1, 5)
        );
    }

    #[test]
    fn converts_byte_spans_to_text_ranges() {
        let buffer = TextBuffer::new("aé\nbc");

        assert_eq!(
            buffer.range_for_span(Span::new(1, 5)).unwrap(),
            TextRange::new(Position::new(0, 1), Position::new(1, 1))
        );
    }

    #[test]
    fn rejects_spans_inside_multibyte_character() {
        let buffer = TextBuffer::new("é");
        let error = buffer.range_for_span(Span::new(1, 2)).unwrap_err();

        assert_eq!(
            error,
            TextBufferError::SpanNotOnCharBoundary {
                span: Span::new(1, 2)
            }
        );
    }
}
