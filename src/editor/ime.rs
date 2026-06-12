use gpui::*;
use std::ops::Range;
use crate::cursor::Selection;

pub fn content_from_file(path: &str) -> String {
    use crate::buffer::Buffer;
    if let Ok((buffer, _)) = Buffer::from_file(std::path::Path::new(path)) {
        buffer.text()
    } else {
        String::from("# Hello, RustMD\n\nStart typing here.\n\n中文输入测试。\n\n- list item 1\n- list item 2\n\n```rust\nfn main() {\n    println!(\"hello\");\n}\n```\n")
    }
}

fn byte_to_utf16(s: &str, byte_offset: usize) -> usize {
    let offset = byte_offset.min(s.len());
    s[..offset].encode_utf16().count()
}

fn utf16_to_byte(s: &str, utf16_offset: usize) -> usize {
    let mut count = 0;
    for (i, ch) in s.char_indices() {
        if count >= utf16_offset { return i; }
        count += ch.len_utf16();
    }
    s.len()
}

use crate::editor::Editor;

impl EntityInputHandler for Editor {
    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        let offset = self.state.cursor().offset;
        let full = self.state.buffer.text();
        let offset = offset.min(full.len());
        let u = byte_to_utf16(&full, offset);
        Some(UTF16Selection { range: u..u, reversed: false })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        if let Some(ref mark) = self.ime_marked_range {
            let full = self.state.buffer.text();
            let s = mark.start.min(full.len());
            let e = mark.end.min(full.len());
            Some(byte_to_utf16(&full, s)..byte_to_utf16(&full, e))
        } else {
            None
        }
    }

    fn text_for_range(&mut self, r: Range<usize>, _: &mut Option<Range<usize>>, _: &mut Window, _: &mut Context<Self>) -> Option<String> {
        let full = self.state.buffer.text();
        let a = utf16_to_byte(&full, r.start);
        let b = utf16_to_byte(&full, r.end);
        if a <= full.len() && b <= full.len() { Some(full[a..b].to_string()) } else { None }
    }

    fn replace_text_in_range(&mut self, replacement: Option<Range<usize>>, text: &str, _w: &mut Window, cx: &mut Context<Self>) {
        // ── IME composition active ──
        if self.ime_marked_range.is_some() && replacement.is_none() {
            let ascii = text.len() == 1 && text.as_bytes()[0].is_ascii_alphabetic();
            if ascii {
                return;
            }
            // IME cancellation: empty text means composition was aborted
            if text.is_empty() {
                if let Some(mark) = self.ime_marked_range.take() {
                    self.state.buffer.delete(mark.clone(), mark.end);
                    self.state.selection = Selection::new(mark.start, mark.start);
                }
                cx.notify();
                return;
            }
            // IME confirmation: replace composition text
            let mark = self.ime_marked_range.take().unwrap();
            let new_end = self.state.buffer.replace(mark.clone(), text, mark.start);
            self.state.selection = Selection::new(new_end, new_end);
            self.sync_list_state(cx);
            cx.notify();
            return;
        }

        // ── No composition ──
        // writ's on_key_down already inserted ASCII chars from KeyDown events.
        // WM_CHAR for ASCII is a duplicate — skip it.
        // Non-ASCII text (e.g. Chinese punctuation via WM_CHAR/WM_IME_CHAR)
        // was NOT inserted by on_key_down, so accept it here.
        if replacement.is_none() {
            // ASCII alpha/digit — already handled by on_key_down, skip duplicate
            if text.len() == 1 && text.as_bytes()[0].is_ascii_alphanumeric() {
                return;
            }
            // ASCII punct — same as above, on_key_down already inserted
            if text.len() == 1 && text.as_bytes()[0].is_ascii() {
                return;
            }
            // Non-ASCII text (e.g. Chinese punctuation or IME confirmation)
            let mut cursor = self.state.cursor().offset;
            // on_key_down may have inserted the raw ASCII key before the
            // IME event arrived (e.g. ',' before '，'). Remove it.
            if cursor > 0 {
                let prev = self.state.buffer.byte_at(cursor - 1);
                if prev.is_some_and(|b| b.is_ascii_punctuation()) {
                    let prev_start = self.state.buffer.prev_char_boundary(cursor);
                    self.state.buffer.delete(prev_start..cursor, cursor);
                    cursor = prev_start;
                }
            }
            // Some IMEs (e.g. Shouxin) send confirmation via replace_text_in_range
            // without ever setting ime_marked_range. Detect ASCII letters before
            // cursor and treat them as unmarked composition text to replace.
            let mut composition_start = cursor;
            while composition_start > 0 {
                let b = self.state.buffer.byte_at(composition_start - 1);
                if b.is_some_and(|b| b.is_ascii_alphabetic()) {
                    composition_start -= 1;
                } else {
                    break;
                }
            }
            let new_end = if composition_start < cursor && !text.is_empty() {
                self.state.buffer.replace(composition_start..cursor, text, composition_start)
            } else {
                self.state.buffer.insert(cursor, text, cursor)
            };
            self.state.selection = Selection::new(new_end, new_end);
            self.sync_list_state(cx);
            cx.notify();
            return;
        }

        // Explicit replacement range (rare for WM_CHAR)
        if let Some(r) = replacement {
            self.ime_marked_range = None;
            let full = self.state.buffer.text();
            let a = utf16_to_byte(&full, r.start);
            let b = utf16_to_byte(&full, r.end);
            let new_end = self.state.buffer.replace(a..b, text, a);
            self.state.selection = Selection::new(new_end, new_end);
            self.sync_list_state(cx);
            cx.notify();
        }
    }

    fn replace_and_mark_text_in_range(&mut self, range: Option<Range<usize>>, new: &str, _sel: Option<Range<usize>>, _w: &mut Window, cx: &mut Context<Self>) {
        let full = self.state.buffer.text();
        let new_len = new.len();
        // IME cancellation: empty composition string means IME was aborted
        if new_len == 0 {
            if let Some(mark) = self.ime_marked_range.take() {
                self.state.buffer.delete(mark.clone(), mark.end);
                self.state.selection = Selection::new(mark.start, mark.start);
            }
            cx.notify();
            return;
        }

        let cursor = self.state.cursor().offset;

        let (from, to) = if let Some(r) = range {
            (utf16_to_byte(&full, r.start), utf16_to_byte(&full, r.end))
        } else if let Some(mark) = self.ime_marked_range.clone() {
            // writ's on_key_down may have inserted extra text AFTER the previous
            // composition range (e.g. pinyin "a" after "d"). Expand the replace
            // region to cover everything up to the current cursor.
            (mark.start, cursor.max(mark.end))
        } else {
            // First composition char — writ already inserted it via key handler.
            // Replace what writ inserted with the (same) composition text.
            let before = cursor.saturating_sub(new_len);
            (before, cursor)
        };

        self.ime_marked_range = None;
        let new_end = self.state.buffer.replace(from..to, new, from);
        self.ime_marked_range = Some(from..from + new_len);
        self.state.selection = Selection::new(new_end, new_end);
        self.sync_list_state(cx);
        cx.notify();
    }

    fn unmark_text(&mut self, _w: &mut Window, cx: &mut Context<Self>) {
        if let Some(mark) = self.ime_marked_range.take() {
            let full = self.state.buffer.text();
            if mark.end <= full.len() {
                let marked_text = &full[mark.start..mark.end];
                // Only delete if the marked text is still ASCII letters (pinyin).
                // If it contains non-ASCII, it was likely already replaced by
                // confirmation text and we must not delete it.
                if !marked_text.is_empty() && marked_text.bytes().all(|b| b.is_ascii_alphabetic()) {
                    self.state.buffer.delete(mark.clone(), mark.end);
                    self.state.selection = Selection::new(mark.start, mark.start);
                }
            }
        }
        cx.notify();
    }

    fn bounds_for_range(&mut self, _r: Range<usize>, eb: Bounds<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<Bounds<Pixels>> {
        Some(Bounds::new(point(eb.origin.x + px(32.0), eb.origin.y + px(8.0)), size(px(16.0), px(22.0))))
    }

    fn character_index_for_point(&mut self, p: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        let x = (p.x - px(32.0)).to_f64().max(0.0);
        Some((x / 10.0) as usize)
    }
}

// ── EditorImeElement ─────────────────────────────

pub struct EditorImeElement {
    entity: Entity<Editor>,
}

impl EditorImeElement {
    pub fn new(entity: Entity<Editor>) -> Self { Self { entity } }
}

impl Element for EditorImeElement {
    type RequestLayoutState = AnyElement;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> { Some(ElementId::View(self.entity.entity_id())) }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> { None }

    fn request_layout(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, w: &mut Window, cx: &mut App) -> (LayoutId, Self::RequestLayoutState) {
        let mut child = self.entity.update(cx, |e, cx| e.render(w, cx).into_any_element());
        let lid = child.request_layout(w, cx);
        (lid, child)
    }

    fn prepaint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, _: Bounds<Pixels>, c: &mut Self::RequestLayoutState, w: &mut Window, cx: &mut App) {
        c.prepaint(w, cx);
    }

    fn paint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, bounds: Bounds<Pixels>, child: &mut Self::RequestLayoutState, _: &mut Self::PrepaintState, w: &mut Window, cx: &mut App) {
        let entity = self.entity.clone();
        let fh = entity.read(cx).focus_handle.clone();
        w.handle_input(&fh, ElementInputHandler::new(bounds, entity), cx);
        child.paint(w, cx);
    }
}

impl IntoElement for EditorImeElement {
    type Element = Self;
    fn into_element(self) -> Self::Element { self }
}
