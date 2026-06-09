# GPUI + writ 中文 IME 实现经验

## 一、架构总览

```
┌─────────────────────────────────────────────┐
│  Windows IME (Imm32)                        │
│  WM_IME_STARTCOMPOSITION / _COMPOSITION     │
│  WM_CHAR / WM_KEYDOWN (VK_PROCESSKEY)       │
└──────────────────┬──────────────────────────┘
                   ▼
┌─────────────────────────────────────────────┐
│  GPUI Windows Platform (events.rs)          │
│  handle_ime_composition / handle_char_msg   │
│  → PlatformInputHandler                     │
└──────────────────┬──────────────────────────┘
                   ▼
┌─────────────────────────────────────────────┐
│  ElementInputHandler<Editor>                │
│  → EntityInputHandler trait methods         │
│  → UTF-16 ↔ byte offset 转换                │
└──────────────────┬──────────────────────────┘
                   ▼
┌─────────────────────────────────────────────┐
│  writ Buffer (ropey)                        │
│  Buffer::insert / replace / delete          │
└─────────────────────────────────────────────┘
```

## 二、IME 注册时机

**关键发现：`window.handle_input()` 只能在 paint 阶段调用。**

GPUI 在 `window.rs:3406` 有 `debug_assert_paint()` 检查。`Render::render` 调用发生在 request_layout 阶段，不是 paint。如果在 render 中调用会 panic：
```
this method can only be called during paint
```

**解决方案：自定义 EditorImeElement**

```rust
pub struct EditorImeElement { entity: Entity<Editor> }

impl Element for EditorImeElement {
    fn paint(&mut self, ..., w: &mut Window, cx: &mut App) {
        let entity = self.entity.clone();
        let fh = entity.read(cx).focus_handle.clone();
        w.handle_input(&fh, ElementInputHandler::new(bounds, entity), cx);
        child.paint(w, cx);
    }
}
```

必须在 paint 方法中调用，因为这个方法运行时 GPUI 已经进入了 paint 阶段。

## 三、WM_CHAR 与 KeyDown 双路插入问题

**最核心的坑：输入英文/标点时会重复插入（输入 d 得到 dd）。**

### 根因分析

Windows 键盘输入有两条并行的消息路径：

| 路径 | 消息 | 处理器 | 行为 |
|------|------|--------|------|
| 路径 A | `WM_KEYDOWN` | writ 的 `on_key_down` → `insert_text(key_char)` | 插入字符 |
| 路径 B | `WM_CHAR` | GPUI → `handle_char_msg` → `replace_text_in_range(None, char)` | **再次插入** |

两条路径都会把同一个字符插入缓冲区，导致重复。

### 解决方案

writ 的 `on_key_down` 已经处理了所有可打印字符。IME handler 中收到的 `replace_text_in_range(None, text)` 是 WM_CHAR 的重复消息，**直接丢弃**：

```rust
fn replace_text_in_range(&mut self, replacement, text, ...) {
    // 无 IME 组合 + 无 explicit range → writ 已通过 KeyDown 插入了
    // WM_CHAR 是重复消息，跳过
    if replacement.is_none() {
        return;
    }
    // 仅处理 explicit replacement range（罕见）
    ...
}
```

只在 IME 组合活跃时（`marked_range.is_some()`）才处理：
- 丢弃单个 ASCII 字符（WM_CHAR 残留）
- 接受多字节中文文本（IME 确认）

## 四、IME 组合与 writ KeyDown 的冲突

**更深层的坑：writ 的 key handler 在两次 IME 事件之间插入拼音字母。**

### 时序

```
用户按 'd'（IME 启动）:
  1. WM_KEYDOWN (VK_PROCESSKEY) → writ on_key_down → insert_text("d") → 缓冲区 "d"
  2. WM_IME_COMPOSITION (GCS_COMPSTR "d") → replace_and_mark_text_in_range(None, "d")
     → marked_range = Some(0..1)
     
用户按 'a'（继续拼音）:
  3. WM_KEYDOWN (VK_PROCESSKEY) → writ on_key_down → insert_text("a") → 缓冲区 "da"
  4. WM_IME_COMPOSITION (GCS_COMPSTR "da") → replace_and_mark_text_in_range(None, "da")
     → 旧 marked_range = Some(0..1)，只覆盖 "d"
     → 但 writ 在步骤 3 已插入 "a" 在位置 1
     → 如果只替换 0..1 → 结果 "daa" ✗
```

### 解决方案

更新组合时，**替换范围扩大到 cursor**（而不是仅 `marked_range.end`）：

```rust
} else if let Some(mark) = self.ime_marked_range.clone() {
    // writ 可能在两次 IME 事件之间插入了文字
    // 扩大到当前 cursor 位置
    (mark.start, cursor.max(mark.end))
}
```

首键组合也做类似处理：
```rust
} else {
    // 第一个组合字符 — writ 已通过 key handler 插入
    // 检查光标前文本是否匹配，如果是则替换之
    let before = cursor.saturating_sub(new_len);
    (before, cursor)
}
```

## 五、UTF-16 与字节偏移转换

**IME API 全部使用 UTF-16 偏移，而 writ 内部使用字节偏移（UTF-8）。**

```rust
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
```

**注意：必须 clamp 偏移量！** 在 IME 事件处理的间隙，光标可能暂时越界（buffer 被 writ 修改了但 cursor 还没更新），直接切片会 panic。

## 六、Buffer 越界崩溃（连续编辑时）

**ropey 的 `byte_to_char` 对越界字节偏移直接 panic。**

### 现象
```
byte index out of bounds: byte index 24, Rope/RopeSlice byte length 22
```

### 根因

writ 的 Buffer 内部 `byte_to_char` 调用遍布 15+ 处，从 `apply_edit`、`slice`、`byte_to_line` 到 `code_highlights_for_range`、`normalize_ordered_lists`。在连续退格或快速输入时，树解析、列表重排等操作可能使用过期的树节点位置，导致字节偏移超出 rope 当前长度。

### 解决方案

1. **入口防御**：`Buffer::delete` / `replace` 中 clamp range 到 rope 长度
2. **中间防御**：`BufferContent::slice` / `apply_edit` 中 clamp
3. **全局防御**：新增 `byte_to_char_safe` 方法，替换所有 `self.text.byte_to_char` 调用

```rust
fn byte_to_char_safe(&self, byte_offset: usize) -> usize {
    let len = self.text.len_bytes();
    if byte_offset >= len {
        return self.text.len_chars();
    }
    self.text.byte_to_char(byte_offset)
}
```

## 七、关键文件清单

| 文件 | 修改内容 |
|------|----------|
| `src/editor/ime.rs` | EntityInputHandler impl, EditorImeElement, UTF-16 转换, WM_CHAR 过滤 |
| `src/editor/mod.rs` | 添加 `ime_marked_range` 字段 |
| `src/buffer.rs` | `byte_to_char_safe`, clamp 防御, delete/replace/slice 边界检查 |
| `src/main.rs` | 简化入口，设置 Global state |

## 八、经验总结

1. **paint 阶段注册 IME**：GPUI 强制要求，忘记这一点就是 panic
2. **WM_CHAR 是重复消息**：任何有 `on_key_down` 处理字符输入的框架，WM_CHAR 都会造成双路插入
3. **IME 和 KeyDown 互相不知晓**：组合期间 key handler 继续插入拼音，必须扩大替换范围
4. **UTF-16 转换必须 clamp**：cursor 和 buffer 在不同步时偏移量会越界
5. **ropey 不宽容**：所有字节操作必须有边界检查，尤其是涉及 tree-sitter 节点位置的
6. **writ 的 Global state 必须全部设置**：`Config`, `FileInfo`, `StatusBarInfo`, `CursorScreenPosition`
7. **空格不是 key_char**：GPUI 中空格键的 `keystroke.key` 为 `"space"`，`key_char` 可能为 None，必须在 `on_key_down` 中单独匹配 `"space"` 分支处理
8. **非 ASCII 字符不走 KeyDown**：中文标点等全角字符不会通过 `WM_KEYDOWN` + `key_char` 到达，而是走 `WM_CHAR`/`WM_IME_CHAR`，IME handler 中不能一刀切丢弃所有 `replacement.is_none()` 的消息
9. **IME 状态清理必须 notify**：`unmark_text` 后若不调用 `cx.notify()`，GPUI 内部 IME 状态与 Editor 不同步，会导致后续 backspace/方向键等输入短暂失效
10. **backspace 不能按字节删**：多字节字符（如中文 3 字节）的 `delete_backward` 必须用 `prev_char_boundary` 找完整字符边界，`cursor_pos - 1` 只删 1 字节 → UTF-8 断裂 → ropey panic/光标消失
11. **byte_at 不能假设 char 对齐**：ropey 的 `byte_slice` 要求两端对齐 char 边界。`byte_at` 中 `byte_slice(offset..offset+1)` 在 offset 落入多字节字符中间时 panic。正确做法：char-based 访问（`byte_to_char_safe → char_to_byte → char → encode_utf8 → 按相对偏移取字节`）
12. **IME 中文标点双路重叠**：中文模式下按标点键时，`on_key_down` 先插入 ASCII 版（如 `,`），`replace_text_in_range` 后收到全角版（如 `，`），需在 IME handler 中检测光标前的 ASCII 标点并替换为 IME 版本
13. 

## 九、待解决问题

1. 上轮未能修复的问题：markdown语法输入过程中，实时渲染-编辑状态反复转换，导致输入区域闪烁，应固定在编辑状态，等光标移走再实时渲染。

原分析过程：
**光标 blink 不能触发重量级渲染**：render 中的 `detect_links` → `spawn_github_validation` 链路应受 `content_changed` 守卫。否则 blink 定时器的 500ms `cx.notify()` 反复执行解析/验证 async spawn，造成文本框闪烁

当时执行的改进：
| 问题 | 修复文件 | 修复摘要 |
|------|----------|----------|
| 光标闪烁导致文本框 flickering | `src/editor/mod.rs` | render 中 `detect_links` 等重量级操作受 `content_changed` 守卫，blink 周期不再重复执行 |

补充信息：实时渲染功能为开源项目writ特性，代码位于：C:\Users\Benai\writ-main，必要时应对照检查是否在修改中破坏了原本的渲染功能。

## 十、改进记录

### 2026-06-09（第一批）

| 问题 | 修复文件 | 修复摘要 |
|------|----------|----------|
| 空格无法输入 | `src/editor/mod.rs` | 在 `on_key_down` 中增加 `"space"` 分支，调用 `try_insert_space()` |
| IME 结束后 backspace 短暂失效 | `src/editor/ime.rs` | `unmark_text` 末尾增加 `cx.notify()`，强制状态同步 |
| 中文标点无法输入 | `src/editor/ime.rs` | `replace_text_in_range` 中收窄丢弃逻辑：仅丢弃单个 ASCII 字符，非 ASCII 文本直接插入 |

### 2026-06-09（第二批）

| 问题 | 修复文件 | 修复摘要 |
|------|----------|----------|
| 中文模式下标点双打 `,，` | `src/editor/ime.rs` | 插入非 ASCII 前检测光标前一字节，若为 ASCII 标点则先删除（替换为 IME 版本） |
| 中文退格光标消失/闪退 | `src/buffer.rs`, `src/editor/mod.rs` | 新增 `prev_char_boundary`/`next_char_boundary`；`delete_backward` 中用其替代 `cursor_pos - 1` |
| 光标从不闪烁 | `src/editor/mod.rs` | 新增 `cursor_blink_visible` 字段，启动 500ms 定时器切换可见性，输入时 `reset_cursor_blink()` |
| 输入 `~~~~` 闪退（byte_at panic） | `src/buffer.rs` | `byte_at` 改用 char-based 安全访问，支持任意 byte offset（含多字节字符中间位置） |

