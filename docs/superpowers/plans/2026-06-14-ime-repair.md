# IME 输入修复方案

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix two Chinese IME input bugs — (1) pinyin encoding leaks into editor during composition, (2) double punctuation when typing Chinese punctuation marks.

**Root cause:** GPUI 0.2.2's Windows platform calls `ImmGetVirtualKey` on `VK_PROCESSKEY` (`events.rs:1345`), recovering the original key and producing a `KeyDownEvent` with `key_char`. This causes `on_key_down` to insert ASCII text that conflicts with IME composition updates arriving via `WM_IME_COMPOSITION`. Zed's newer GPUI drops `VK_PROCESSKEY` entirely, avoiding the conflict.

**Strategy — P0 (critical):** Skip printable character insertion in `on_key_down` when IME is actively composing. The `replace_and_mark_text_in_range` handler's fallback naturally becomes an insert-at-cursor, remaining correct.

**Strategy — P1 (important):** Expand the CJK range check in the "unmarked composition" heuristic to include fullwidth punctuation (U+FF00-U+FFEF), so Chinese punctuation marks like `，` correctly replace their ASCII counterparts inserted by keydown.

**Tech Stack:** Rust, GPUI 0.2

---

## File Structure

| File | Action | Responsibility |
|------|--------|---------------|
| `src/editor/mod.rs` | Modify | Skip text insertion in `on_key_down` during IME composition |
| `src/editor/ime.rs` | Modify | Expand `is_ime_output` range to cover fullwidth punctuation |

---

## Architecture Note

```
IME active, user types 'd' (before fix):
  WM_KEYDOWN (VK_PROCESSKEY) → GPUI ImmGetVirtualKey('D') → KeyDownEvent("d")
    → on_key_down → insert_text("d") ← BAD: inserts ASCII during composition
  WM_IME_COMPOSITION (GCS_COMPSTR "d") → replace_and_mark_text_in_range
    → must guess range (cursor-1..cursor) to replace

IME active, user types 'd' (after fix):
  WM_KEYDOWN (VK_PROCESSKEY) → GPUI ImmGetVirtualKey('D') → KeyDownEvent("d")
    → on_key_down → ime_marked_range.is_some() → SKIP ← FIXED
  WM_IME_COMPOSITION (GCS_COMPSTR "d") → replace_and_mark_text_in_range
    → first-char branch: (cursor, cursor) → insert at cursor ✓
```

---

### Task 1: Skip keydown insertion during IME composition

**Files:**
- Modify: `src/editor/mod.rs`

- [ ] **Step 1: Add early return in `on_key_down` when IME is composing**

In `src/editor/mod.rs`, locate the `_ =>` branch that handles printable characters (around line 2986). Inside the `if let Some(key_char) = &keystroke.key_char` block, add a check for active composition before inserting text:

```rust
_ => {
    if let Some(key_char) = &keystroke.key_char {
        // ── IME composition guard ──────────────────────────
        // During IME composition, WM_KEYDOWN (VK_PROCESSKEY)
        // produces key_char via ImmGetVirtualKey, but the IME
        // will send WM_IME_COMPOSITION separately. Inserting
        // text here creates a duplicate that the IME handler
        // must guess-and-replace, which fails under timing shifts.
        if self.ime_marked_range.is_some() {
            return;
        }
        // ───────────────────────────────────────────────────

        if key_char == " " {
            if !self.state.try_insert_space() {
                return;
            }
        } else {
            self.insert_text(key_char);
        }
        // ... rest unchanged
    }
}
```

**Why this is safe:** The `replace_and_mark_text_in_range` handler's first-character branch (ime.rs:178-182) computes `(cursor.saturating_sub(new_len), cursor)`. When no keydown text was inserted, `cursor` is still at the pre-insertion position and `new_len` is >= 1, producing `(cursor, cursor)` which is an empty range — `Buffer::replace(empty_range, text, cursor)` follows the `if range.is_empty()` branch and **inserts at cursor**.

- [ ] **Step 2: Build to verify**

```bash
cargo check 2>&1
```

Expected: compiles cleanly.

---

### Task 2: Expand fullwidth punctuation handling

**Files:**
- Modify: `src/editor/ime.rs`

- [ ] **Step 1: Add fullwidth range to `is_ime_output` check**

In `src/editor/ime.rs` line 112-118, extend the range match to include fullwidth forms:

```rust
let is_ime_output = text.chars().any(|c| matches!(c as u32,
    0x3040..=0x309F | // Hiragana
    0x30A0..=0x30FF | // Katakana
    0x3400..=0x4DBF | // CJK Extension A
    0x4E00..=0x9FFF | // CJK Unified Ideographs
    0xAC00..=0xD7AF | // Hangul Syllables
    0xFF00..=0xFFEF   // Fullwidth ASCII variants & punctuation
));
```

**Why:** Without this, Chinese punctuation like `，` (U+FF0C) enters the `else` branch (line 134-136) and does a direct `insert()` instead of replacing the ASCII `,` that `on_key_down` already inserted. The result is `，,` (both characters in the buffer).

- [ ] **Step 2: Build to verify**

```bash
cargo check 2>&1
```

Expected: compiles cleanly.

---

### Verification

- [ ] **Build:** `cargo build` succeeds
- [ ] **Functional tests (manual):**

  1. **Pinyin composition:** Activate Chinese IME (e.g., Microsoft Pinyin), type "nihao" + Space → should produce "你好" with no "n" or "nihao" leaking before the candidate
  2. **Incremental composition:** Type "d" → "a" (pinyin "da") → composition underline visible → Space to confirm → "大" appears, no residual "da"
  3. **Cancel composition:** Type "nihao" → Esc → composition cancelled, no "nihao" residue in buffer
  4. **Chinese punctuation:** With IME active, type `,` → should produce `，` only, not `，,`
  5. **English mode:** Toggle IME off, type `hello, world` → should produce `hello, world` (no regression)
  6. **Markdown after IME:** Type some Chinese, then type `**bold**` → formatting works, no stray characters
  7. **Backspace after IME:** Type Chinese, backspace → full character deleted (not partial UTF-8 byte)

- [ ] **Cross-IME test:** Test with Microsoft Pinyin, QQ Pinyin, Rime (if available), checking that composition + cancel + punctuation all work correctly
