use std::ops::Range;

/// State for the find-and-replace search bar.
/// Pure data + algorithms — no GPUI dependency.
pub struct FindState {
    pub visible: bool,
    pub query: String,
    pub replace_text: String,
    pub matches: Vec<Range<usize>>,
    pub current_match: Option<usize>,
    pub match_case: bool,
    pub replace_visible: bool,
    /// When true, keyboard input goes to the search/replace input instead of the editor.
    pub input_focused: bool,
    /// When true, the replace input field is focused (else search input).
    pub replace_input_focused: bool,
    /// Cached compiled regex to avoid recompilation on every search.
    compiled_re: Option<regex::Regex>,
    /// The query string that was used to compile the cached regex.
    compiled_query: String,
    /// The case mode that was used to compile the cached regex.
    compiled_case: bool,
}

impl FindState {
    pub fn new() -> Self {
        Self {
            visible: false,
            query: String::new(),
            replace_text: String::new(),
            matches: Vec::new(),
            current_match: None,
            match_case: false,
            replace_visible: false,
            input_focused: false,
            replace_input_focused: false,
            compiled_re: None,
            compiled_query: String::new(),
            compiled_case: false,
        }
    }

    /// Search the full text for all matches of the current query.
    /// Called whenever the query or match_case changes.
    /// Caches the compiled regex to avoid recompilation on every keystroke.
    pub fn search(&mut self, text: &str) {
        self.matches.clear();
        self.current_match = None;
        if self.query.is_empty() {
            return;
        }
        let re = if self.compiled_query == self.query && self.compiled_case == self.match_case {
            // Reuse cached regex
            self.compiled_re.as_ref().unwrap()
        } else {
            let pattern = regex::escape(&self.query);
            let compiled = if self.match_case {
                regex::Regex::new(&pattern).unwrap()
            } else {
                regex::Regex::new(&format!("(?i){}", pattern)).unwrap()
            };
            self.compiled_query = self.query.clone();
            self.compiled_case = self.match_case;
            self.compiled_re = Some(compiled);
            self.compiled_re.as_ref().unwrap()
        };
        self.matches = re.find_iter(text).map(|m| m.range()).collect();
        if !self.matches.is_empty() {
            self.current_match = Some(0);
        }
    }

    /// Reset all state (close the bar).
    pub fn close(&mut self) {
        self.visible = false;
        self.query.clear();
        self.replace_text.clear();
        self.matches.clear();
        self.current_match = None;
        self.input_focused = false;
        self.replace_input_focused = false;
        self.replace_visible = false;
        self.match_case = false;
    }

    /// Move to the next match. Wraps around.
    pub fn find_next(&mut self) -> Option<usize> {
        if self.matches.is_empty() {
            return None;
        }
        let next = match self.current_match {
            Some(i) => (i + 1) % self.matches.len(),
            None => 0,
        };
        self.current_match = Some(next);
        Some(next)
    }

    /// Move to the previous match. Wraps around.
    pub fn find_prev(&mut self) -> Option<usize> {
        if self.matches.is_empty() {
            return None;
        }
        let prev = match self.current_match {
            Some(i) => {
                if i == 0 {
                    self.matches.len() - 1
                } else {
                    i - 1
                }
            }
            None => self.matches.len() - 1,
        };
        self.current_match = Some(prev);
        Some(prev)
    }

    /// Number of matches found.
    #[allow(dead_code)]
    pub fn match_count(&self) -> usize {
        self.matches.len()
    }

    /// Reset all state (close the bar).
    pub fn close(&mut self) {
        self.visible = false;
        self.query.clear();
        self.replace_text.clear();
        self.matches.clear();
        self.current_match = None;
        self.input_focused = false;
        self.replace_input_focused = false;
        self.replace_visible = false;
        self.match_case = false;
    }

    /// Returns the byte range of the current match, if any.
    pub fn current_match_range(&self) -> Option<Range<usize>> {
        self.current_match.map(|i| self.matches[i].clone())
    }

    /// For a given line byte range, return (inline_highlight_ranges, current_match_range)
    /// where ranges are relative to the line start.
    #[allow(dead_code)]
    pub fn highlights_for_line(
        &self,
        line_start: usize,
        line_end: usize,
    ) -> (Vec<Range<usize>>, Option<Range<usize>>) {
        if self.matches.is_empty() || self.query.is_empty() {
            return (Vec::new(), None);
        }
        let mut highlights = Vec::new();
        let mut current = None;
        for (i, m) in self.matches.iter().enumerate() {
            if m.start >= line_end || m.end <= line_start {
                continue;
            }
            let rel_start = m.start.saturating_sub(line_start);
            let rel_end = m.end.saturating_sub(line_start);
            let rel_end = rel_end.min(line_end - line_start);
            if rel_start < rel_end {
                highlights.push(rel_start..rel_end);
            }
            if Some(i) == self.current_match {
                current = Some(rel_start..rel_end);
            }
        }
        (highlights, current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_query_finds_nothing() {
        let mut fs = FindState::new();
        fs.search("hello world");
        assert!(fs.matches.is_empty());
        assert_eq!(fs.match_count(), 0);
    }

    #[test]
    fn test_basic_search() {
        let mut fs = FindState::new();
        fs.query = "hello".to_string();
        fs.search("hello world hello");
        assert_eq!(fs.match_count(), 2);
        assert_eq!(fs.matches[0], 0..5);
        assert_eq!(fs.matches[1], 12..17);
        assert_eq!(fs.current_match, Some(0));
    }

    #[test]
    fn test_case_sensitive() {
        let mut fs = FindState::new();
        fs.query = "Hello".to_string();
        fs.match_case = true;
        fs.search("hello Hello HELLO");
        assert_eq!(fs.match_count(), 1);
        assert_eq!(fs.matches[0], 6..11);
    }

    #[test]
    fn test_case_insensitive() {
        let mut fs = FindState::new();
        fs.query = "hello".to_string();
        fs.match_case = false;
        fs.search("hello Hello HELLO");
        assert_eq!(fs.match_count(), 3);
    }

    #[test]
    fn test_find_next_wraps() {
        let mut fs = FindState::new();
        fs.query = "a".to_string();
        fs.search("a b a");
        assert_eq!(fs.match_count(), 2);
        assert_eq!(fs.current_match, Some(0));
        assert_eq!(fs.find_next(), Some(1));
        assert_eq!(fs.find_next(), Some(0));
    }

    #[test]
    fn test_find_prev_wraps() {
        let mut fs = FindState::new();
        fs.query = "a".to_string();
        fs.search("a b a");
        assert_eq!(fs.current_match, Some(0));
        assert_eq!(fs.find_prev(), Some(1));
        assert_eq!(fs.find_prev(), Some(0));
    }

    #[test]
    fn test_close_resets_state() {
        let mut fs = FindState::new();
        fs.visible = true;
        fs.query = "test".to_string();
        fs.replace_text = "new".to_string();
        fs.search("test test");
        assert!(fs.match_count() > 0);
        fs.close();
        assert!(!fs.visible);
        assert!(fs.query.is_empty());
        assert!(fs.matches.is_empty());
        assert_eq!(fs.match_count(), 0);
        assert!(!fs.match_case);
    }

    #[test]
    fn test_search_with_regex_special_chars() {
        let mut fs = FindState::new();
        fs.query = "(a+b)".to_string();
        fs.search("(a+b) test (a+b)");
        assert_eq!(fs.match_count(), 2);
    }

    #[test]
    fn test_current_match_range() {
        let mut fs = FindState::new();
        assert!(fs.current_match_range().is_none());
        fs.query = "x".to_string();
        fs.search("x y x");
        assert_eq!(fs.current_match_range(), Some(0..1));
        fs.find_next();
        assert_eq!(fs.current_match_range(), Some(4..5));
    }

    #[test]
    fn test_highlights_for_line() {
        let mut fs = FindState::new();
        fs.query = "ab".to_string();
        fs.search("ab xxx ab yyy ab");
        // Matches: 0..2, 7..9, 14..16
        // Line covers bytes 0-16 (entire text)
        let (ranges, current) = fs.highlights_for_line(0, 16);
        assert_eq!(ranges.len(), 3);
        assert_eq!(current, Some(0..2));

        // Line covers bytes 0-4 (only first match)
        let (ranges, current) = fs.highlights_for_line(0, 4);
        assert_eq!(ranges.len(), 1);
        assert_eq!(current, Some(0..2));

        // Line covers bytes 4-10 (second match at 7..9)
        let (ranges, current) = fs.highlights_for_line(4, 10);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0], 3..5); // 7-4=3, 9-4=5
        assert!(current.is_none());

        // Line with no matches
        let (ranges, current) = fs.highlights_for_line(16, 20);
        assert!(ranges.is_empty());
        assert!(current.is_none());
    }
}
