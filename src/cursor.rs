use std::ops::Range;

use crate::buffer::Buffer;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Cursor {
    pub offset: usize,
}

impl Cursor {
    pub fn new(offset: usize) -> Self {
        Self { offset }
    }

    pub fn start() -> Self {
        Self { offset: 0 }
    }

    pub fn end(buffer: &Buffer) -> Self {
        Self {
            offset: buffer.len_bytes(),
        }
    }

    pub fn clamp(&self, buffer: &Buffer) -> Self {
        Self {
            offset: self.offset.min(buffer.len_bytes()),
        }
    }

    /// Move cursor left. Markers are atomic - cursor jumps over entire marker.
    /// Blank lines are not skipped.
    pub fn move_left(&self, buffer: &Buffer) -> Self {
        if self.offset == 0 {
            return *self;
        }

        let current_line_idx = buffer.byte_to_line(self.offset);

        let line = buffer.line_markers(current_line_idx);
        for marker in &line.markers {
            if self.offset == marker.range.end {
                return Self {
                    offset: marker.range.start,
                };
            }
        }

        if self.offset == line.range.start {
            if current_line_idx > 0 {
                let prev_line_range = buffer.line_byte_range(current_line_idx - 1);
                return Self {
                    offset: prev_line_range.end,
                };
            }
            return *self;
        }

        let rope = buffer.rope();
        let char_idx = rope.byte_to_char(self.offset);
        if char_idx == 0 {
            return *self;
        }
        Self {
            offset: rope.char_to_byte(char_idx - 1),
        }
    }

    /// Move cursor right. Markers are atomic - cursor jumps over entire marker.
    /// Blank lines are not skipped.
    pub fn move_right(&self, buffer: &Buffer) -> Self {
        let len = buffer.len_bytes();
        if self.offset >= len {
            return *self;
        }

        let current_line_idx = buffer.byte_to_line(self.offset);

        let line = buffer.line_markers(current_line_idx);
        for marker in line.markers.iter().rev() {
            if self.offset == marker.range.start {
                return Self {
                    offset: marker.range.end,
                };
            }
        }

        let rope = buffer.rope();
        let char_idx = rope.byte_to_char(self.offset);
        let char_count = rope.len_chars();
        if char_idx >= char_count {
            return *self;
        }
        Self {
            offset: rope.char_to_byte(char_idx + 1),
        }
    }

    pub fn move_up(&self, buffer: &Buffer) -> Self {
        let current_line = buffer.byte_to_line(self.offset);
        if current_line == 0 {
            return Self::start();
        }

        let target_line = current_line - 1;
        let line_start = buffer.line_to_byte(current_line);
        let column = self.offset - line_start;

        let target_line_range = buffer.line_byte_range(target_line);
        let target_line_start = target_line_range.start;
        let target_line_len = target_line_range.len();

        let new_column = column.min(target_line_len);
        Self {
            offset: target_line_start + new_column,
        }
    }

    pub fn move_down(&self, buffer: &Buffer) -> Self {
        let current_line = buffer.byte_to_line(self.offset);
        let line_count = buffer.line_count();

        if current_line >= line_count - 1 {
            return Self::end(buffer);
        }

        let target_line = current_line + 1;
        let line_start = buffer.line_to_byte(current_line);
        let column = self.offset - line_start;

        let target_line_range = buffer.line_byte_range(target_line);
        let target_line_start = target_line_range.start;
        let target_line_len = target_line_range.len();

        let new_column = column.min(target_line_len);
        Self {
            offset: target_line_start + new_column,
        }
    }

    pub fn move_to_line_start(&self, buffer: &Buffer) -> Self {
        let current_line = buffer.byte_to_line(self.offset);
        Self {
            offset: buffer.line_to_byte(current_line),
        }
    }

    pub fn move_to_line_end(&self, buffer: &Buffer) -> Self {
        let current_line = buffer.byte_to_line(self.offset);
        let line_range = buffer.line_byte_range(current_line);
        Self {
            offset: line_range.end,
        }
    }

    pub fn move_to_start(&self) -> Self {
        Self::start()
    }

    pub fn move_to_end(&self, buffer: &Buffer) -> Self {
        Self::end(buffer)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
}

impl Selection {
    pub fn new(anchor: usize, head: usize) -> Self {
        Self { anchor, head }
    }

    pub fn from_cursor(cursor: Cursor) -> Self {
        Self {
            anchor: cursor.offset,
            head: cursor.offset,
        }
    }

    pub fn is_collapsed(&self) -> bool {
        self.anchor == self.head
    }

    pub fn cursor(&self) -> Cursor {
        Cursor::new(self.head)
    }

    pub fn range(&self) -> Range<usize> {
        if self.anchor <= self.head {
            self.anchor..self.head
        } else {
            self.head..self.anchor
        }
    }

    pub fn extend_to(&self, new_head: usize) -> Self {
        Self {
            anchor: self.anchor,
            head: new_head,
        }
    }

    pub fn collapse(&self) -> Self {
        Self {
            anchor: self.head,
            head: self.head,
        }
    }

    pub fn collapse_to_start(&self) -> Self {
        let start = self.range().start;
        Self {
            anchor: start,
            head: start,
        }
    }

    pub fn collapse_to_end(&self) -> Self {
        let end = self.range().end;
        Self {
            anchor: end,
            head: end,
        }
    }

    pub fn clamp(&self, buffer: &Buffer) -> Self {
        let len = buffer.len_bytes();
        Self {
            anchor: self.anchor.min(len),
            head: self.head.min(len),
        }
    }

    pub fn select_all(buffer: &Buffer) -> Self {
        Self {
            anchor: 0,
            head: buffer.len_bytes(),
        }
    }

    pub fn select_word_at(offset: usize, buffer: &Buffer) -> Self {
        let rope = buffer.rope();
        let len_bytes = buffer.len_bytes();

        if len_bytes == 0 || offset >= len_bytes {
            return Self::new(offset.min(len_bytes), offset.min(len_bytes));
        }

        let is_word_char = |c: char| c.is_alphanumeric() || c == '_';
        let char_idx = rope.byte_to_char(offset);
        let char_count = rope.len_chars();

        if char_idx >= char_count {
            return Self::new(offset, offset);
        }

        let c = rope.char(char_idx);

        if !is_word_char(c) {
            let char_end = rope.char_to_byte(char_idx + 1);
            return Self::new(offset, char_end.min(len_bytes));
        }

        let mut start_char_idx = char_idx;
        for i in (0..char_idx).rev() {
            if is_word_char(rope.char(i)) {
                start_char_idx = i;
            } else {
                break;
            }
        }

        let mut end_char_idx = char_idx + 1;
        for i in (char_idx + 1)..char_count {
            if is_word_char(rope.char(i)) {
                end_char_idx = i + 1;
            } else {
                break;
            }
        }

        let start_byte = rope.char_to_byte(start_char_idx);
        let end_byte = rope.char_to_byte(end_char_idx);
        Self::new(start_byte, end_byte)
    }

    pub fn select_line_at(offset: usize, buffer: &Buffer) -> Self {
        let line = buffer.byte_to_line(offset);
        let line_range = buffer.line_byte_range(line);
        Self::new(line_range.start, line_range.end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Cursor movement tests are in editor/mod.rs using the | cursor style.
    // These tests cover Selection data structure behavior.

    #[test]
    fn test_selection_range() {
        let sel = Selection::new(5, 10);
        assert_eq!(sel.range(), 5..10);

        // Reversed selection
        let sel_rev = Selection::new(10, 5);
        assert_eq!(sel_rev.range(), 5..10);
    }

    #[test]
    fn test_selection_is_collapsed() {
        let sel = Selection::new(5, 5);
        assert!(sel.is_collapsed());

        let sel2 = Selection::new(5, 10);
        assert!(!sel2.is_collapsed());
    }

    #[test]
    fn test_selection_extend() {
        let sel = Selection::new(5, 10);
        let extended = sel.extend_to(15);
        assert_eq!(extended.anchor, 5);
        assert_eq!(extended.head, 15);
    }

    #[test]
    fn test_selection_collapse() {
        let sel = Selection::new(5, 10);

        let collapsed = sel.collapse();
        assert_eq!(collapsed.anchor, 10);
        assert_eq!(collapsed.head, 10);

        let to_start = sel.collapse_to_start();
        assert_eq!(to_start.anchor, 5);
        assert_eq!(to_start.head, 5);

        let to_end = sel.collapse_to_end();
        assert_eq!(to_end.anchor, 10);
        assert_eq!(to_end.head, 10);
    }

    #[test]
    fn test_selection_select_all() {
        let buf: Buffer = "hello world".parse().unwrap();
        let sel = Selection::select_all(&buf);
        assert_eq!(sel.anchor, 0);
        assert_eq!(sel.head, 11);
    }

    #[test]
    fn test_selection_select_word_at() {
        let buf: Buffer = "hello world test".parse().unwrap();

        // Click in middle of "hello"
        let sel = Selection::select_word_at(2, &buf);
        assert_eq!(sel.range(), 0..5); // "hello"

        // Click in middle of "world"
        let sel = Selection::select_word_at(8, &buf);
        assert_eq!(sel.range(), 6..11); // "world"

        // Click on space (non-word char)
        let sel = Selection::select_word_at(5, &buf);
        assert_eq!(sel.range(), 5..6); // just the space
    }

    #[test]
    fn test_selection_select_line_at() {
        let buf: Buffer = "line one\nline two\nline three".parse().unwrap();

        // Click on first line (excludes newline)
        let sel = Selection::select_line_at(3, &buf);
        assert_eq!(sel.range(), 0..8); // "line one"

        // Click on second line (excludes newline)
        let sel = Selection::select_line_at(12, &buf);
        assert_eq!(sel.range(), 9..17); // "line two"

        // Click on last line (no trailing newline)
        let sel = Selection::select_line_at(22, &buf);
        assert_eq!(sel.range(), 18..28); // "line three"
    }
}
