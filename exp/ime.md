# IME 实现经验

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

**最终架构：文本插入只有一条通道。** `on_key_down` 不插入任何可打印字符，所有普通的文本插入走 WM_CHAR → `replace_text_in_range`，IME 组合走 WM_IME_COMPOSITION → `replace_and_mark_text_in_range`。消除了 Windows 双消息路径（WM_KEYDOWN + WM_CHAR）导致的重复插入和 IME 冲突。

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

## 三、最终方案：on_key_down 不插入文本，统一 replace_text_in_range

### 根因

Windows 键盘输入有两条并行的消息路径：

| 路径 | 消息 | 处理器 | 行为 |
|------|------|--------|------|
| 路径 A | `WM_KEYDOWN` | `on_key_down` → `insert_text(key_char)` | 插入字符 |
| 路径 B | `WM_CHAR` | GPUI → `handle_char_msg` → `replace_text_in_range(None, char)` | **再次插入** |

两条路径都会把同一个字符插入缓冲区导致重复。更严重的是，IME 组合期间 VK_PROCESSKEY 通过 GPUI 0.2.2 的 `ImmGetVirtualKey` 回退仍产生 key_char，插入拼音字母到 buffer，随后 WM_IME_COMPOSITION 事件需要猜测范围去替换，时序一乱就产生残留。

### 最终方案

**`on_key_down` 不插入任何可打印字符。** 所有文本插入由 `replace_text_in_range` 统一处理（来自 WM_CHAR），IME 组合由 `replace_and_mark_text_in_range` 处理（来自 WM_IME_COMPOSITION）。

```rust
// on_key_down 中：
_ => {
    if let Some(key_char) = &keystroke.key_char {
        if key_char == " " {
            if !self.state.try_insert_space() {
                return;
            }
        }
        // 普通字符不插入 — 由 WM_CHAR → replace_text_in_range 处理
        // markdown 触发器和滚动光标保留
        if key_char == ">" { self.state.maybe_complete_blockquote_marker(); }
        if key_char == "`" || key_char == "~" { self.state.maybe_complete_code_fence(); }
        self.scroll_to_cursor_pending = true;
    }
}
```

```rust
// replace_text_in_range (无 composition 分支)：
if replacement.is_none() {
    if text.is_empty() { return; }
    let cursor = self.state.cursor().offset;

    // 空格由 on_key_down 的 try_insert_space 处理
    if text == " " { return; }

    // 单 ASCII 字符 — 直接插入
    if text.len() == 1 && text.as_bytes()[0].is_ascii() {
        let new_end = self.state.buffer.insert(cursor, text, cursor);
        ...
        return;
    }

    // 非 ASCII / 多字符 — 含未标记 composition 启发式
    // (手心输入法等 IME 不维护 marked_range)
    let is_ime_output = text.chars().any(|c| matches!(c as u32,
        0x3040..=0x309F | 0x30A0..=0x30FF |
        0x3400..=0x4DBF | 0x4E00..=0x9FFF |
        0xAC00..=0xD7AF | 0xFF00..=0xFFEF
    ));
    ...
}
```

### 解决了的问题

1. **拼音泄露**（"n你好" / "nihao 你好"）：on_key_down 不插入拼音字母，IME 组合完全通过 replace_and_mark_text_in_range 处理
2. **中文标点双打**（"，" → "，,"）：on_key_down 不插入 ASCII 标点，WM_CHAR 直接收到全角字符并插入
3. **组合首字符残留**：不再需要猜测范围替换，直接插入光标位置

### 关键细节：首字符插入

第一代方案中 `replace_and_mark_text_in_range` 的首字符分支用 `cursor.saturating_sub(new_len)` 计算替换范围，假设 on_key_down 已插入文本。最终方案改为直接插入：

```rust
} else {
    // 首个组合字符 — on_key_down 不插入文本，直接插入光标处
    (cursor, cursor)
};
```

`Buffer::replace(cursor..cursor, text, cursor)` === insert at cursor，因为 range 是空区间。

## 四、IME 组合期间后续字符的替换范围

当组合已存在（`ime_marked_range = Some(mark)`），后续 WM_IME_COMPOSITION 更新时，替换范围需要覆盖到当前光标：

```rust
(mark.start, cursor.max(mark.end))
```

这覆盖了 mark 开始到光标之间的所有内容，正确处理了增量拼音输入（如 "d" → "da" → "dan"）。

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

| 文件 | 职责 |
|------|------|
| `src/editor/ime.rs` | EntityInputHandler impl（`replace_text_in_range`、`replace_and_mark_text_in_range`、`unmark_text` 等）、EditorImeElement、UTF-16 转换 |
| `src/editor/mod.rs` | `ime_marked_range` 字段、`on_key_down`（不插入可打印字符） |
| `src/buffer.rs` | `byte_to_char_safe`、clamp 防御、delete/replace/slice 边界检查 |

## 八、经验总结

### 核心架构

1. **不要从 on_key_down 插入可打印字符。** Windows 的双消息路径（WM_KEYDOWN → key_char + WM_CHAR → replace_text_in_range）是所有 IME 问题的根源。`on_key_down` 只管导航键、快捷键、空格（因为空格涉及列表缩进等复杂逻辑）；普通字符插入统一由 `replace_text_in_range` 处理。

2. **`replace_text_in_range` 是唯一文本插入入口。** 无论英文、中文标点还是 IME 确认文本，都通过此方法进入 buffer。单 ASCII 字符 fast path 直接插入，非 ASCII/多字符通过未标记 composition 启发式处理。

3. **`replace_and_mark_text_in_range` 的首字符必须是纯插入。** 不能用 `cursor.saturating_sub(new_len)` 做范围替换——那个逻辑只在上一条（on_key_down 已插入）时成立。改为 `(cursor, cursor)` 空区间插入。

4. **后续组合字符的替换范围扩大到光标。** `(mark.start, cursor.max(mark.end))` 确保 on_key_down 没有插字符的情况下也覆盖正确。

### 关键陷阱

5. **IME 注册必须在 paint 阶段。** GPUI 强制 `debug_assert_paint()`，必须在 `EditorImeElement::paint()` 中调用 `w.handle_input()`。

6. **空格既特殊又麻烦。** 空格需要 `try_insert_space()` 做列表缩进续行，但 WM_CHAR 也会发送空格。方案：on_key_down 处理空格，replace_text_in_range 跳过空格（`if text == " " { return; }`）。

7. **UTF-16 转换必须 clamp。** cursor 和 buffer 在不同步时偏移量会越界。

8. **ropey 不宽容。** 所有字节操作必须有边界检查，尤其是涉及 tree-sitter 节点位置的。

9. **IME 取消时必须清理 buffer。** `replace_text_in_range`/`replace_and_mark_text_in_range` 收到空字符串 = 取消，必须 `buffer.delete(mark)` 删除 composition 文本，否则拼音残留。

10. **`unmark_text` 删除 composition 需加 ASCII 守卫。** 确保 marked_text 仍为 ASCII 字母才执行删除，误删已确认的中文。

### IME 兼容性

11. **不同 IME 使用不同事件路径。** 微软拼音用 `replace_and_mark_text_in_range` 维护 mark；手心输入法可能用 `replace_text_in_range` 发送组合更新（或不发后续更新），`ime_marked_range` 始终为 None。`replace_text_in_range` 的未标记 composition 启发式（扫描光标前 ASCII 字母替换为 CJK 文本）是为此设计的。

12. **`is_ime_output` 范围必须包含全角符号。** 不仅 CJK 表意文字（U+4E00-U+9FFF）、假名（U+3040-U+30FF）、谚文（U+AC00-U+D7AF），还必须有全角 ASCII 与标点（U+FF00-U+FFEF），否则中文标点 `，`（U+FF0C）不触发替换启发式，直接 insert 导致双打。

13. **不同 IME 使用不同取消路径。** 有的发空字符串 `replace_text_in_range("")`，有的发 `replace_and_mark_text_in_range("")`，有的直接 `unmark_text()`。三个路径都得处理。

### 调试提示

14. **IME 事件顺序追踪。** 用 `OutputDebugString` 或日志分别在 on_key_down、replace_text_in_range、replace_and_mark_text_in_range、unmark_text 打点，观察 VK_PROCESSKEY / WM_CHAR / WM_IME_COMPOSITION 的到达顺序和参数。

15. **GPUI 跨版本问题。** GPUI 0.2.2 的 Windows 平台对 VK_PROCESSKEY 做了 `ImmGetVirtualKey` 回退（`events.rs:1345`），导致 IME 组合期间的 WM_KEYDOWN 仍产生 key_char。Zed 的新版 GPUI 丢弃了这些事件。这差异解释了为什么在 Zed 中不存在此问题，而在 GPUI 0.2.2 上需要做以上所有补偿。

## 九、修复批次纪要

### 2026-06-09（第一批：基础 IME 支持）
- 空格无法输入 → `on_key_down` 增加 `"space"` 分支
- IME 后 backspace 失效 → `unmark_text` 末尾增加 `cx.notify()`
- 中文标点无法输入 → 收窄 WM_CHAR 丢弃逻辑，仅丢弃单个 ASCII

### 2026-06-09（第二批：中文兼容）
- 中文标点双打 `,，` → 插入非 ASCII 前检测并删除光标前 ASCII 标点
- 中文退格崩溃 → `prev_char_boundary`/`next_char_boundary`，`byte_at` 安全访问
- `~~~~` 闪退 → `byte_at` char-based 安全访问

### 2026-06-11（第十四批：IME 残留）
- Esc 取消后拼音残留 → 空字符串时 `buffer.delete(mark)`
- 首字符 precondition 致重入 → 回退原始替换逻辑
- unmark_text 误删中文 → ASCII 守卫

### 2026-06-13（修复 9：标点误删英文）
- "hello，" 误删 hello → `is_ime_output` 范围限制

### 2026-06-14（最终方案：统一文本通道）
- **架构变更**：`on_key_down` 不再插入可打印字符，统一走 `replace_text_in_range`
- **修复**：首字符插入改为 `(cursor, cursor)` 空区间，消除 destructive range
- **修复**：`is_ime_output` 扩展至全角符号区 0xFF00-0xFFEF
- **简化**：删除 replace_text_in_range 中的 ASCII 跳过逻辑和 ASCII 标点清理启发式

### 2026-06-16（修复 10：IME 候选框跟随光标）
- **修复**：`bounds_for_range` 改为读取 `cursor_screen_pos` 返回光标正下方位置，而非固定偏移

### 2026-06-18（0.4.5：全角标点误吞 & 单字符提交卡死）

两个 bug 的根因都在 `replace_text_in_range` 的 **组合模式分支**。

**Bug 1：全角标点误吞英文字符**
- **现象**：中文 IME 输入英文上屏后，输入 `，；？：` 等全角标点导致上屏英文消失；`。、""` 安全
- **根因**：`is_ime_output` 范围包含 `0xFF00..=0xFFEF`（全角符号），纯全角标点无 CJK 字符时仍触发 pinyin 启发式回扫，替换光标前英文
- **确认方法**：对照 Unicode 范围与用户测试结果完全吻合（触发字符全在 0xFF00-0xFFEF 内，安全字符全不在）
- **修复**：从 `is_ime_output` 范围中删除 `0xFF00..=0xFFEF`（`ime.rs:113-119`）
- **原理**：Shouxin 类 IME 的未标记 composition 输出必然携带具体 CJK 字符字形（如 `你好，` 含 `0x4F60 0x597D`），纯全角标点不需要 pinyin cleanup

**Bug 2：单 ASCII 字符 IME 提交卡死编辑**
- **现象**：中文 IME 输入单个英文字符上屏后，无法删除/移动光标/输入任何内容；切换英文模式仍无法输入
- **根因**：`replace_text_in_range` 组合分支有 ASCII 守卫（`ime.rs:56-59`），IME 提交单 ASCII 字符时被静默丢弃；`ime_marked_range` 未清除，编辑器认为仍在组合；后续所有 `replace_text_in_range` 调用都被守卫拦截或错误替换
- **修复**：删除 ASCII 守卫块（`ime.rs:56-59`）
- **原理**：守卫原为防止 WM_KEYDOWN+WM_CHAR 双路径重复插入，但 `on_key_down` 已不插入可打印字符（0.4.4 最终方案），守卫已无必要且阻塞 IME 提交流程

### 2026-06-20（0.4.8：Rime 输入法全适配）

Rime（小狼毫/weasel 0.17.4）在 Windows 上使用 TSF 框架，与 GPUI 0.2.2 的 IMM32 IME 处理存在根本性事件时序差异，导致四个独立 bug。以下逐一描述根因与修复。

**核心发现：Rime 的 WM_IME_COMPOSITION 比 WM_KEYDOWN 先到达**

Rime 在 `ImeProcessKey` 中同步提交文本（`_EndComposition` → `WM_IME_COMPOSITION`），然后返回 `TRUE` 或 `FALSE`。无论返回值如何，`WM_IME_COMPOSITION` 消息已经进入了队列。当 GPUI 处理消息时，IME composition 事件先于键事件到达。这要求 editor 的 IME 状态管理必须在**确认路径中保持 `ime_composing` 标志**，以便随后的键事件被 GPUI 的 `marked_text_range()` 检查拦截。

#### Bug 1：空格上屏首选后多插入空格

- **现象**：Rime 用空格选首选上屏后，上屏字符后面多一个空格
- **根因**：`replace_text_in_range` 的 IME 确认分支在替换完 composition 文本后立即清除了 `ime_composing` 标志。随后的空格键事件到达时，`marked_text_range()` 返回 `None`，GPUI 将空格派发到 `on_key_down`，导致 `try_insert_space()` 插入空格
- **修复**：确认分支不清除 `ime_composing`，同时刷新 `last_ime_activity` 时间戳。`marked_text_range()` 返回合成零宽范围，GPUI 将键事件路由到 `translate_message`（IME 处理）而非 `on_key_down`。`ime_composing` 由 500ms 超时自动清除

#### Bug 2：Enter 上屏英文后换行

- **现象**：Rime 英文模式按 Enter 上屏后，英文出现在新一行
- **根因**：与 Bug 1 完全相同的机制——Enter 键事件在 IME 确认后到达，被派发到 `on_key_down` 执行 `self.enter()` 插入换行
- **修复**：同 Bug 1

#### Bug 3：候选窗内删除键导致编辑区误删 / 光标锁死

- **现象**：在 Rime 候选窗激活时按 Backspace/Delete 删除拼音，最后一个拼音删除后光标前编辑区字符也被删除；删完所有候选窗字符后光标锁死无法输入
- **根因**：(a) 最后一个退格清空 composition 字符串时，`replace_and_mark_text_in_range` 收到空字符串并清除了 `ime_composing`，导致随后的退格键事件穿透到 `on_key_down`；(b) 超时时间 5 秒过长，用户删完候选窗字符后长时间无法恢复
- **修复**：(a) 空字符串路径不清除 `ime_composing`，保持键事件抑制；(b) 超时从 5 秒缩短到 500ms

#### Bug 4：中文标点触发回删

- **现象**：Enter 上屏英文后，输入 `，；：？"` 等全角标点导致之前上屏的英文字符被删除
- **根因**：`replace_text_in_range` 的向后扫描 ASCII 拼音字母逻辑**没有 `is_ime_output` 保护**。全角标点（U+FF0C 等）不在 CJK 范围（`is_ime_output=false`），但扫描仍执行，找到光标前刚上屏的英文字母，将其当作"拼音"替换为标点
- **修复**：将整个向后扫描 + 替换逻辑收进 `if is_ime_output` 块内。非 CJK 文本走 `PLAIN_INSERT`，不碰已有内容

#### 关键架构改进

| 机制 | 说明 |
|------|------|
| `ime_composing: Cell<bool>` | 独立于 `ime_marked_range` 的 IME 组合状态标志。为不维护 marked_range 的 IME（Rime/手心）提供组合检测，通过 `marked_text_range()` 返回合成零宽范围来抑制键事件 |
| `last_ime_activity: Cell<Option<Instant>>` | 500ms 无活动自动清除 `ime_composing`，防止光标锁死 |
| `last_commit_key: Cell<Option<u8>>` | 追踪 `on_key_down` 中的空格/回车键，在 `replace_text_in_range` 收到提交文本时反向回吞（处理键先到、文本后到的时序）|
| `ime_just_committed_text: Cell<bool>` | CJK 文本提交后设置，`on_key_down` 检查并抑制随后的空格/回车（处理文本先到、键后到的时序）|
| 向后扫描 `is_ime_output` 守卫 | 拼音字母扫描仅在 `is_ime_output=true` 时执行，防止非 CJK 标点误触发 |

**关键教训**：IME 提交路径（`replace_text_in_range` 确认分支）是状态转换的关键节点。清除 `ime_composing` 必须在键事件**之后**——通过超时或显式 `unmark_text`——而非在确认分支中同步清除。
