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
