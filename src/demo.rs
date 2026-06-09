use std::time::Duration;

use crate::editor::EditorAction;

#[derive(Clone, Debug)]
pub enum DemoStep {
    Action(EditorAction),
    Type(String),
    Wait(u64),
}

pub fn demo_script() -> Vec<DemoStep> {
    use DemoStep::*;
    use EditorAction as A;

    vec![
        // === INTRO ===
        Type("# Welcome to writ".into()),
        Action(A::Enter),
        Wait(400),
        Type("A markdown editor for people who know markdown.".into()),
        Action(A::Enter),
        Action(A::Enter),
        Wait(600),
        // === INLINE FORMATTING ===
        Type("## Inline Formatting".into()),
        Action(A::Enter),
        Wait(400),
        Type("Write **bold**, _italic_, `code`, and ~~strikethrough~~.".into()),
        Action(A::Enter),
        Type("Syntax hides when you're not editing it.".into()),
        Action(A::Enter),
        Action(A::Enter),
        Wait(800),
        // === LISTS ===
        Type("## Lists".into()),
        Action(A::Enter),
        Wait(400),
        Type("- First item".into()),
        Action(A::ShiftEnter), // continue list
        Type("Second item".into()),
        Action(A::ShiftEnter),
        Type("Third item".into()),
        Action(A::Enter), // raw newline exits
        Action(A::Enter),
        Wait(600),
        // Ordered lists
        Type("1. Ordered lists".into()),
        Action(A::ShiftEnter),
        Type("Auto-number with Shift+Enter".into()),
        Action(A::ShiftEnter),
        Type("Keep going".into()),
        Action(A::Enter),
        Action(A::Enter),
        Wait(600),
        // Task lists
        Type("- [x] Unchecked task".into()),
        Action(A::Enter),
        Type("- [ ] Checked task".into()),
        Action(A::ShiftEnter),
        Type("Click to toggle".into()),
        Action(A::Enter),
        Action(A::Enter),
        Wait(800),
        // === NESTING WITH TAB ===
        Type("## Nesting".into()),
        Action(A::Enter),
        Wait(400),
        Type("- Parent item".into()),
        Action(A::Enter),
        Action(A::Tab), // cycles to indent
        Action(A::Tab), // cycles to nested marker
        Type("Nested with Tab".into()),
        Action(A::ShiftEnter),
        Type("Shift+Tab to unnest".into()),
        Action(A::Enter),
        Action(A::ShiftTab), // cycle back
        Action(A::ShiftTab),
        Type("- Back to top".into()),
        Action(A::Enter),
        Action(A::Enter),
        Wait(800),
        // === BLOCKQUOTES ===
        Type("## Blockquotes".into()),
        Action(A::Enter),
        Wait(400),
        Type("> Quotes show a border instead of `>` markers.".into()),
        Action(A::ShiftEnter),
        Type("Continue with Shift+Enter.".into()),
        Action(A::Enter),
        Action(A::Enter),
        Wait(600),
        // Nested blockquote with list
        Type("> Nesting works too:".into()),
        Action(A::ShiftEnter),
        Type("- List inside quote".into()),
        Action(A::ShiftEnter),
        Type("Another item".into()),
        Action(A::Enter),
        Action(A::Enter),
        Wait(800),
        // === CODE ===
        Type("## Code".into()),
        Action(A::Enter),
        Wait(400),
        Type("```rust".into()),
        Action(A::Enter),
        Type("fn main() {".into()),
        Action(A::Enter),
        Type("    println!(\"Hello, writ!\");".into()),
        Action(A::Enter),
        Type("}".into()),
        Action(A::Enter),
        Type("```".into()),
        Action(A::Enter),
        Wait(600),
        Type("Fences hide when cursor is outside.".into()),
        Action(A::Enter),
        Action(A::Enter),
        Wait(800),
        // === LINKS & IMAGES ===
        Type("## Links & Images".into()),
        Action(A::Enter),
        Wait(400),
        Type("Check out [the repo](https://github.com/wilfreddenton/writ)!".into()),
        Action(A::Enter),
        Action(A::Enter),
        Type("![Hello, World!](https://upload.wikimedia.org/wikipedia/commons/9/97/The_Earth_seen_from_Apollo_17.jpg)".into()),
        Wait(2000),
        Action(A::Enter),
        Action(A::Enter),
        // === PHILOSOPHY ===
        Type("---".into()),
        Action(A::Enter),
        Type("_No magic. No surprises. Just markdown._".into()),
        Wait(1000),
        Action(A::Enter),
    ]
}

pub struct DemoTiming {
    pub char_delay: Duration,
    pub key_delay: Duration,
}

impl Default for DemoTiming {
    fn default() -> Self {
        Self {
            char_delay: Duration::from_millis(50),
            key_delay: Duration::from_millis(150),
        }
    }
}
