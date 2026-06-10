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

1. ~~上轮未能修复的问题：markdown语法输入过程中，实时渲染-编辑状态反复转换，导致输入区域闪烁，应固定在编辑状态，等光标移走再实时渲染。~~ **已修复（2026-06-10）**：根因是 `visual_cursor_offset` 在光标 blink 关闭阶段被设为 `usize::MAX`，导致 Line 组件认为光标不在当前行，从而隐藏所有 markdown 标记（切换到渲染模式）。修复方案：将光标位置（用于编辑模式检测）与光标可见性（用于 blink 动画）解耦，新增 `show_cursor: bool` 字段独立控制光标视觉渲染。

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

### 2026-06-10（第十批 — Action Handler 中直接使用 defer）

**问题：** 第九批修复在 `render()` 中使用 `window.defer()` 仍然崩溃。

**根因分析：**

`window.defer()` 在 `render()` 中调用时，回调可能仍在 App 被借用的上下文中执行。因为 `render()` 本身是在 GPUI 的事件处理循环中被调用的，App 的借用贯穿整个事件处理过程。

**修复方案：**

在 action handler 中直接使用 `window.defer()`，而不是在 `render()` 中：

```rust
.on_action(cx.listener(
    |_editor: &mut Editor, _: &crate::file_ops::OpenFile, window, cx| {
        crate::file_ops::set_dialog_open(true);
        let entity = cx.entity().clone();
        window.defer(cx, move |_window, cx| {
            entity.update(cx, |editor, cx| {
                editor.open_file(cx);
                crate::file_ops::set_dialog_open(false);
            });
        });
    },
))
```

Action handler 执行完毕后，GPUI 的事件处理会返回，App 的借用会释放。然后 `defer` 回调执行时，App 已未被借用，rfd 对话框可以安全创建嵌套消息循环。

**涉及文件：**

| 文件 | 改动 |
|------|------|
| `src/editor/mod.rs` | 移除 `pending_file_op` 字段；4 个 action handler 直接使用 `window.defer()` |

**经验总结：**

1. **`render()` 中的 `defer` 仍可能不安全** — 因为 `render()` 在事件处理循环中被调用，App 的借用贯穿整个过程
2. **Action handler 中的 `defer` 是安全的** — Action handler 执行完毕后，事件处理会返回，App 的借用会释放
3. **移除 `pending_file_op` 字段** — 不再需要在 `render()` 中检查 pending 标志，代码更简洁
4. **`set_dialog_open(true)` 在 `defer` 之前调用** — 确保在对话框打开前设置标志，防止 cursor blink 等 async task 在对话框打开期间尝试访问 App

### 2026-06-10（第十一批 — 关闭前未保存确认 & watch_file RefCell 修复）

**问题 1：** 关闭窗口时直接退出，没有未保存确认。

**问题 2：** watch_file 的 async loop 调用 `cx.update()` 导致 `RefCell already borrowed` panic。

**修复方案：**

1. **CloseWindow 拦截** (`src/main.rs`)
   - `CloseWindow` action handler 中先检查 `editor.is_dirty()`
   - 有变更时弹出 `confirm_discard()` 对话框（Save / Don't Save / Cancel）
   - 用 `window.defer()` 推迟对话框操作，避免在 render 中打开模态对话框
   - `editor.update()` 返回 `bool` 决定是否关闭窗口

2. **watch_file 守卫** (`src/editor/mod.rs`)
   - 与 cursor blink 相同模式，在 `cx.update()` 前加 `is_dialog_open()` 检查
   - 对话框打开期间跳过文件变更检查，避免嵌套 borrow

**涉及文件：**

| 文件 | 改动 |
|------|------|
| `src/main.rs` | CloseWindow action handler 添加未保存确认逻辑；使用 `cx.listener()` + `window.defer()` |
| `src/editor/mod.rs` | watch_file loop 添加 `is_dialog_open()` 守卫 |

**经验总结：**

1. **`return` 在闭包中只退出闭包** — 需要在 `editor.update()` 外部判断是否继续执行 `window.remove_window()`
2. **`editor.update(cx, ...)` 可以返回值** — 利用返回值传递决策（是否关闭窗口），避免标志位或外部状态
3. **`cx.listener()` 提供 `WindowContext`** — 比 bare `on_action(|...|)` 更方便使用 `window.defer()`

