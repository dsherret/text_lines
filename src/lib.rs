const BOM_CHAR: char = '\u{FEFF}';

#[cfg_attr(
  feature = "serialization",
  derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineAndColumnIndex {
  /// The zero-indexed line index.
  pub line_index: usize,
  /// The character index relative to the start of the line.
  pub column_index: usize,
}

#[cfg_attr(
  feature = "serialization",
  derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineAndColumnDisplay {
  /// The 1-indexed line number for display purposes.
  pub line_number: usize,
  /// The 1-indexed column number based on the indent width.
  pub column_number: usize,
}

#[derive(Debug)]
struct MultiByteCharInfo {
  /// The byte index in the entire file.
  byte_index: usize,
  /// The character index on the current line.
  line_char_index: usize,
  /// The UTF-16 code unit index on the current line.
  line_utf16_index: usize,
  /// The length in bytes.
  length: usize,
}

#[derive(Debug)]
struct TextLine {
  start_index: usize,
  end_index: usize,
  multi_byte_chars: Vec<MultiByteCharInfo>,
  tab_chars: Vec<usize>,
}

#[derive(Debug)]
pub struct TextLines {
  lines: Vec<TextLine>,
  indent_width: usize,
}

impl TextLines {
  /// Creates a new `TextLines` with the specified text and default
  /// indent width of 4.
  pub fn new(text: &str) -> Self {
    TextLines::with_indent_width(text, 4)
  }

  /// Creates a new `TextLines` with the specified text and indent width.
  /// The indent width sets the width of a tab character when getting
  /// the display column.
  pub fn with_indent_width(text: &str, indent_width: usize) -> Self {
    let mut last_line_start = if text.starts_with(BOM_CHAR) {
      BOM_CHAR.len_utf8()
    } else {
      0
    };
    let mut multi_byte_chars = Vec::new();
    let mut tab_chars = Vec::new();
    let mut lines = Vec::new();
    let mut was_last_slash_r = false;
    let mut line_char_index = 0;
    // the number of utf-16 surrogate pairs on the line so far
    let mut line_surrogate_pair_count = 0;
    for (byte_index, c) in text.char_indices() {
      if byte_index == 0 && c == BOM_CHAR {
        continue;
      }

      if c == '\n' {
        lines.push(TextLine {
          start_index: last_line_start,
          end_index: if was_last_slash_r {
            byte_index - 1
          } else {
            byte_index
          },
          multi_byte_chars: std::mem::take(&mut multi_byte_chars),
          tab_chars: std::mem::take(&mut tab_chars),
        });
        last_line_start = byte_index + 1;
        line_char_index = 0;
        line_surrogate_pair_count = 0;
      } else {
        if c == '\t' {
          tab_chars.push(byte_index);
        } else if c.len_utf8() > 1 {
          multi_byte_chars.push(MultiByteCharInfo {
            byte_index,
            line_char_index,
            line_utf16_index: line_char_index + line_surrogate_pair_count,
            length: c.len_utf8(),
          });
          if c.len_utf16() == 2 {
            line_surrogate_pair_count += 1;
          }
        }
        line_char_index += 1;
      }
      was_last_slash_r = c == '\r';
    }

    lines.push(TextLine {
      start_index: last_line_start,
      end_index: text.len(),
      multi_byte_chars,
      tab_chars,
    });

    Self {
      lines,
      indent_width,
    }
  }

  /// Gets the number of lines in the text.
  pub fn lines_count(&self) -> usize {
    self.lines.len()
  }

  /// Gets the text length in bytes.
  pub fn text_length(&self) -> usize {
    self.lines.last().unwrap().end_index
  }

  /// Gets the line index from a byte index.
  /// Note that if you provide the middle byte index of a \r\n newline
  /// then it will return the index of the preceding line.
  pub fn line_index(&self, byte_index: usize) -> usize {
    self.assert_valid_byte_index(byte_index);

    match self
      .lines
      .binary_search_by_key(&byte_index, |line| line.start_index)
    {
      Ok(index) => index,
      Err(insert_index) => {
        if insert_index == 0 {
          0 // may happen when there's a BOM
        } else {
          insert_index - 1
        }
      }
    }
  }

  /// Gets the line start byte index.
  pub fn line_start(&self, line_index: usize) -> usize {
    self.assert_valid_line_index(line_index);
    self.lines[line_index].start_index
  }

  /// Gets the line end byte index (before/at the newline character).
  pub fn line_end(&self, line_index: usize) -> usize {
    self.assert_valid_line_index(line_index);
    self.lines[line_index].end_index
  }

  /// Gets the line range.
  pub fn line_range(&self, line_index: usize) -> (usize, usize) {
    self.assert_valid_line_index(line_index);
    let line = &self.lines[line_index];
    (line.start_index, line.end_index)
  }

  /// Gets the byte position from the provided line and column index.
  pub fn byte_index(&self, line_and_column: LineAndColumnIndex) -> usize {
    let line = &self.lines[line_and_column.line_index];
    let column_index = line_and_column.column_index;
    let prev_char_count = line
      .multi_byte_chars
      .partition_point(|char_info| char_info.line_char_index < column_index);
    let byte_index = match prev_char_count.checked_sub(1) {
      Some(prev_char_index) => {
        let prev_char = &line.multi_byte_chars[prev_char_index];
        // the remaining characters until the column are single byte
        prev_char.end_byte_index() + (column_index - prev_char.line_char_index - 1)
      }
      None => line.start_index + column_index,
    };

    // fallback gracefully to the end index of the line when the column goes off
    byte_index.min(line.end_index)
  }

  /// Gets a byte index from the provided character index.
  pub fn byte_index_from_char_index(&self, char_index: usize) -> usize {
    let mut last_char_index = 0;
    let mut last_byte_index = 0;

    let mut lines = self.lines.iter().peekable();
    while let Some(line) = lines.next() {
      for char_info in &line.multi_byte_chars {
        let char_length = char_info.byte_index - last_byte_index;
        if last_char_index + char_length >= char_index {
          let byte_diff = char_index - last_char_index;
          return last_byte_index + byte_diff;
        } else {
          // move to the position past the character
          last_byte_index = char_info.byte_index + char_info.length;
          last_char_index += char_length + 1;
        }
      }

      // check the end of the line
      let line_end = if let Some(next_line) = lines.peek() {
        next_line.start_index
      } else {
        line.end_index
      };
      let char_length = line_end - last_byte_index;
      if last_char_index + char_length >= char_index {
        let byte_diff = char_index - last_char_index;
        return last_byte_index + byte_diff;
      } else {
        last_byte_index = line_end;
        last_char_index += char_length;
      }
    }

    last_byte_index
  }

  /// Gets a character index from the provided byte index.
  pub fn char_index(&self, byte_index: usize) -> usize {
    let mut lines = self.lines.iter().peekable();
    let mut last_char_index = 0;
    let mut last_byte_index = 0;
    while let Some(line) = lines.next() {
      for char_info in &line.multi_byte_chars {
        if char_info.byte_index >= byte_index {
          let char_length = byte_index - last_byte_index;
          return last_char_index + char_length;
        } else {
          // move to the position past the character
          let char_length = char_info.byte_index - last_byte_index;
          last_byte_index = char_info.byte_index + char_info.length;
          last_char_index += char_length + 1;
        }
      }

      // check the end of the line
      let line_end = if let Some(next_line) = lines.peek() {
        next_line.start_index
      } else {
        line.end_index
      };
      if line_end >= byte_index {
        let char_length = byte_index - last_byte_index;
        return last_char_index + char_length;
      } else {
        let char_length = line_end - last_byte_index;
        last_byte_index = line_end;
        last_char_index += char_length;
      }
    }

    last_char_index
  }

  /// Gets the line and column index of the provided byte index.
  pub fn line_and_column_index(&self, byte_index: usize) -> LineAndColumnIndex {
    let line_index = self.line_index(byte_index);
    LineAndColumnIndex {
      line_index,
      column_index: self.lines[line_index].column_indexes(byte_index).char_index,
    }
  }

  /// Gets the line and column index of the provided byte index where the
  /// column index is the number of UTF-16 code units from the start of the line.
  ///
  /// This differs from `line_and_column_index` for characters outside the
  /// Basic Multilingual Plane (ex. emojis), which are a single character, but
  /// two UTF-16 code units. A byte index in the middle of a character resolves
  /// to the start of that character.
  pub fn line_and_column_index_utf16(&self, byte_index: usize) -> LineAndColumnIndex {
    let line_index = self.line_index(byte_index);
    LineAndColumnIndex {
      line_index,
      column_index: self.lines[line_index]
        .column_indexes(byte_index)
        .utf16_index,
    }
  }

  /// Gets the line and column display based on the indentation width and the provided byte index.
  pub fn line_and_column_display(&self, byte_index: usize) -> LineAndColumnDisplay {
    self.line_and_column_display_with_indent_width(byte_index, self.indent_width)
  }

  /// Gets the line and column display based on the provided byte index and indentation width.
  pub fn line_and_column_display_with_indent_width(
    &self,
    byte_index: usize,
    indent_width: usize,
  ) -> LineAndColumnDisplay {
    let line_and_column_index = self.line_and_column_index(byte_index);
    let line = &self.lines[line_and_column_index.line_index];
    let tab_char_count = line
      .tab_chars
      .partition_point(|tab_index| *tab_index < byte_index);

    LineAndColumnDisplay {
      line_number: line_and_column_index.line_index + 1,
      column_number: line_and_column_index.column_index - tab_char_count
        + tab_char_count * indent_width
        + 1,
    }
  }

  fn assert_valid_byte_index(&self, byte_index: usize) {
    if byte_index > self.text_length() {
      panic!(
        "The specified byte index {} was greater than the text length of {}.",
        byte_index,
        self.text_length()
      )
    }
  }

  fn assert_valid_line_index(&self, line_index: usize) {
    if line_index >= self.lines.len() {
      panic!(
        "The specified line index {} was greater or equal to the number of lines of {}.",
        line_index,
        self.lines.len()
      );
    }
  }
}

struct ColumnIndexes {
  char_index: usize,
  utf16_index: usize,
}

impl TextLine {
  /// Gets the column indexes of the provided byte index on this line.
  ///
  /// A byte index in the middle of a character resolves to the start of
  /// that character.
  fn column_indexes(&self, byte_index: usize) -> ColumnIndexes {
    let prev_char_count = self
      .multi_byte_chars
      .partition_point(|char_info| char_info.byte_index < byte_index);
    match prev_char_count.checked_sub(1) {
      Some(prev_char_index) => {
        let prev_char = &self.multi_byte_chars[prev_char_index];
        let prev_char_end = prev_char.end_byte_index();
        if byte_index < prev_char_end {
          ColumnIndexes {
            char_index: prev_char.line_char_index,
            utf16_index: prev_char.line_utf16_index,
          }
        } else {
          // the remaining characters until the byte index are single byte
          let single_byte_count = byte_index - prev_char_end;
          ColumnIndexes {
            char_index: prev_char.line_char_index + 1 + single_byte_count,
            utf16_index: prev_char.line_utf16_index + prev_char.utf16_len() + single_byte_count,
          }
        }
      }
      None => {
        // the byte index is before the start index when it's in the BOM
        let single_byte_count = byte_index.saturating_sub(self.start_index);
        ColumnIndexes {
          char_index: single_byte_count,
          utf16_index: single_byte_count,
        }
      }
    }
  }
}

impl MultiByteCharInfo {
  fn end_byte_index(&self) -> usize {
    self.byte_index + self.length
  }

  fn utf16_len(&self) -> usize {
    // a char is 4 bytes in utf-8 exactly when it's a surrogate pair in utf-16
    if self.length == 4 { 2 } else { 1 }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn line_and_column_index() {
    let text = "12\n3\r\n4\n5";
    let info = TextLines::new(text);
    assert_line_and_col_index(&info, 0, 0, 0); // 1
    assert_line_and_col_index(&info, 1, 0, 1); // 2
    assert_line_and_col_index(&info, 2, 0, 2); // \n
    assert_line_and_col_index(&info, 3, 1, 0); // 3
    assert_line_and_col_index(&info, 4, 1, 1); // \r
    assert_line_and_col_index(&info, 5, 1, 2); // \n
    assert_line_and_col_index(&info, 6, 2, 0); // 4
    assert_line_and_col_index(&info, 7, 2, 1); // \n
    assert_line_and_col_index(&info, 8, 3, 0); // 5
    assert_line_and_col_index(&info, 9, 3, 1); // <EOF>
  }

  #[test]
  fn line_and_column_index_bom() {
    let text = "\u{FEFF}12\n3";
    let info = TextLines::new(text);
    assert_line_and_col_index(&info, 0, 0, 0); // first BOM index
    assert_line_and_col_index(&info, 1, 0, 0); // second BOM index
    assert_line_and_col_index(&info, 2, 0, 0); // third BOM index
    assert_line_and_col_index(&info, 3, 0, 0); // 1
    assert_line_and_col_index(&info, 4, 0, 1); // 2
    assert_line_and_col_index(&info, 5, 0, 2); // \n
    assert_line_and_col_index(&info, 6, 1, 0); // 3
    assert_line_and_col_index(&info, 7, 1, 1); // <EOF>
  }

  #[test]
  fn line_and_column_index_multi_byte_chars() {
    let text = "β1β\nΔβ1";
    let info = TextLines::new(text);
    assert_line_and_col_index(&info, 0, 0, 0); // first β index
    assert_line_and_col_index(&info, 1, 0, 0); // second β index
    assert_line_and_col_index(&info, 2, 0, 1); // 1
    assert_line_and_col_index(&info, 3, 0, 2); // first β index
    assert_line_and_col_index(&info, 4, 0, 2); // second β index
    assert_line_and_col_index(&info, 5, 0, 3); // \n
    assert_line_and_col_index(&info, 6, 1, 0); // first Δ index
    assert_line_and_col_index(&info, 7, 1, 0); // second Δ index
    assert_line_and_col_index(&info, 8, 1, 1); // first β index
    assert_line_and_col_index(&info, 9, 1, 1); // second β index
    assert_line_and_col_index(&info, 10, 1, 2); // 1
    assert_line_and_col_index(&info, 11, 1, 3); // <EOF>
  }

  fn assert_line_and_col_index(
    info: &TextLines,
    byte_index: usize,
    line_index: usize,
    column_index: usize,
  ) {
    assert_eq!(
      info.line_and_column_index(byte_index),
      LineAndColumnIndex {
        line_index,
        column_index,
      }
    );
  }

  #[test]
  fn line_and_column_index_utf16() {
    // 😀 is 4 bytes in utf-8 and 2 code units in utf-16, β is 2 bytes and 1 code unit
    let text = "a😀b\r\nβ😀😀\n😀";
    let info = TextLines::new(text);
    assert_line_and_col_index_utf16(&info, 0, 0, 0); // a
    assert_line_and_col_index_utf16(&info, 1, 0, 1); // first 😀 index
    assert_line_and_col_index_utf16(&info, 2, 0, 1); // second 😀 index
    assert_line_and_col_index_utf16(&info, 3, 0, 1); // third 😀 index
    assert_line_and_col_index_utf16(&info, 4, 0, 1); // fourth 😀 index
    assert_line_and_col_index_utf16(&info, 5, 0, 3); // b
    assert_line_and_col_index_utf16(&info, 6, 0, 4); // \r
    assert_line_and_col_index_utf16(&info, 7, 0, 5); // \n
    assert_line_and_col_index_utf16(&info, 8, 1, 0); // first β index
    assert_line_and_col_index_utf16(&info, 9, 1, 0); // second β index
    assert_line_and_col_index_utf16(&info, 10, 1, 1); // first 😀 index
    assert_line_and_col_index_utf16(&info, 13, 1, 1); // fourth 😀 index
    assert_line_and_col_index_utf16(&info, 14, 1, 3); // first 😀 index
    assert_line_and_col_index_utf16(&info, 17, 1, 3); // fourth 😀 index
    assert_line_and_col_index_utf16(&info, 18, 1, 5); // \n
    assert_line_and_col_index_utf16(&info, 19, 2, 0); // first 😀 index
    assert_line_and_col_index_utf16(&info, 23, 2, 2); // <EOF>
  }

  #[test]
  fn line_and_column_index_utf16_bom() {
    let text = "\u{FEFF}😀1";
    let info = TextLines::new(text);
    assert_line_and_col_index_utf16(&info, 0, 0, 0); // first BOM index
    assert_line_and_col_index_utf16(&info, 2, 0, 0); // third BOM index
    assert_line_and_col_index_utf16(&info, 3, 0, 0); // first 😀 index
    assert_line_and_col_index_utf16(&info, 7, 0, 2); // 1
    assert_line_and_col_index_utf16(&info, 8, 0, 3); // <EOF>
  }

  #[test]
  fn line_and_column_indexes_match_naive_implementation() {
    for text in [
      "a😀β\tc\n\u{10FFFF}xΔ😀\r\n\n😀",
      "\u{FEFF}β😀1\r\n\t😀Δ\n",
      "ascii only\nlines",
    ] {
      let info = TextLines::new(text);
      for byte_index in 0..=text.len() {
        let line_index = info.line_index(byte_index);
        let line_start = info.line_start(line_index);
        // a byte index in the middle of a char resolves to the start of that char
        let char_start = (0..=byte_index)
          .rev()
          .find(|i| text.is_char_boundary(*i))
          .unwrap()
          .max(line_start);
        let line_text = &text[line_start..char_start];
        assert_line_and_col_index(&info, byte_index, line_index, line_text.chars().count());
        assert_line_and_col_index_utf16(
          &info,
          byte_index,
          line_index,
          line_text.encode_utf16().count(),
        );
        let is_in_line = byte_index >= line_start && byte_index <= info.line_end(line_index);
        if char_start == byte_index && is_in_line {
          assert_byte_index(&info, line_index, line_text.chars().count(), byte_index);
        }
      }
    }
  }

  fn assert_line_and_col_index_utf16(
    info: &TextLines,
    byte_index: usize,
    line_index: usize,
    column_index: usize,
  ) {
    assert_eq!(
      info.line_and_column_index_utf16(byte_index),
      LineAndColumnIndex {
        line_index,
        column_index,
      }
    );
  }

  #[test]
  fn line_and_column_display() {
    let text = "\t1\n\t 3\t4";
    let info = TextLines::new(text);
    assert_line_and_col_display(&info, 0, 1, 1); // \t
    assert_line_and_col_display(&info, 1, 1, 5); // 1
    assert_line_and_col_display(&info, 2, 1, 6); // \n
    assert_line_and_col_display(&info, 3, 2, 1); // \t
    assert_line_and_col_display(&info, 4, 2, 5); // <space>
    assert_line_and_col_display(&info, 5, 2, 6); // 3
    assert_line_and_col_display(&info, 6, 2, 7); // \t
    assert_line_and_col_display(&info, 7, 2, 11); // \t
    assert_line_and_col_display(&info, 8, 2, 12); // <EOF>
  }

  #[test]
  fn line_and_column_display_bom() {
    let text = "\u{FEFF}\t1";
    let info = TextLines::new(text);
    assert_line_and_col_display(&info, 0, 1, 1); // first BOM index
    assert_line_and_col_display(&info, 1, 1, 1); // second BOM index
    assert_line_and_col_display(&info, 2, 1, 1); // third BOM index
    assert_line_and_col_display(&info, 3, 1, 1); // \t
    assert_line_and_col_display(&info, 4, 1, 5); // 1
    assert_line_and_col_display(&info, 5, 1, 6); // <EOF>
  }

  #[test]
  fn line_and_column_display_indent_width() {
    let text = "\t1";
    let info = TextLines::with_indent_width(text, 2);
    assert_line_and_col_display(&info, 0, 1, 1); // \t
    assert_line_and_col_display(&info, 1, 1, 3); // 1
    assert_line_and_col_display(&info, 2, 1, 4); // <EOF>
  }

  fn assert_line_and_col_display(
    info: &TextLines,
    byte_index: usize,
    line_number: usize,
    column_number: usize,
  ) {
    assert_eq!(
      info.line_and_column_display(byte_index),
      LineAndColumnDisplay {
        line_number,
        column_number,
      }
    );
  }

  #[test]
  fn line_and_column_display_with_indent_width() {
    let text = "\t1\n\t 3\t4";
    let info = TextLines::new(text);
    assert_eq!(
      info.line_and_column_display_with_indent_width(1, 2),
      LineAndColumnDisplay {
        line_number: 1,
        column_number: 3,
      }
    );
    assert_eq!(
      info.line_and_column_display_with_indent_width(1, 4),
      LineAndColumnDisplay {
        line_number: 1,
        column_number: 5,
      }
    );
  }

  #[test]
  #[should_panic(expected = "The specified byte index 5 was greater than the text length of 4.")]
  fn line_and_column_index_panic_greater_than() {
    let info = TextLines::new("test");
    info.line_and_column_index(5);
  }

  #[test]
  fn line_start() {
    let text = "12\n3\r\n4\n5";
    let info = TextLines::new(text);
    assert_line_start(&info, 0, 0);
    assert_line_start(&info, 1, 3);
    assert_line_start(&info, 2, 6);
    assert_line_start(&info, 3, 8);
  }

  fn assert_line_start(info: &TextLines, line_index: usize, line_start: usize) {
    assert_eq!(info.line_start(line_index), line_start);
  }

  #[test]
  #[should_panic(
    expected = "The specified line index 1 was greater or equal to the number of lines of 1."
  )]
  fn line_start_equal_number_lines() {
    let info = TextLines::new("test");
    info.line_start(1);
  }

  #[test]
  fn line_end() {
    let text = "12\n3\r\n4\n5";
    let info = TextLines::new(text);
    assert_line_end(&info, 0, 2);
    assert_line_end(&info, 1, 4);
    assert_line_end(&info, 2, 7);
    assert_line_end(&info, 3, 9);
  }

  fn assert_line_end(info: &TextLines, line_index: usize, line_end: usize) {
    assert_eq!(info.line_end(line_index), line_end);
  }

  #[test]
  #[should_panic(
    expected = "The specified line index 1 was greater or equal to the number of lines of 1."
  )]
  fn line_end_equal_number_lines() {
    let info = TextLines::new("test");
    info.line_end(1);
  }

  #[test]
  fn byte_index() {
    let text = "12\n3\r\n4\n5";
    let info = TextLines::new(text);
    assert_byte_index(&info, 0, 0, 0); // 1
    assert_byte_index(&info, 0, 1, 1); // 2
    assert_byte_index(&info, 0, 2, 2); // \n
    assert_byte_index(&info, 0, 3, 2); // passed the \n
    assert_byte_index(&info, 0, 4, 2); // passed the \n
    assert_byte_index(&info, 1, 0, 3); // 3
    assert_byte_index(&info, 1, 1, 4); // \r
    assert_byte_index(&info, 1, 2, 4); // \n
    assert_byte_index(&info, 1, 3, 4); // passed the \r\n
    assert_byte_index(&info, 2, 0, 6); // 4
    assert_byte_index(&info, 2, 1, 7); // \n
    assert_byte_index(&info, 3, 0, 8); // 5
    assert_byte_index(&info, 3, 1, 9); // <EOF>
    assert_byte_index(&info, 3, 2, 9); // passed the<EOF>
  }

  #[test]
  fn byte_index_bom() {
    let text = "\u{FEFF}12\n3";
    let info = TextLines::new(text);
    assert_byte_index(&info, 0, 0, 3); // 1
    assert_byte_index(&info, 0, 1, 4); // 2
    assert_byte_index(&info, 0, 2, 5); // \n
    assert_byte_index(&info, 1, 0, 6); // 3
    assert_byte_index(&info, 1, 1, 7); // <EOF>
  }

  #[test]
  fn byte_index_bom_multi_byte_chars() {
    let text = "\u{FEFF}β1\nβ";
    let info = TextLines::new(text);
    assert_byte_index(&info, 0, 0, 3); // first β index
    assert_byte_index(&info, 0, 1, 5); // 1
    assert_byte_index(&info, 0, 2, 6); // \n
    assert_byte_index(&info, 1, 0, 7); // first β index
    assert_byte_index(&info, 1, 1, 9); // <EOF>
  }

  #[test]
  fn byte_index_multi_byte_chars() {
    let text = "β1β\nΔβ1";
    let info = TextLines::new(text);
    assert_byte_index(&info, 0, 0, 0); // first β index
    assert_byte_index(&info, 0, 1, 2); // 1
    assert_byte_index(&info, 0, 2, 3); // first β index
    assert_byte_index(&info, 0, 3, 5); // \n
    assert_byte_index(&info, 1, 0, 6); // first Δ index
    assert_byte_index(&info, 1, 1, 8); // first β index
    assert_byte_index(&info, 1, 2, 10); // 1
    assert_byte_index(&info, 1, 3, 11); // <EOF>
  }

  fn assert_byte_index(
    info: &TextLines,
    line_index: usize,
    column_index: usize,
    byte_index: usize,
  ) {
    assert_eq!(
      info.byte_index(LineAndColumnIndex {
        line_index,
        column_index,
      }),
      byte_index,
    );
  }

  #[test]
  fn byte_index_from_char_index() {
    let text = "1234\n567\r\n8\n";
    let info = TextLines::new(text);
    assert_byte_index_from_char_index(&info, 0, 0); // 1
    assert_byte_index_from_char_index(&info, 1, 1); // 2
    assert_byte_index_from_char_index(&info, 2, 2); // 3
    assert_byte_index_from_char_index(&info, 3, 3); // 4
    assert_byte_index_from_char_index(&info, 4, 4); // \n
    assert_byte_index_from_char_index(&info, 5, 5); // 5
    assert_byte_index_from_char_index(&info, 6, 6); // 6
    assert_byte_index_from_char_index(&info, 7, 7); // 7
    assert_byte_index_from_char_index(&info, 8, 8); // \r
    assert_byte_index_from_char_index(&info, 9, 9); // \n
    assert_byte_index_from_char_index(&info, 10, 10); // 8
    assert_byte_index_from_char_index(&info, 11, 11); // \n
    assert_byte_index_from_char_index(&info, 12, 12); // <EOF>
    assert_byte_index_from_char_index(&info, 13, 12); // <EOF> + 1
  }

  #[test]
  fn byte_index_from_char_index_multi_byte_chars() {
    let text = "β1β\nΔβ1\r\nt\nu";
    let info = TextLines::new(text);
    assert_byte_index_from_char_index(&info, 0, 0); // β
    assert_byte_index_from_char_index(&info, 1, 2); // 1
    assert_byte_index_from_char_index(&info, 2, 3); // β
    assert_byte_index_from_char_index(&info, 3, 5); // \n
    assert_byte_index_from_char_index(&info, 4, 6); // Δ
    assert_byte_index_from_char_index(&info, 5, 8); // β
    assert_byte_index_from_char_index(&info, 6, 10); // 1
    assert_byte_index_from_char_index(&info, 7, 11); // \r
    assert_byte_index_from_char_index(&info, 8, 12); // \n
    assert_byte_index_from_char_index(&info, 9, 13); // t
    assert_byte_index_from_char_index(&info, 10, 14); // \n
    assert_byte_index_from_char_index(&info, 11, 15); // u
    assert_byte_index_from_char_index(&info, 12, 16); // <EOF>
    assert_byte_index_from_char_index(&info, 13, 16); // <EOF> + 1
  }

  fn assert_byte_index_from_char_index(info: &TextLines, char_index: usize, byte_index: usize) {
    assert_eq!(info.byte_index_from_char_index(char_index), byte_index,);
  }

  #[test]
  fn char_index() {
    let text = "1234\n567\r\n8\n";
    let info = TextLines::new(text);
    assert_char_index(&info, 0, 0); // 1
    assert_char_index(&info, 1, 1); // 2
    assert_char_index(&info, 2, 2); // 3
    assert_char_index(&info, 3, 3); // 4
    assert_char_index(&info, 4, 4); // \n
    assert_char_index(&info, 5, 5); // 5
    assert_char_index(&info, 6, 6); // 6
    assert_char_index(&info, 7, 7); // 7
    assert_char_index(&info, 8, 8); // \r
    assert_char_index(&info, 9, 9); // \n
    assert_char_index(&info, 10, 10); // 8
    assert_char_index(&info, 11, 11); // \n
    assert_char_index(&info, 12, 12); // <EOF>
    assert_char_index(&info, 13, 12); // <EOF> + 1
  }

  #[test]
  fn char_index_multi_byte_chars() {
    let text = "β1β\nΔβ1\r\nt\nu";
    let info = TextLines::new(text);
    assert_char_index(&info, 0, 0); // β
    assert_char_index(&info, 2, 1); // 1
    assert_char_index(&info, 3, 2); // β
    assert_char_index(&info, 5, 3); // \n
    assert_char_index(&info, 6, 4); // Δ
    assert_char_index(&info, 8, 5); // β
    assert_char_index(&info, 10, 6); // 1
    assert_char_index(&info, 11, 7); // \r
    assert_char_index(&info, 12, 8); // \n
    assert_char_index(&info, 13, 9); // t
    assert_char_index(&info, 14, 10); // \n
    assert_char_index(&info, 15, 11); // u
    assert_char_index(&info, 16, 12); // <EOF>
    assert_char_index(&info, 17, 12); // <EOF> + 1
  }

  fn assert_char_index(info: &TextLines, char_index: usize, byte_index: usize) {
    assert_eq!(info.char_index(char_index), byte_index,);
  }

  #[test]
  fn readme_example() {
    let text = "Line 1\n\tLine 2";
    let info = TextLines::new(&text);

    assert_eq!(info.line_index(9), 1);
    assert_eq!(
      info.line_and_column_index(9),
      LineAndColumnIndex {
        line_index: 1,
        column_index: 2,
      }
    );
    assert_eq!(
      info.line_and_column_display(9),
      LineAndColumnDisplay {
        line_number: 2,
        column_number: 6,
      }
    );

    let info = TextLines::with_indent_width(&text, 2);
    assert_eq!(
      info.line_and_column_display(9),
      LineAndColumnDisplay {
        line_number: 2,
        column_number: 4,
      }
    );
  }
}
