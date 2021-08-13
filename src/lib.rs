const BOM_CHAR: char = '\u{FEFF}';

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineAndColumnIndex {
  /// The zero-indexed line index.
  pub line_index: usize,
  /// The character index relative to the start of the line.
  pub column_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineAndColumnDisplay {
  /// The 1-indexed line number for display purposes.
  pub line_number: usize,
  /// The 1-indexed column number based on the indent width.
  pub column_number: usize,
}

struct TextLine {
  start_index: usize,
  end_index: usize,
  multi_line_chars: Vec<(usize, usize)>,
  tab_chars: Vec<usize>,
}

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
    let mut multi_line_chars = Vec::new();
    let mut tab_chars = Vec::new();
    let mut lines = Vec::new();
    let mut was_last_slash_r = false;
    for (index, c) in text.char_indices() {
      if index == 0 && c == BOM_CHAR {
        continue;
      }

      if c == '\n' {
        lines.push(TextLine {
          start_index: last_line_start,
          end_index: if was_last_slash_r { index - 1 } else { index },
          multi_line_chars: std::mem::replace(&mut multi_line_chars, Vec::new()),
          tab_chars: std::mem::replace(&mut tab_chars, Vec::new()),
        });
        last_line_start = index + 1;
      } else if c == '\t' {
        tab_chars.push(index);
      } else if c.len_utf8() > 1 {
        multi_line_chars.push((index, c.len_utf8()));
      }
      was_last_slash_r = c == '\r';
    }

    lines.push(TextLine {
      start_index: last_line_start,
      end_index: text.len(),
      multi_line_chars,
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
  /// then it will return the index of the line the preceding line.
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

  /// Gets the line and column index of the provided byte index.
  pub fn line_and_column_index(&self, byte_index: usize) -> LineAndColumnIndex {
    // ensure no panics will happen here in case someone is specifying a byte position in the middle of a char
    let line_index = self.line_index(byte_index);
    let line = &self.lines[line_index];

    let relative_byte_index = if byte_index < line.start_index {
      0 // could happen when at the BOM position
    } else {
      byte_index - line.start_index
    };
    let multi_line_char_offset = line
      .multi_line_chars
      .iter()
      .take_while(|(char_index, _)| *char_index < byte_index)
      .map(|(char_index, len)| {
        if char_index + len > byte_index {
          byte_index - char_index
        } else {
          *len - 1
        }
      })
      .sum::<usize>();
    println!("{}", multi_line_char_offset);

    LineAndColumnIndex {
      line_index,
      column_index: relative_byte_index - multi_line_char_offset,
    }
  }

  /// Gets the line and column display based on the indentation width and the provided byte index.
  pub fn line_and_column_display(&self, byte_index: usize) -> LineAndColumnDisplay {
    let line_and_column_index = self.line_and_column_index(byte_index);
    let line = &self.lines[line_and_column_index.line_index];
    let tab_char_count = line
      .tab_chars
      .iter()
      .take_while(|tab_index| **tab_index < byte_index)
      .count();

    LineAndColumnDisplay {
      line_number: line_and_column_index.line_index + 1,
      column_number: line_and_column_index.column_index - tab_char_count
        + tab_char_count * self.indent_width
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

  fn assert_line_start(info: &TextLines, line_index: usize, line_end: usize) {
    assert_eq!(info.line_start(line_index), line_end,);
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
