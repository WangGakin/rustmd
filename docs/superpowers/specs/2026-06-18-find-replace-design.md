# Find & Replace 功能设计

> 日期: 2026-06-18
> 项目: rustmd — GPUI Markdown 实时渲染编辑器

---

## 一、概述

为 rustmd 添加 VSCode 风格的查找替换功能。通过工具栏放大镜按钮触发，搜索栏浮层展示在编辑器右上角，支持纯文本匹配、大小写切换、逐跳和全部替换。

---

## 二、状态模型

在 `src/editor/find.rs` 中定义 `FindState`：

```rust
pub struct FindState {
    pub visible: bool,              // 搜索栏是否显示
    pub query: String,              // 查找内容
    pub replace_text: String,       // 替换内容
    pub matches: Vec<Range<usize>>, // 所有匹配的字节范围（按文档顺序）
    pub current_match: Option<usize>, // 当前激活匹配的索引
    pub match_case: bool,           // 区分大小写
    pub replace_visible: bool,      // 替换输入框展开
}
```

Editor struct 增加 `find_state: Option<FindState>` 字段。初始为 `None`，首次点 🔍 时创建。

---

## 三、搜索算法

使用 `regex` crate（已存在于依赖），将用户输入转义后做全量搜索：

```
用户输入 → regex::escape → 拼正则 → find_iter → 收集 Range<usize>
```

大小写敏感开关通过正则 `(?i)` 标记控制。

**触发时机：** 用户在查找输入框每输入一个字符，重新搜索全文档。当文档内容被编辑时，自动重新搜索。

**匹配数：** 搜索结果为空时，查找输入框边框变红（或输入框内显示 "No results"）。

---

## 四、匹配高亮

复用 `line.rs` 的 `inline_highlight_ranges` + `inline_highlight_color` 机制：

- **所有匹配**：淡橙色/黄色背景（`Hsla` 设置透明度）
- **当前匹配**：更亮或使用 selection 色作为背景
- 在 `editor/render.rs` 的 `build_line` 闭包中，当 `find_state` 可见时，提取本行范围内的匹配区间传入

---

## 五、导航与替换

### 查找导航
- `FindNext`：`current_match += 1`，光标移动到匹配范围开始，选中匹配文本
- `FindPrevious`：`current_match -= 1`，同样移动光标和选区
- 到达末尾/开头时循环回起点/终点
- 快捷键：`Enter` 下一个，`Shift+Enter` 上一个

### 替换
- **替换当前**：用 `replace_text` 替换 `matches[current_match]`，然后重新搜索并定位到原位置的下一个匹配
- **全部替换**：从末尾到开头反向逐个替换（避免偏移漂移），完成后清空匹配列表

替换操作通过 Editor 已有的 `edit_buffer` 或直接操作 `state.buffer` 完成，自动进入撤销栈。

---

## 六、UI 布局

搜索栏渲染为 Editor 内部的绝对定位浮层（跟随 Editor 可视区域右上角）：

```
┌──────────────────────────────────┐
│  🔍  ████████████      ▼▲   ✕   │  查找行
│      ████████████      ↻   ⊡    │  替换行（展开时显示）
│      [Aa] 区分大小写             │  选项行
└──────────────────────────────────┘
```

- `🔍`：搜索图标（静态）
- 输入框：点击聚焦，捕获键盘输入
- `▼`：下一个匹配
- `▲`：上一个匹配
- `↻`：替换当前
- `⊡`：全部替换
- `✕` / `Esc`：关闭搜索栏
- `Aa`：切换大小写
- 替换行：默认隐藏，点击替换区域时展开

---

## 七、键盘输入路由

当搜索栏可见时，Editor 的 `on_key_down` 需要区分输入目标：

- 若搜索栏输入框获得焦点，字符键、Backspace、Delete、方向键、Enter、Esc 由搜索栏处理
- 否则由编辑器正常处理
- 搜索栏获得焦点时，Editor 本身的输入被阻断（`input_blocked` 或独立判断）

实现方式：搜索栏使用独立的 `FocusHandle`（或者用布尔标记 + 条件分支）。

---

## 八、工具栏集成

在 `src/menu.rs` 的 `get_toolbar_buttons()` 中，在 Save 按钮后添加 🔍 按钮，dispatch `ToggleFind` action。

| 图标 | 功能 | 快捷键 | 鼠标悬停提示 |
|------|------|--------|-------------|
| 🔍 | 查找替换 | — | Toggle find and replace |

---

## 九、文件变更清单

| 文件 | 操作 | 内容 |
|------|------|------|
| `src/editor/find.rs` | 新增 | `FindState`、搜索算法、替换逻辑、UI 事件处理 |
| `src/editor/mod.rs` | 修改 | 添加 `mod find`、Editor 字段 `find_state`、`on_key_down` 路由、新增 action handler |
| `src/editor/render.rs` | 修改 | 搜索栏浮层渲染、匹配高亮传递 |
| `src/editor/action.rs` | 修改 | 新增 `ToggleFind`、`FindNext`、`FindPrevious`、`ReplaceNext`、`ReplaceAll` |
| `src/menu.rs` | 修改 | 工具栏添加 🔍 按钮 |
| `src/main.rs` | 修改 | 注册 `ToggleFind` action（如果需要窗口级路由） |

---

## 十、错误处理

- 空查询：不搜索，不显示匹配
- 查询未找到匹配：搜索栏输入框视觉反馈（边框变色 + 提示文字）
- 替换操作时文档被外部修改：通过已有的 `version` 检测机制，若 buffer 版本变化则重新搜索

---

## 十一、未包含（明确排除）

- 正则表达式搜索（需求明确为基础文本搜索）
- Whole Word 匹配（未来可扩展）
- 跨行匹配（未来可扩展）
- 搜索结果计数悬浮球（VSCode 的匹配数提示，简化处理，仅在输入框显示计数）
