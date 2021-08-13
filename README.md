# text_lines

[![](https://img.shields.io/crates/v/text_lines.svg)](https://crates.io/crates/text_lines)

Information about lines of text in a string.

```rust
use text_lines::TextLines;

let text = "Line 1\n\tLine 2";
let info = TextLines::new(&text); // defaults to an indent width of 4

let line_index = info.line_index(9); // 1
let line_and_column = info.line_and_column_index(9); // 1, 2
let line_and_column_display = info.line_and_column_display(9); // 2, 6

let info = TextLines::with_indent_width(&text, 2);
let line_and_column_display = info.line_and_column_display(9); // 2, 4
```
