# 编辑器功能实现经验

## 文件操作（第四批 — 2026-06-10）

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

---

## 自绘菜单与客户端装饰（第五批 — 2026-06-10）

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

---

## 窗口拖动与红绿灯控制（第七批 — 2026-06-10）

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
   
   if window.is_maximized() {
       // ...ShowWindowAsync(hwnd, SW_RESTORE)
   } else {
       window.zoom_window();
   }
   ```

3. **菜单自动关闭** — 在编辑区添加 `on_mouse_down` 监听器关闭菜单状态。

**新增依赖：**

```toml
raw-window-handle = "0.6"
windows = { version = "0.61", features = [
    "Win32_UI_WindowsAndMessaging",
    "Win32_Foundation",
    "Win32_UI_Input_KeyboardAndMouse"
] }
```

**经验总结：**

1. GPUI 的 `start_window_move()` 在 Windows 未实现
2. `ShowWindow` 会导致重入问题，使用 `ShowWindowAsync` 更安全
3. 获取 HWND 需要 `raw-window-handle` 的 `HasWindowHandle` trait
4. 菜单关闭需要显式监听，`window.refresh()` 在某些情况下强制重绘

---

## Ctrl+L 居中当前行（第十二批 — 2026-06-11）

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

---

## JSON 配置与主题（第十三批 — 2026-06-11）

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

---

## 多窗口 + 图标工具栏 + About 浮层（第十五批 — 2026-06-12）

**版本：** 0.1.1 → 0.2.0

| 功能 | 说明 |
|------|------|
| 多窗口 | 工具栏 New Window 按钮 + `Ctrl+Shift+N`，每个窗口独立文档 |
| 图标工具栏 | 文字按钮改为图标：🦀 📄 📂 💾 │ 🔲 ⌨ |
| About 浮层 | 🦀 点击弹出版本信息、writ 致谢、Open Config Directory 链接 |
| 全局点击关闭 | 点击浮层外部任意位置自动关闭 |

**架构重构：** 移除 3 个 Global trait，Editor 存储自有状态，通过 `Rc<RefCell<>>` 在 Editor 和 Line painting 之间传递 `CursorScreenPosition`，提取 `open_new_window()` 工厂函数。

**经验总结：**

1. GPUI `flex_1()` 需要 flex 容器父级
2. `div()` 可以被闭包参数遮蔽
3. `when` 方法需要 `FluentBuilder` trait 在作用域
4. 全屏遮罩 + 绝对定位 = 全局点击关闭
5. `open` crate 可跨平台打开文件/目录

---

## 13. 2026-06-13：问题修复与功能完善

### 1. 删除死亡代码 demo.rs

整文件死亡代码。直接删除并移除 `lib.rs` 中的 `pub mod demo`。

### 2. 终端窗口修复

**问题：** release 编译后弹出控制台窗口，关闭控制台导致主程序退出。

**对策：**
- `main.rs` 顶部加 `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]`
- `eprintln!` 用 `#[cfg(debug_assertions)]` 包裹

### 3. 主题代码重构

`editor/theme.rs`：移除 `EditorTheme::dracula()` 方法，`Default` 直接硬编码 Dracula 色值。`user_config.rs`：新增 `Preset { Dracula, Nord }` + `from_preset()` 工厂方法。

### 4. Windows 图标嵌入

`build.rs` + `res/icon.rc` + `Cargo.toml` 添加 `embed-resource = "2"`。

**注意：** embed-resource v2.5.2 的 `compile()` 需要两个参数（第二个参数为宏定义迭代器），传空迭代器即可。v3 已改为单参数。

### 5. README 完善

工具栏图标对照表、主题配置完整文档、命令行参数表。

### 6. 鼠标悬停提示（Tooltip）

`src/tooltip.rs`（新建）：`Tooltip` 结构体实现 `Global` trait，提供 `show(text, cx)` / `hide(cx)` 静态方法。按钮绑定 `on_hover` 事件。RootView 渲染时读取 Tooltip 内容。

**技术细节：**
- GPUI 0.2 的 `on_hover` 方法在 `StatefulInteractiveElement` trait 上
- 回调参数 `&bool`：`true` = 鼠标进入，`false` = 鼠标离开

### 7. 扩展代码高亮语言

新增 Python、JavaScript、C#、CSS、JSON 五种语言高亮。每个语言按相同模式添加 `create_xxx_config()` + `Highlighter::new()` 中注册。

### 8. 修复 icon.rc 文件路径问题

release 构建时 RC.EXE 找不到 `..\code.ico`（路径解析相对于 RC 工作目录）。将 `code.ico` 复制到 `res/` 目录中，`icon.rc` 直接引用 `code.ico`（无相对路径）。

### 10. 编辑器滚动条（2026-06-13）

**实现方案：** 滚动条作为 `Editor::render()` 的内联子元素，绝对定位于编辑区右侧。轨道 8px 宽，hover 展开至 12px；滑块、轨道均 4px 圆角。

**数据流：**

```
total_h = Σ measured item heights + unmeasured × default_line_h
thumb_h = max(track_h × viewport_h / total_h, 20px)
thumb_top = track_h × scroll_offset / total_h
```

**交互：** 拖拽滑块按比例滚动、点击轨道翻页、fallback 清理。

**涉及文件：**

| 文件 | 改动 |
|------|------|
| `src/editor/mod.rs` | 新增 `ScrollbarDrag` 标记结构体；`scrollbar_drag_start_y` 字段；`compute_total_content_height` / `compute_scroll_offset_pixels` 辅助方法；`render_scrollbar` 完整渲染逻辑 |

### 10b. 滚动条交互修复 — 延迟判定（2026-06-14）

**问题：** 原实现中 `on_mouse_down` 做二元判定——点击在滑块上 = 拖拽，点击在轨道上 = 翻页。长文章滑块最小仅 20px，几乎不可拖拽；任何微小鼠标偏移都被识别为翻页。

**修复（方案 B）：**
- `on_mouse_down`：任何点击都标记为潜在拖拽（`scrollbar_pending_page_turn = true`）
- `on_drag_move`：首次移动超过 3px 阈值 → 切换为真实拖拽模式；未超阈值的移动忽略
- `on_mouse_up`：若 `pending` 仍为 true（无拖拽移动）→ 根据点击位置相对滑块中心执行翻页

**经验总结：** 拖拽/点击的分类应从 `mousedown` 延迟到 `mouseup`，通过移动距离阈值区分意图。

| 文件 | 改动 |
|------|------|
| `src/editor/mod.rs` | 新增 `scrollbar_pending_page_turn` 字段；rewrite `on_mouse_down`/`on_drag_move`/`on_mouse_up` 三个回调；main editor `on_mouse_up` 清理新 flag |

### 10c. 滚动条修复 — 中文文本 panic 'invalid text run'（2026-06-14）

**问题：** 打开纯中文 Markdown 文件时 GPUI 在 `text.rs:239` panic `invalid text run`。堆栈指向 `StyledText::new().with_runs()`。

**根因：** `build_styled_content` 中的 `boundaries` 来自 tree-sitter 解析结果。在纯 CJK 文本（每个字符 3 UTF-8 字节）场景下，某个 boundary 落在一个字符的中间字节位置。`str::get(64..)` 要求索引在 UTF-8 字符边界上 → `None` → panic。

**修复：**
1. **边界规范化**（`build_styled_content`）：`boundaries` 去重后在 rope 上验证每个位置是否为有效 UTF-8 字符边界，非边界的向前调整到下一字符边界
2. **防御性验证**（所有 `with_runs` 调用前）：逐 run 模拟 GPUI 的验证逻辑，不匹配时用安全 fallback 替代

**经验总结：** tree-sitter 字节位置在特定场景（非 ASCII 文本）下可能不是有效的 UTF-8 边界。需要在渲染层做 boundary 规范化，而不是信任上游数据。

| 文件 | 改动 |
|------|------|
| `src/line.rs` | `build_styled_content` 新增 UTF-8 boundary 规范化步骤；主 `with_runs` 调用前添加逐 run 验证 + fallback；分隔线 `with_runs` 调用前同上 |

---

## 关闭前未保存确认（第十一批 — 2026-06-10）

`CloseWindow` action handler 中拦截关闭，`editor.is_dirty()` 判断是否有变更，弹 `confirm_discard()` 对话框（Save / Don't Save / Cancel）。

```rust
.on_action(cx.listener(
    |this: &mut RootView, _: &CloseWindow, window, cx| {
        let editor = this.editor.clone();
        if editor.read(cx).is_dirty() {
            // 弹确认框，Save → save 后关闭 / Cancel → 不关闭 / Don't Save → 直接关闭
        } else {
            window.remove_window();
        }
    },
))
```

**经验总结：**
1. `return` 在闭包中只退出闭包。利用 `editor.update()` 返回值传递决策。
2. `cx.listener()` 提供 `WindowContext`，比 bare `on_action` 更方便使用 `window.defer()`。

---

## scroll beyond last line（第十二批 — 2026-06-11）

为使最后一行也能居中，增加最后一项的 `padding_bottom`：

```rust
let padding_bottom = padding_bottom_px + viewport_h / 2.0;
```

效果类似 VS Code 的 `editor.scrollBeyondLastLine`。副作用：在最后一行时有打字机式自动居中。

---

## 光标闪烁修复（第十三批 — 2026-06-11）

**根因：** `start_cursor_blink` 在 `Editor::new` 期间调用时 `cx.windows()` 返回空列表。

**修复：** 改为外部传入 `AnyWindowHandle`，从 `with_config` 中移除调用，在 main.rs 中传入。

| 文件 | 改动 |
|------|------|
| `src/editor/mod.rs` | 签名改为 `(handle: AnyWindowHandle, cx)`；移除 `with_config` 中的调用 |
| `src/main.rs` | `cx.new()` 后传入 handle |

---

---

## 历史文件菜单（第十六批 — 2026-06-14）

在标题栏右侧新增 🕐 下拉菜单，记录最近打开的 5 个文件，支持跨会话持久化。

| 功能 | 说明 |
|------|------|
| 自动记录 | 打开文件、另存为、`--file` 启动时自动记录 |
| 快速打开 | 点击历史文件直接打开（dirty 时弹确认框） |
| 条目格式 | `文件名 — 父目录名` 显示，方便区分同名文件 |
| 已删除文件 | 灰色显示，点击后报错退出（不崩溃） |
| 清除历史 | 下拉菜单底部「Clear Recent Files」链接 |
| 持久化 | 存储在 `config.json` 的 `recent_files` 字段，最多 5 条 |
| 去重 | 同一文件重复打开自动移到列表最前 |

**涉及文件：**

| 文件 | 改动 |
|------|------|
| `src/user_config.rs` | `recent_files: Vec<String>` 字段；`RECENT_FILES` 静态缓存；`add_recent_file()` / `clear_recent_files()` / `recent_files()` |
| `src/file_ops.rs` | 新增 `OpenRecentFile(usize)`、`ClearRecentFiles` action 结构体 |
| `src/editor/mod.rs` | 提取 `open_file_at(path, cx)` 方法；在 `open_file`、`save_as` 中调用 `add_recent_file` |
| `src/title_bar.rs` | `FileInfo.recent_files` 字段；🕐 按钮（空列表时灰色，有文件时显示主题色带 hover）；`ToggleRecentFiles` action |
| `src/main.rs` | 注册 3 个 action handler；弹窗浮层（跟随 About 浮层模式）；`--file` 启动时记录路径 |

**弹窗 UI 说明：**

```
┌──────────────────────────┐
│ notes.md — Desktop       │
│ README.md — rustmd       │
│ ————————————————————     │
│ Clear Recent Files       │
└──────────────────────────┘
```

**经验总结：**

1. GPUI 的 `actions!` 宏不支持带数据参数的 action → 用 `#[derive(Action)]` + tuple struct
2. 下拉弹窗复用 About 浮层的 overlay 模式：全屏透明遮罩 + 绝对定位内容层
3. 关闭按钮在空列表时仍应保持可点击但视觉淡化（`.text_color(theme.comment)`.without `cursor_pointer`/`hover`）
4. `OpenRecentFile` handler 需要 `set_dialog_open(true)` + `window.defer()` 包裹 `confirm_discard()`，避免 rfd 嵌套消息循环导致 GPUI RefCell panic
5. 持久化操作（`load_config` + `save_config`）应放在 Mutex 锁外部，避免 I/O 阻塞其他线程读取最近文件列表

---

## 强调文字颜色（第十七批 — 2026-06-14）

**版本：** 0.2.2 → 0.2.3

粗体（`**bold**`）和斜体（`*italic*`）文本现在使用独立的强调色，与正文区分。颜色可通过 `config.json` 的 `theme.emphasis` 字段定制。

**默认配色：** Dracula `#F1FA8C`（黄），Nord `#EBCB8B`（黄）

**颜色优先级：** `strikethrough > checkbox > link > code_highlight > inline_code > **emphasis** > text_color`

标题不受影响，保持原有正文色。

| 文件 | 改动 |
|------|------|
| `src/editor/theme.rs` | `EditorTheme` 新增 `emphasis: Rgba` 字段 + Default 值 |
| `src/user_config.rs` | `SerializedTheme` 新增 `emphasis: String` + preset 默认值 + `to_editor_theme`/`from_editor_theme` 序列化 |
| `src/line.rs` | `LineTheme` 新增 `emphasis_color: Rgba`；`build_styled_content()` 颜色选择链新增 `is_bold \|\| is_italic → emphasis_color` |
| `src/editor/mod.rs` | `Editor::render()` 映射 `theme.emphasis → line_theme.emphasis_color` |

## Mac 模式全选快捷键修复（2026-06-14）

**问题：** Mac 模式下 `Ctrl+Shift+A`（全选）被 Emacs 风格快捷键块拦截，实际行为为"选中到行首"。

**根因：** `src/editor/mod.rs:2827` 的 Mac 模式处理块条件为 `is_mac_mode && is_ctrl && !alt`，匹配所有 `Ctrl+字母` 组合，包括 `Ctrl+Shift+A`。该块的 `"a"` arm 执行 `move_to_line_start()` + `move_cursor(new_cursor, extend=true)`，导致选中从光标到行首而非全选。第 2942 行的全选逻辑（要求 `is_ctrl_shift`）永远收不到事件。

| 文件 | 改动 |
|------|------|
| `src/editor/mod.rs:2829` | `"a"` 匹配臂增加 `!keystroke.modifiers.shift` 守卫条件 |

**修复效果：**
- `Ctrl+A`（无 shift）→ 移到行首（Emacs 行为，不变）
- `Ctrl+Shift+A` → 穿透到全选逻辑，正确执行 `Selection::select_all()`

**经验总结：** Mac 模式下 Emacs 快捷键处理块（line 2827）优先级高于主匹配块（line 2882），如果后续有需要 `Ctrl+Shift` 组合的快捷键，必须在对应 Emacs arm 上加 `!shift` 守卫，否则会被 Emacs 块提前拦截。

---

## RefCell / Async Task 安全模式汇总

### 根因

GPUI 的 `RefCell<App>` 全局借用机制不支持嵌套消息循环。rfd 对话框在 Windows 上创建 modal loop，此时 App 被 `borrow_mut`，任何 async task 尝试 `cx.update()`（内部 `borrow_mut`）都会 panic。

### 修复模式

| 模式 | 适用场景 | 代码 |
|------|----------|------|
| `is_dialog_open()` 守卫 | 定时触发的 async loop（cursor blink、watch_file） | 在调用 `cx.update()` 前检查，跳过对话框打开期间的执行 |
| `update_window` | 可能被对话框打断的 async task | 用 `cx.update_window(handle, \|_, _, cx\| ...)` 替代 `cx.update()`，`try_borrow_mut` 返回 `Err` 而非 panic |
| `window.defer()` | 在 `render()` 或 action handler 中打开对话框 | 推迟到当前帧渲染完成后执行，此时 App 借用已释放 |
| `pending_file_op` | render 中触发的文件操作 | 仅在 render 中设置标志，下一次 render 时检查并执行 (后改为 `window.defer()`) |

### 涉及批次

| 批次 | 修复内容 |
|------|----------|
| 第六批 | 首次 RefCell panic：cursor blink + `DIALOG_OPEN` 原子标志 + `pending_file_op` 机制 |
| 第八批 | 所有 async task 改用 `update_window` |
| 第九批 | `render()` 中使用 `window.defer()` 推迟文件操作 |
| 第十批 | `watch_file` async loop 添加 `is_dialog_open()` 守卫 |
| 第十二批 | `watch_file` 改用 `update_window(window, ...)` 彻底解决 |

---

## OpenFile 改为在新窗口打开（第十八批 — 2026-06-16）

**版本：** 0.3.0 → 0.4.0

原 `Ctrl+O` / 📂 在当前 Editor 替换打开，改为弹出文件对话框后在**新 OS 窗口**打开。

**行为变更：**

| 操作 | 之前 | 之后 |
|------|------|------|
| Ctrl+O / 📂 | 当前窗口替换文件内容 | 新窗口打开文件 |
| Ctrl+Shift+N / 🔲 | 新窗口（空白文档） | 不变 |
| 最近文件点击 | 当前窗口替换 | 不变（仍为当前窗口） |

**涉及文件：**

| 文件 | 改动 |
|------|------|
| `src/editor/render.rs` | 移除 `OpenFile` 的 `on_action` handler，让 action 冒泡到 `RootView` |
| `src/main.rs` | `RootView` 中新增 `OpenFile` handler：`pick_open_file()` → `open_new_window_with_file()`；新增 `open_new_window_with_file(path, cx)` 函数（读文件内容 + 创建窗口 + 设置文件监视器 + 写最近文件列表）；新增 `use std::path::PathBuf` import |

**实现要点：** `open_new_window_with_file` 复用 `open_new_window` 的窗口创建逻辑，差别在于：
- Editor 创建时传入文件内容 + 调用 `editor.watch_file(path, cx)`
- `FileInfo.path` 设为 `Some(path)`
- 调用 `user_config::add_recent_file(&path)`
