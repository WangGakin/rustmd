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
13. **IME 取消时必须同时清理状态和 buffer**：`replace_text_in_range`/`replace_and_mark_text_in_range` 收到空字符串时，不能只清除 `ime_marked_range` 和调用 `cx.notify()`，还必须 `buffer.delete(mark)` 删除 composition 文本。否则拼音原样留在编辑区，表现为"键盘锁死"（GPUI 认为组合已结束，但用户看到残留拼音以为锁住了）或"Esc 取消后拼音残留"
14. **`replace_text_in_range` 与 `replace_and_mark_text_in_range` 是两条独立的事件路径**：微软拼音使用后者维护 composition 标记，手心输入法可能使用前者（或根本不维护 marked_range）。不能假设所有 IME 都走同一条路径。当 `replace_text_in_range` 在"无 composition"状态收到中文时，应启发式检查光标前的 ASCII 字母（如未标记的拼音）并替换之，而非直接 insert
15. **首字母残留的时序成因**：IME 组合的第一个字符会通过 `on_key_down` 插入 buffer，然后 `replace_and_mark_text_in_range` 再替换它。如果替换时 `cursor.saturating_sub(new_len)` 范围内实际文本与预期不匹配（如 Shouxin 带空格的 composition 字符串 → 启发了 precondition 检查 → fallback insert），会导致重复插入。**最佳做法是保持原始替换逻辑，不加入前置条件校验**——校验本身引入了时序依赖的 bug
16. **`unmark_text` 删除 composition 文本必须加 ASCII 守卫**：`unmark_text` 可能在确认和取消两个场景被调用。确认场景下 marked_range 已被 `replace_text_in_range` 的 `take()` 清空，`unmark_text` 的 `if let Some(mark)` 分支不会执行。但如果因事件顺序异常导致 mark 仍然存在，删除 text 会误删中文。必须检查 marked_text 是否仍为 ASCII 字母（拼音）才执行删除

## 九、待解决问题

已在第十轮完成改进。

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

### 2026-06-10（第三批 — markdown 实时渲染闪烁修复）

| 问题 | 修复文件 | 修复摘要 |
|------|----------|----------|
| markdown 语法输入时光标闪烁导致编辑/渲染状态反复切换 | `src/line.rs`, `src/editor/mod.rs` | 将 `cursor_offset`（编辑模式检测）与 `show_cursor`（光标视觉渲染）解耦；新增 `show_cursor: bool` 字段到 `Line` 结构体；`compute_visual_cursor_pos` 改为检查 `show_cursor` 而非 `usize::MAX`；主编辑器 render 中 `editing_cursor_offset` 仅在失焦时才为 `usize::MAX`，blink 关闭时仍保持真实光标位置 |

### 2026-06-10（第四批 — 文件读写、新建与菜单）

新增文件操作功能：打开、保存、另存为、新建文件，以及原生 File 菜单。

| 功能 | 快捷键 | 说明 |
|------|--------|------|
| 打开文件 | Ctrl+O | 弹 rfd 原生对话框，dirty 时先弹确认框 |
| 保存 | Ctrl+S | 有路径直接写盘，无路径自动弹 Save As |
| 另存为 | Ctrl+Shift+S | 弹 rfd Save 对话框，切换当前文件路径 |
| 新建文件 | Ctrl+Alt+N | 清空编辑器，dirty 时先弹确认框 |

**涉及文件：**

| 文件 | 改动 |
|------|------|
| `Cargo.toml` | 添加 `rfd = "0.15"` 依赖 |
| `src/file_ops.rs` | **新建** — `actions!(file, [NewFile, OpenFile, Save, SaveAs])` + 对话框封装 + 启动逻辑辅助函数 |
| `src/title_bar.rs` | `FileInfo.path` 改为 `Option<PathBuf>`，无路径时显示 "untitled" |
| `src/editor/mod.rs` | 新增 `save_as()`/`open_file()`/`new_file()` 方法；`save()` 改为从 `self.file_path` 读取；render 中注册 4 个 action handler；移除 on_key_down 中的 Ctrl+S |
| `src/lib.rs` | 导出 `file_ops` 模块 |
| `src/main.rs` | `Config::parse()` 解析命令行参数；`cx.bind_keys()` 注册快捷键；`cx.set_menus()` 注册 File 菜单；按 `--file`/`--demo`/空白三种模式启动 |

**启动行为：**
- `--file <path>` → 打开指定文件
- `--demo` → 演示内容
- 无参数 → 空白新文件

**文件对话框过滤：** `.md` 和 `.txt` 为主，可切换显示所有文件。

**未保存变更确认：** 使用 `rfd::MessageDialog`（Yes/No/Cancel），Save / Don't Save / Cancel 三选一。

### 2026-06-10（第五批 — 自绘菜单与客户端装饰）

GPUI 在 Windows 上不支持原生菜单栏（`cx.set_menus()` 仅存储数据，不创建 HMENU）。改为自绘菜单 + 客户端装饰。

**架构变更：**

| 组件 | 说明 |
|------|------|
| `WindowDecorations::Client` | 窗口选项启用客户端装饰，移除系统标题栏 |
| `window_shadow()` | 使用 GPUI 自绘窗口边框、标题栏、状态栏 |
| `menu.rs` | **新建** — 自绘菜单栏 + 下拉菜单 |
| `title_bar.rs` | 集成 `menu::menu_bar()` 到标题栏左侧 |

**菜单交互：**
- 点击菜单按钮展开下拉列表
- 打开状态下鼠标 hover 其他菜单按钮会切换
- 点击菜单项触发 action 并关闭菜单
- 菜单项支持分隔线

**涉及文件：**

| 文件 | 改动 |
|------|------|
| `src/menu.rs` | **新建** — `MenuBarState` (Global)、`Menu`、`MenuEntry`、`menu_bar()`、`dropdown_overlay()` |
| `src/title_bar.rs` | 集成 `menu::menu_bar()` 到标题栏 |
| `src/main.rs` | `WindowDecorations::Client`；`RootView` 使用 `window_shadow()` 包裹内容 |
| `src/lib.rs` | 导出 `menu` 模块 |

### 2026-06-10（第六批 — RefCell panic 修复）

**问题：** 通过菜单或快捷键触发文件操作时闪退，报错 `RefCell already borrowed`。

**根因：** `rfd` 文件对话框在 Windows 上创建嵌套消息循环（modal loop），此时 cursor blink 的 async task 触发，尝试 `borrow_mut(App)` —— 但 App 仍被外层 `update_window` 借用 → panic。

**修复方案：**

1. **`DIALOG_OPEN` 原子标志** (`file_ops.rs`)
   - 菜单点击时立即设置 `set_dialog_open(true)`
   - 文件操作完成后重置 `set_dialog_open(false)`
   - cursor blink 检查 `is_dialog_open()`，为 true 时跳过更新

2. **pending_file_op 机制** (`editor/mod.rs`)
   - action handler 仅设置 `pending_file_op = Some(op)`
   - `render()` 开头检查并执行 pending 操作
   - 避免在 action dispatch 期间直接打开对话框

3. **cursor blink 使用 `update_window`**
   - `cx.update_window()` 内部用 `try_borrow_mut()`，被借用时返回 `Err` 而非 panic
   - 替代原来的 `cx.update()` + `borrow_mut()`

**涉及文件：**

| 文件 | 改动 |
|------|------|
| `src/file_ops.rs` | 新增 `DIALOG_OPEN: AtomicBool`、`is_dialog_open()`、`set_dialog_open()`、`FileOp` 枚举 |
| `src/editor/mod.rs` | `pending_file_op` 字段；action handler 仅设置 pending；`render()` 执行 pending 操作；cursor blink 检查 `is_dialog_open()` 并使用 `update_window` |
| `src/menu.rs` | 菜单点击时调用 `set_dialog_open(true)` |

**经验总结：**

1. **rfd 在 Windows 上创建嵌套消息循环** — 对话框打开期间，GPUI 的 async task 仍会触发，必须避免 `borrow_mut` 冲突
2. **GPUI 的 `cx.update()` 会 panic** — 如果 App 已被借用（如在 `update_window` 内部），必须用 `try_borrow_mut` 或 `update_window` 替代
3. **GPUI Windows 不支持原生菜单** — `set_menus()` 仅存储数据，需要自绘或使用其他方案
4. **客户端装饰需要 `window_shadow()`** — 没有系统标题栏时，必须用 GPUI 自绘边框、标题栏、状态栏

### 2026-06-10（第七批 — 窗口拖动与红绿灯控制）

**问题：** 隐藏原生标题栏后，窗口无法拖动，红绿灯按钮功能异常（最大化后恢复时红绿灯消失）。

**根因分析：**

1. **窗口无法拖动** — GPUI 的 `window.start_window_move()` 在 Windows 平台未实现
2. **红绿灯消失** — 使用 `ShowWindow()` 同步调用导致重入消息处理，破坏 GPUI 状态
3. **菜单无法自动关闭** — 点击编辑区时未监听鼠标事件关闭菜单

**解决方案：**

1. **窗口拖动** — 使用 Win32 API 发送 `WM_NCLBUTTONDOWN` 消息：
   ```rust
   use raw_window_handle::RawWindowHandle;
   use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
   use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
   use windows::Win32::UI::WindowsAndMessaging::{SendMessageW, HTCAPTION, WM_NCLBUTTONDOWN};
   
   // 在标题栏的 on_mouse_down 中
   if let Ok(handle) = raw_window_handle::HasWindowHandle::window_handle(window) {
       if let RawWindowHandle::Win32(win32_handle) = handle.as_raw() {
           unsafe {
               let hwnd = HWND(win32_handle.hwnd.get() as _);
               let _ = ReleaseCapture();
               let _ = SendMessageW(hwnd, WM_NCLBUTTONDOWN, 
                                   Some(WPARAM(HTCAPTION as _)), 
                                   Some(LPARAM(0)));
           }
       }
   }
   ```

2. **红绿灯最大化/恢复切换** — 使用 `ShowWindowAsync` 替代 `ShowWindow`：
   ```rust
   use windows::Win32::UI::WindowsAndMessaging::{ShowWindowAsync, SW_RESTORE};
   
   // 在 ZoomWindow action handler 中
   if window.is_maximized() {
       if let Ok(handle) = raw_window_handle::HasWindowHandle::window_handle(window) {
           if let RawWindowHandle::Win32(win32_handle) = handle.as_raw() {
               unsafe {
                   let hwnd = HWND(win32_handle.hwnd.get() as _);
                   let _ = ShowWindowAsync(hwnd, SW_RESTORE);
               }
           }
       }
   } else {
       window.zoom_window();
   }
   ```

3. **菜单自动关闭** — 在编辑区添加 `on_mouse_down` 监听器：
   ```rust
   .on_mouse_down(MouseButton::Left, |_, window, cx| {
       let state = cx.global_mut::<MenuBarState>();
       if state.open_menu_index.is_some() {
           state.close_menu();
           window.refresh();
       }
   })
   ```

**新增依赖：**

```toml
raw-window-handle = "0.6"
windows = { version = "0.61", features = [
    "Win32_UI_WindowsAndMessaging",
    "Win32_Foundation",
    "Win32_UI_Input_KeyboardAndMouse"
] }
```

**涉及文件：**

| 文件 | 改动 |
|------|------|
| `Cargo.toml` | 添加 `raw-window-handle` 和 `windows` crate 依赖 |
| `src/title_bar.rs` | 使用 `SendMessageW` + `WM_NCLBUTTONDOWN` + `HTCAPTION` 实现拖动 |
| `src/main.rs` | 红绿灯 action handler 使用 `ShowWindowAsync` 实现最大化/恢复切换；编辑区添加菜单关闭监听 |

**经验总结：**

1. **GPUI 的 `start_window_move()` 在 Windows 未实现** — 必须使用 Win32 API 手动发送 `WM_NCLBUTTONDOWN` 消息，配合 `HTCAPTION` 参数触发拖动
2. **`ShowWindow` 会导致重入问题** — 同步调用会立即处理窗口消息，可能破坏 GPUI 的渲染状态。使用 `ShowWindowAsync` 异步执行更安全
3. **获取 HWND 需要 `raw-window-handle`** — GPUI 的 `Window` 实现了 `HasWindowHandle` trait，通过 `window_handle()` 获取 `RawWindowHandle::Win32` 变体
4. **菜单关闭需要显式监听** — 点击编辑区时不会自动关闭菜单，需要在编辑区的 `on_mouse_down` 中检查并关闭
5. **`window.refresh()` 强制重绘** — 在某些情况下（如状态变更后），需要显式调用 `refresh()` 触发重绘
6. **标题栏布局需要 `flex_1()`** — 可拖动区域必须占满标题栏剩余空间，否则拖拽区域太小

### 2026-06-10（第八批 — Async Task RefCell Panic 彻底修复）

**问题：** 读取文件 → 新建 → 再读取时崩溃，报错 `RefCell already borrowed`。

**根因分析：**

第六批修复只处理了 cursor blink 的 async task，但还有其他 async task 使用 `cx.update()`：
- GitHub validation task
- Autocomplete debounce task  
- Issue suggestions fetch task
- User suggestions fetch task

`AsyncApp::update()` 内部使用 `borrow_mut()` 会 panic，而 `AsyncApp::update_window()` 使用 `try_borrow_mut()` 返回 `Err` 不会 panic。

即使添加了 `is_dialog_open()` 检查，仍存在 race condition：
1. async task 检查 `is_dialog_open()` → false
2. 对话框打开（App 被借用）
3. async task 调用 `cx.update()` → panic

**修复方案：**

将所有 async task 中的 `cx.update()` 改为 `cx.update_window()`：

```rust
// 之前（会 panic）
cx.spawn(async move |weak, cx| {
    let result = some_async_work().await;
    let _ = cx.update(|cx| {
        if let Some(editor) = weak.upgrade() {
            editor.update(cx, |editor, cx| {
                // update editor state
            });
        }
    });
});

// 之后（安全）
let window = cx.windows().first().cloned();
cx.spawn(async move |weak, cx| {
    let result = some_async_work().await;
    if crate::file_ops::is_dialog_open() {
        return;
    }
    if let Some(window) = window {
        let _ = cx.update_window(window, |_, _window, cx| {
            if let Some(editor) = weak.upgrade() {
                editor.update(cx, |editor, cx| {
                    // update editor state
                });
            }
        });
    }
});
```

**涉及文件：**

| 文件 | 改动 |
|------|------|
| `src/editor/mod.rs` | 4 个 async task 改用 `update_window`：`spawn_github_validation`、`fetch_autocomplete_suggestions_debounced`、`fetch_issue_suggestions`、`fetch_user_suggestions` |

**经验总结：**

1. **`AsyncApp::update()` 使用 `borrow_mut()`** — 如果 App 已被借用会 panic，不能在对话框打开期间调用
2. **`AsyncApp::update_window()` 使用 `try_borrow_mut()`** — 返回 `Result`，被借用时返回 `Err` 而非 panic
3. **Race condition 无法仅靠标志位避免** — 即使检查 `is_dialog_open()`，在检查和调用之间仍可能被借用
4. **所有 async task 都应使用 `update_window`** — 不仅是对话框相关的 task，所有可能在运行时遇到 App 被借用的 async task 都应该使用安全的方法
5. **获取 window handle 的方法** — 在 `Context<T>` 中使用 `cx.windows().first().cloned()` 获取当前窗口的 handle

### 2026-06-10（第九批 — render() 中调用对话框的 RefCell Panic）

**问题：** 读取文件 → 新建 → 再读取时崩溃，报错 `RefCell already borrowed`。

**根因分析：**

之前的修复（第八批）只处理了 async task 中的 `cx.update()` 调用，但真正的问题在于 **在 `render()` 中直接调用 rfd 对话框**：

1. Action handler 设置 `pending_file_op = Some(op)`
2. `render()` 开头检查并执行 `open_file()` → 调用 `pick_open_file()` → rfd 打开对话框
3. rfd 在 Windows 上创建嵌套消息循环（modal loop）
4. 但 `render()` 是在 GPUI 的 `update_window` 内部调用的，此时 App 已被 `borrow_mut`
5. 嵌套消息循环中 GPUI 尝试再次访问 App → panic

**关键洞察：** `render()` 执行时 App 已被借用，任何在 `render()` 中打开对话框的操作都会导致嵌套消息循环，进而触发 RefCell panic。

**修复方案：**

使用 `window.defer()` 把文件操作推迟到 `render()` 返回后执行：

```rust
impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(op) = self.pending_file_op.take() {
            let entity = cx.entity().clone();
            window.defer(cx, move |_window, cx| {
                entity.update(cx, |editor, cx| {
                    match op {
                        FileOp::Save => editor.save(cx),
                        FileOp::SaveAs => editor.save_as(cx),
                        FileOp::Open => editor.open_file(cx),
                        FileOp::New => editor.new_file(cx),
                    }
                    set_dialog_open(false);
                });
            });
        }
        // ... rest of render
    }
}
```

`window.defer()` 会在当前帧渲染完成后执行回调，此时 App 的借用已释放，rfd 对话框可以安全创建嵌套消息循环。

**涉及文件：**

| 文件 | 改动 |
|------|------|
| `src/editor/mod.rs` | `render()` 中使用 `window.defer()` 推迟文件操作 |

**经验总结：**

1. **`render()` 中不能打开对话框** — `render()` 执行时 App 已被 `borrow_mut`，任何创建嵌套消息循环的操作都会导致 RefCell panic
2. **`window.defer()` 是安全的推迟机制** — 回调在当前帧渲染完成后执行，此时 App 的借用已释放
3. **rfd 在 Windows 上创建嵌套消息循环** — 这是导致 RefCell panic 的根本原因，必须确保调用时 App 未被借用
4. **`cx.entity().clone()` 获取 Entity** — 在 `defer` 回调中需要克隆 Entity，因为闭包需要 `'static` 生命周期
5. **Action handler 中可以直接使用 `window.defer()`** — 比在 `render()` 中检查 pending 标志更简洁可靠

### 2026-06-10（第十批 — watch_file async loop 缺少 dialog 守卫）

**问题：** 通过菜单或快捷键触发文件操作后报错 `RefCell already borrowed`。

**根因分析：**

之前的批次（第六~九批）已经修复了以下路径的 RefCell panic：
- cursor blink async task（第六批加 `is_dialog_open()` 守卫）
- GitHub validation、autocomplete、suggestions 等 async task（第八批改用 `update_window`）
- Action handler 直接打开对话框（第九/十批改用 `window.defer()`）

但还有一个关键路径遗漏了：`watch_file` 的 async loop。

```
watch_file (editor/mod.rs:2364)
  → cx.spawn(async loop)
    → timer(100ms).await
    → cx.update(|cx| { ... })   ← 没有 is_dialog_open() 检查！
```

当 rfd 对话框打开时（嵌套消息循环），`watch_file` 的 timer 触发：

1. 对话框打开 → App 被外层 `update_window` 借用 → `borrow_mut` 未释放
2. `watch_file` 的 timer 到期 → `cx.update()` 调用 `borrow_mut`
3. 但 App 仍在步骤 1 的 borrow 中 → `RefCell already borrowed` → panic

其他所有 async task 都已加守卫，但 `watch_file` 是后来新增的，遗漏了。

**修复方案：**

在 `watch_file` 的 async loop 中添加 `is_dialog_open()` 守卫，与其他 async task 保持一致：

```rust
loop {
    timer(100ms).await;

    let mut continue_loop = true;
    if !crate::file_ops::is_dialog_open() {   // ← 新增
        continue_loop = cx
            .update(|cx| {
                // ... 文件变更检查 ...
            })
            .unwrap_or(false);
    }

    if !continue_loop { break; }
}
```

**涉及文件：**

| 文件 | 改动 |
|------|------|
| `src/editor/mod.rs` | `watch_file` 的 async loop 添加 `is_dialog_open()` 守卫 |

**经验总结：**

1. **新增的 async task 必须添加 dialog 守卫** — 不是所有 async task 都能在创建时预见到所有保护措施，新的 async loop 要沿用现有模式
2. **`is_dialog_open()` 是防御性方案，不是根治** — 真正的问题是 GPUI 的 `RefCell<App>` 全局借用机制不支持嵌套消息循环
3. **对话框打开期间的 timer 仍然会触发** — `background_executor().timer()` 不受 rfd 对话框阻塞影响，await 后恢复执行


### 2026-06-10（第十一批 — 关闭前未保存确认）

**问题：** 关闭窗口时直接退出，没有未保存确认。

**修复方案：**

`CloseWindow` action handler 中拦截关闭，检查是否有未保存变更：

1. `editor.is_dirty()` 判断是否有变更
2. 有变更时弹出 `confirm_discard()` 对话框（Save / Don't Save / Cancel）
3. `editor.update()` 返回 `bool` 决定是否关闭窗口
4. Save → `!editor.is_dirty()` —— 保存成功才关（SaveAs 取消时保持打开）
5. Cancel → `false` —— 不关闭
6. Don't Save → `true` —— 直接关闭

```rust
.on_action(cx.listener(
    |this: &mut RootView, _: &CloseWindow, window, cx| {
        let editor = this.editor.clone();
        if editor.read(cx).is_dirty() {
            rustmd::file_ops::set_dialog_open(true);
            window.defer(cx, move |window, cx| {
                let should_close = editor.update(cx, |editor, cx| {
                    match rustmd::file_ops::confirm_discard() {
                        rustmd::file_ops::DiscardChoice::Save => {
                            editor.save(cx);
                            !editor.is_dirty()
                        }
                        rustmd::file_ops::DiscardChoice::Cancel => false,
                        rustmd::file_ops::DiscardChoice::DontSave => true,
                    }
                });
                rustmd::file_ops::set_dialog_open(false);
                if should_close { window.remove_window(); }
            });
        } else {
            window.remove_window();
        }
    },
))
```

**涉及文件：**

| 文件 | 改动 |
|------|------|
| `src/main.rs` | CloseWindow action handler 添加未保存确认逻辑；使用 `cx.listener()` + `window.defer()` |

**经验总结：**

1. **`return` 在闭包中只退出闭包** — 需要在 `editor.update()` 外部判断是否继续执行 `window.remove_window()`
2. **`editor.update(cx, ...)` 可以返回值** — 利用返回值传递决策（是否关闭窗口），避免标志位或外部状态
3. **`cx.listener()` 提供 `WindowContext`** — 比 bare `on_action(|...|)` 更方便使用 `window.defer()`

### 2026-06-11（第十二批 — Ctrl+L 居中、滚动 RefCell 修复、scroll beyond last line）

**新增功能：Ctrl+L 居中当前行（Mac mode）**

| 文件 | 改动 |
|------|------|
| `src/editor/action.rs` | 新增 `CenterLine` Action |
| `src/editor/mod.rs` | `pub use action::{CenterLine, ...}`；render 中添加 `.on_action` handler |
| `src/main.rs` | 注册 `ctrl-l` keybinding |

**居中实现：**

```
Case A（行可见有 bounds）→ 立即 scroll_by(offset)
Case B（行不可见/未测量）→ scroll_to_reveal_item + window.defer 延迟到布局后居中
```

**关键修复：watch_file async loop RefCell panic**

| 问题 | 根因 | 修复 |
|------|------|------|
| 窗口拖动时崩溃 | `watch_file` 的 async loop 使用 `cx.update()`（`borrow_mut` 会 panic）；窗口拖动期间 `SendMessageW(WM_NCLBUTTONDOWN)` 创建嵌套消息循环，App RefCell 被事件处理器借用 | `watch_file` 改为 `cx.update_window(window, ...)`（`try_borrow_mut` 返回 `Err` 而非 panic） |

**scroll beyond last line**

为使最后一行也能居中，增加最后一项的 `padding_bottom`：

```rust
let padding_bottom = padding_bottom_px + viewport_h / 2.0;
```

效果类似 VS Code 的 `editor.scrollBeyondLastLine`。副作用：在最后一行时有打字机式自动居中。

**其他：**

| 文件 | 改动 |
|------|------|
| `src/key_mode.rs` | `Default::default()` 改为 `Self::Mac` |

**涉及文件：**

| 文件 | 改动 |
|------|------|
| `src/editor/action.rs` | 新增 `CenterLine` |
| `src/editor/mod.rs` | CenterLine handler、scroll_beyond_last_line、watch_file 改用 update_window |
| `src/main.rs` | 注册 ctrl-l |
| `src/key_mode.rs` | 默认 Mac mode |

### 2026-06-11（第十三批 — JSON 配置、默认启动、光标闪烁修复、scroll beyond 微调）

**JSON 配置文件**

新增 `src/user_config.rs`，首次启动自动创建 `config.json`（路径由 `dirs::config_dir()` 决定）：

| 平台 | 路径 |
|------|------|
| Windows | `%APPDATA%\rustmd\config.json` |
| macOS | `~/Library/Application Support/rustmd/config.json` |
| Linux | `~/.config/rustmd/config.json` |

内置 dracula/nord 两套预设，用户可自定义完整色值。

| 文件 | 改动 |
|------|------|
| `src/user_config.rs` | **新建** — UserConfig、SerializedTheme、load_config/save_config |
| `Cargo.toml` | 添加 `dirs = "5"` 依赖 |
| `src/lib.rs` | `pub mod user_config;` |
| `src/main.rs` | 启动时 `load_config()` 加载 theme/font 写入 `EditorConfig` |
| `src/editor/mod.rs` | `pub mod theme;`（导出供 user_config 使用） |

**光标闪烁修复**

**根因：** `start_cursor_blink` 在 `Editor::new` 期间调用时 `cx.windows()` 返回空列表。

**修复：** 改为外部传入 `AnyWindowHandle`，从 `with_config` 中移除调用，在 main.rs 中 `cx.new()` 返回后用 `window.window_handle()` 传入。

| 文件 | 改动 |
|------|------|
| `src/editor/mod.rs` | 签名改为 `(handle: AnyWindowHandle, cx)`；导入 `AnyWindowHandle`；移除 `with_config` 中的调用 |
| `src/main.rs` | `cx.new()` 后 `editor.update(cx, \|editor, cx\| editor.start_cursor_blink(handle, cx))` |

**scroll beyond 微调**

打字机效应的根因：auto-scroll 用 `item_bounds.size.height`（含视图半高空白）判断光标位置。最后一行改用 `line_height` 替代。

**默认启动行为**

`config.rs` 移除 `file` 参数的 `required_unless_present = "demo"`，无参数自动空白新建。启动时 `eprintln!("[rustmd] config: {:?}", path)` 显示配置路径。

**经验总结：**

1. **GPUI AsyncApp 没有 `windows()`** — `ViewContext::windows()` 在构造阶段可能返回空。需用 `window.window_handle()` 获取 handle
2. **`dirs::config_dir()` 各平台不同** — Windows `%APPDATA%`，macOS `~/Library/Application Support`，Linux `~/.config`
3. **`EditorConfig.theme` 需显式传入** — `..Default::default()` 不会自动使用 `cx.set_global` 的 theme

### 2026-06-11（第十四批 — IME 取消拼音残留 + 不同 IME 兼容性修复）

**问题：** Esc/Backspace IME 取消后拼音文本残留；手心输入法（Shouxin）完成输入后遗留首字符；微软输入法退格删空后遗留首字符。

**根因分析：**

1. **取消时不删除 composition 文本** — 第一轮修复仅清除了 `ime_marked_range` 并调用 `cx.notify()`，但 buffer 里的拼音没有被 `delete()` 掉，导致残留
2. **首字符前置条件检查引发副作用** — 第一轮添加的 `&full[before..cursor] != new` 检查，当 Shouxin 发送含空格的 composition 字符串时会触发 fallback insert，把拼音再次插入 buffer 而非替换
3. **不同 IME 使用不同事件路径** — 微软拼音通过 `replace_and_mark_text_in_range` 维护 `ime_marked_range`，Shouxin 可能通过 `replace_text_in_range` 发送组合更新（或不发后续更新），`ime_marked_range` 始终为 `None`

**修复方案：**

| 方法 | 改动 |
|------|------|
| `replace_text_in_range` | 空字符串时 `buffer.delete(mark)` 删除 composition 文本；非 ASCII 分支增加未标记 composition 启发式（光标前 ASCII 字母替换为中文） |
| `replace_and_mark_text_in_range` | 空字符串时 `buffer.delete(mark)`；回退原始第一字符逻辑（`cursor.saturating_sub(new_len)`），移除 precondition 检查 |
| `unmark_text` | `if let Some(mark)` 时检查 marked_text 是否仍为 ASCII 字母，是则删除（安全防护，应对 IME 只调 `unmark_text` 不发送空字符串的场景） |
| `on_key_down`（mod.rs） | 保留第一轮的越界 staleness 检查 |

**涉及文件：**

| 文件 | 改动 |
|------|------|
| `src/editor/ime.rs` | 三轮修改：取消时删除 composition 文本、移除 precondition 检查、未标记 composition 启发式、unmark_text 带 ASCII 守卫的删除 |

**经验总结：**
见本章第八节第 13 ~ 16 条。

### 2026-06-12（第十五批 — 多窗口支持 + 图标工具栏 + About 浮层）

**版本：** 0.1.1 → 0.2.0

**功能变更：**

| 功能 | 说明 |
|------|------|
| 多窗口 | 工具栏 New Window 按钮 + `Ctrl+Shift+N`，每个窗口独立文档 |
| 图标工具栏 | 文字按钮改为图标：🦀 📄 📂 💾 │ 🔲 ⌨ |
| About 浮层 | 🦀 点击弹出版本信息、writ 致谢、Open Config Directory 链接 |
| 全局点击关闭 | 点击浮层外部任意位置自动关闭 |

**架构重构：**

| 改动 | 说明 |
|------|------|
| 移除 3 个 Global trait | `FileInfo`、`StatusBarInfo`、`CursorScreenPosition` 改为 per-window 数据 |
| RootView 控制布局 | 之前 WindowShadow 内部渲染 title_bar + 编辑器 + status_bar，现在 RootView 直接组装完整布局 |
| WindowShadow 回归装饰 | 仅保留阴影、圆角、拖拽边框功能 |
| Editor 存储自有状态 | 新增 `status_info`、`window_handle`、`cursor_screen_pos` 字段 |
| CursorScreenPosition 共享 | 通过 `Rc<RefCell<>>` 在 Editor 和 Line painting 之间传递 |
| 窗口 factory | 提取 `open_new_window()` 函数，可从任意 action handler 调用 |

**涉及文件：**

| 文件 | 改动 |
|------|------|
| `src/editor/mod.rs` | 新增 `status_info`/`window_handle`/`cursor_screen_pos` 字段；移除全局写入；`start_cursor_blink` 改为 `&mut self`；async task 改用存储的 `window_handle` |
| `src/line.rs` | `CursorScreenPosition` 移除 `Global` impl；Line 通过 `Rc<RefCell<>>` 写入 cursor 位置 |
| `src/title_bar.rs` | 接受 `&FileInfo` 参数；移除 `FileInfo::global()` |
| `src/status_bar.rs` | 接受 `&StatusBarInfo`/`&EditorTheme`/`&Config` 参数；移除全局读取 |
| `src/window.rs` | WindowShadow 仅保留装饰；新增 `NewWindow` action |
| `src/main.rs` | RootView 控制完整布局；窗口 factory 函数；`Ctrl+Shift+N` 快捷键；About 浮层 |
| `src/menu.rs` | 图标工具栏（Unicode）；`ToggleAbout` action |
| `src/file_ops.rs` | 移除已废弃的 `update_file_info_global`/`update_file_info_from_editor` |
| `src/user_config.rs` | `config_path()` 改为 `pub` |

**经验总结：**

1. **GPUI `flex_1()` 需要 flex 容器父级** — 移除 title_bar/status_bar 后，WindowShadow 内部不再是 flex 容器，残留的 `flex_1()` 导致编辑区高度塌缩为零。修复：改 `size_full()`
2. **`div()` 可以被闭包参数遮蔽** — `.when(cond, |div| { ... div() ... })` 中 `div` 参数与 `div()` 函数重名。修复：重命名闭包参数
3. **`when` 方法需要 `FluentBuilder` trait 在作用域** — `use gpui::*` 不够，需额外 `use gpui::prelude::FluentBuilder`
4. **全屏遮罩 + 绝对定位 = 全局点击关闭** — 遮罩层（size_full + absolute）覆盖窗口，浮层作为同级后续元素自然在遮罩层之上。点击遮罩关闭，点击浮层正常交互
5. **`open` crate 可跨平台打开文件/目录** — `open::that(path)` 调用 OS 默认打开方式，Win32 上等价于 `ShellExecuteW("open", path)`

---

## 十三、2026-06-13：问题修复与功能完善

### 1. 删除死亡代码 demo.rs

`demo_script()`、`DemoStep`、`DemoTiming` 全部定义但从未被调用，整文件死亡代码。直接删除并移除 `lib.rs` 中的 `pub mod demo`。

### 2. 终端窗口修复

**问题：** release 编译后运行时弹出控制台窗口，关闭控制台导致主程序退出。

**根因：** 缺少 `#![windows_subsystem = "windows"]` 属性，Windows 将程序识别为控制台应用；`user_config.rs` 的 `eprintln!` 向 stderr 输出配置路径。

**对策：**
- `main.rs` 顶部加 `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` — release 模式不显示终端，debug 模式保留以查看 panic 信息
- `eprintln!` 用 `#[cfg(debug_assertions)]` 包裹

### 3. 主题代码重构

**背景：** 代码中 `dracula()` / `nord()` 命名暗示存在"主题类型"，实际上 `EditorTheme` 只是一包颜色值，config 可以覆盖任意色值。

**改动：**
- `editor/theme.rs`：移除 `EditorTheme::dracula()` 方法，`Default` 直接硬编码 Dracula 色值
- `user_config.rs`：新增 `pub enum Preset { Dracula, Nord }` + `SerializedTheme::from_preset(&Preset)` 工厂方法
- `dracula()` / `nord()` 改为私有方法，外部通过 `from_preset()` 访问

### 4. Windows 图标嵌入

**背景：** 编译后的 exe 没有图标。

**做法：**
- `build.rs`：调用 `embed_resource::compile("res/icon.rc", std::iter::empty::<&str>())`
- `res/icon.rc`：`MAINICON ICON "..\\code.ico"`
- `Cargo.toml`：`[build-dependencies]` 加 `embed-resource = "2"`

**注意：** embed-resource v2.5.2 的 `compile()` 需要两个参数（第二个参数为宏定义迭代器），传空迭代器即可。v3 已改为单参数。

### 5. README 完善

- 新增**工具栏图标对照表**（图标/功能/快捷键/悬停提示四列）
- 新增**主题配置完整文档**（默认 JSON 示例、全部 11 个色值用途说明、Dracula/Nord 双色对照列）
- 新增**命令行参数表**（`--file`/`--autosave`/`--github-token` 等）
- 去除已删除的 `demo.rs` 架构引用，替换为 `tooltip.rs`

### 6. 鼠标悬停提示（Tooltip）

**需求：** 工具栏图标悬停时显示功能注释。

**实现方案：**
- `src/tooltip.rs`（新建）：`Tooltip` 结构体实现 `Global` trait，提供 `show(text, cx)` / `hide(cx)` 静态方法
- `src/menu.rs`：`ToolbarButton` 增加 `description` 字段；每个按钮绑定 `on_hover` 事件，hover 时调用 `Tooltip::show()`，离开时调用 `Tooltip::hide()`
- `src/main.rs`：`RootView::render()` 读取 `Tooltip::global(cx)`，有内容时渲染为绝对定位的白字深底圆角标签（位于标题栏下方）

**技术细节：**
- GPUI 0.2 的 `on_hover` 方法在 `StatefulInteractiveElement` trait 上，需额外 `use gpui::StatefulInteractiveElement`
- 回调参数 `&bool`：`true` = 鼠标进入，`false` = 鼠标离开
- Tooltip 在鼠标点击按钮时也会自动隐藏（`on_mouse_down` 中调用 `Tooltip::hide`）

