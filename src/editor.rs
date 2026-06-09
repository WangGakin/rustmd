use gpui::*;
use std::ops::Range;

const FONT_SIZE: Pixels = px(16.0);
const PADDING: Pixels = px(8.0);

pub struct Editor {
    text: String,
    cursor_utf16: usize,
    marked_range: Option<Range<usize>>,
    focus_handle: FocusHandle,
}

impl Editor {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        Self {
            text: "Hello, RustMD!\n\nStart typing here...\n\n中文输入测试。".into(),
            cursor_utf16: 0,
            marked_range: None,
            focus_handle,
        }
    }

    fn utf16_to_char_offset(&self, utf16_offset: usize) -> usize {
        let mut utf16_count = 0;
        for (char_offset, ch) in self.text.char_indices() {
            if utf16_count >= utf16_offset {
                return char_offset;
            }
            utf16_count += ch.len_utf16();
        }
        self.text.len()
    }

    fn insert_at_utf16(&mut self, utf16_offset: usize, insert_text: &str) {
        let char_offset = self.utf16_to_char_offset(utf16_offset);
        self.text.insert_str(char_offset, insert_text);
    }

    fn delete_range_utf16(&mut self, range: Range<usize>) {
        if range.is_empty() {
            return;
        }
        let start = self.utf16_to_char_offset(range.start);
        let end = self.utf16_to_char_offset(range.end);
        self.text.replace_range(start..end, "");
    }

    fn text_for_utf16_range(&self, range: Range<usize>) -> Option<String> {
        let start = self.utf16_to_char_offset(range.start);
        let end = self.utf16_to_char_offset(range.end);
        if start <= self.text.len() && end <= self.text.len() {
            Some(self.text[start..end].to_string())
        } else {
            None
        }
    }

    fn render_content(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let text = &self.text;
        let marked = self.marked_range.clone();
        let cursor = self.cursor_utf16;

        let mut lines: Vec<AnyElement> = Vec::new();
        let mut line_utf16_offset = 0usize;

        for line_str in text.split('\n') {
            let line_utf16_len = line_str.encode_utf16().count();
            let line_end = line_utf16_offset + line_utf16_len;

            let marked_tuple = marked.as_ref().map(|r| (r.start, r.end));
            let cursor_on_line = cursor >= line_utf16_offset && cursor <= line_end;

            let line_content = render_text_with_mark(line_str, marked_tuple, line_utf16_offset);

            if cursor_on_line {
                let rel_cursor = cursor - line_utf16_offset;
                let char_width = FONT_SIZE * 0.6;
                let cursor_x = char_width * rel_cursor as f32;

                lines.push(
                    div()
                        .flex()
                        .child(line_content)
                        .child(
                            div()
                                .absolute()
                                .left(cursor_x)
                                .top(px(0.0))
                                .w(px(2.0))
                                .h(px(18.0))
                                .bg(rgb(0xcdd6f4)),
                        )
                        .into_any_element(),
                );
            } else {
                lines.push(div().flex().child(line_content).into_any_element());
            }

            line_utf16_offset = line_end + 1;
        }

        div()
            .size_full()
            .bg(rgb(0x1e1e2e))
            .text_color(rgb(0xcdd6f4))
            .text_size(FONT_SIZE)
            .p(PADDING)
            .track_focus(&self.focus_handle)
            .child(div().flex().flex_col().relative().children(lines))
    }
}

fn render_text_with_mark(
    line_str: &str,
    marked_range: Option<(usize, usize)>,
    line_utf16_offset: usize,
) -> impl IntoElement {
    if let Some((m_start, m_end)) = marked_range {
        if m_end <= line_utf16_offset {
            return div().child(line_str.to_string());
        }
        let rel_start = m_start.saturating_sub(line_utf16_offset);
        let rel_end = (m_end - line_utf16_offset).min(line_str.encode_utf16().count());

        if rel_start < rel_end {
            let mut utf16_count = 0usize;
            let mut char_start = 0usize;
            let mut char_end = line_str.len();

            for (i, c) in line_str.char_indices() {
                if utf16_count == rel_start && char_start == 0 {
                    char_start = i;
                }
                if utf16_count == rel_end {
                    char_end = i;
                    break;
                }
                utf16_count += c.len_utf16();
            }

            let before = &line_str[..char_start];
            let marked = &line_str[char_start..char_end];
            let after = &line_str[char_end..];

            return div()
                .flex()
                .flex_row()
                .child(before.to_string())
                .child(
                    div()
                        .bg(rgba(0x4455ddaa))
                        .underline()
                        .child(marked.to_string()),
                )
                .child(after.to_string());
        }
    }

    div().child(line_str.to_string())
}

impl EntityInputHandler for Editor {
    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.cursor_utf16..self.cursor_utf16,
            reversed: false,
        })
    }

    fn marked_text_range(&self, _window: &mut Window, _cx: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_range.clone()
    }

    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        self.text_for_utf16_range(range_utf16)
    }

    fn replace_text_in_range(
        &mut self,
        replacement_range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        // Windows: WM_CHAR messages arrive even during IME composition,
        // inserting raw pinyin letters into the buffer. GPUI's platform
        // layer doesn't filter them. When composition is active, single
        // ASCII characters from stale WM_CHAR must be discarded.
        // Legitimate IME confirmation (GCS_RESULTSTR) delivers multi-byte
        // Chinese text and should replace the composition range.
        if self.marked_range.is_some() && replacement_range.is_none() {
            let is_ascii = text.len() == 1 && text.as_bytes()[0].is_ascii_alphabetic();
            if is_ascii || text.is_empty() {
                return;
            }
            // IME confirmation: replace composition text
            let range = self.marked_range.take().unwrap();
            self.delete_range_utf16(range.clone());
            self.insert_at_utf16(range.start, text);
            self.cursor_utf16 = range.start + text.encode_utf16().count();
            window.refresh();
            return;
        }
        self.marked_range = None;
        if let Some(range) = replacement_range {
            self.delete_range_utf16(range.clone());
            self.insert_at_utf16(range.start, text);
            self.cursor_utf16 = range.start + text.encode_utf16().count();
        } else {
            let cursor = self.cursor_utf16;
            self.insert_at_utf16(cursor, text);
            self.cursor_utf16 = cursor + text.encode_utf16().count();
        }
        window.refresh();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        let new_utf16_len = new_text.encode_utf16().count();
        if new_utf16_len == 0 {
            return;
        }

        let (delete_from, delete_to) = if let Some(range) = range_utf16 {
            (range.start, range.end)
        } else if let Some(mark) = self.marked_range.clone() {
            // Replace existing composition text with the updated version
            (mark.start, mark.end)
        } else {
            // First composition — the WM_CHAR that arrived before this IME
            // event already inserted the raw pinyin letter. Replace it.
            let before = self.cursor_utf16.saturating_sub(new_utf16_len);
            (before, self.cursor_utf16)
        };

        self.marked_range = None;
        self.delete_range_utf16(delete_from..delete_to);
        self.insert_at_utf16(delete_from, new_text);

        self.marked_range = Some(delete_from..delete_from + new_utf16_len);

        if let Some(sel_range) = new_selected_range {
            self.cursor_utf16 = delete_from + sel_range.start;
        } else {
            self.cursor_utf16 = delete_from + new_utf16_len;
        }
        window.refresh();
    }

    fn unmark_text(&mut self, window: &mut Window, _cx: &mut Context<Self>) {
        self.marked_range = None;
        window.refresh();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let char_width = FONT_SIZE * 0.6;
        let char_offset = self.utf16_to_char_offset(range_utf16.start);
        let x = element_bounds.origin.x + PADDING + char_width * char_offset as f32;
        let y = element_bounds.origin.y + PADDING;
        Some(Bounds::new(point(x, y), size(char_width, px(22.0))))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let char_width = FONT_SIZE * 0.6;
        let col = ((point.x - PADDING) / char_width).max(0.0);
        Some(col.round() as usize)
    }
}

impl Editor {
    pub fn focus_handle_ref(&self) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_content(window, cx)
    }
}

impl Focusable for Editor {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

pub struct EditorElement {
    entity: Entity<Editor>,
}

impl EditorElement {
    pub fn new(entity: Entity<Editor>) -> Self {
        Self { entity }
    }
}

impl Element for EditorElement {
    type RequestLayoutState = AnyElement;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(ElementId::View(self.entity.entity_id()))
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut child = self.entity.update(cx, |editor, cx| {
            editor.render_content(window, cx).into_any_element()
        });
        let layout_id = child.request_layout(window, cx);
        (layout_id, child)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        child: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) {
        child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        child: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let entity = self.entity.clone();
        let focus_handle = entity.update(cx, |editor, _cx| editor.focus_handle.clone());
        let fh = focus_handle.clone();
        window.on_mouse_event(move |_: &MouseDownEvent, _phase, window, _cx| {
            window.focus(&fh);
        });
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, entity),
            cx,
        );
        child.paint(window, cx);
    }
}

impl IntoElement for EditorElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}
