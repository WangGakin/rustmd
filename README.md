# rustmd

一个基于 GPUI 框架的 Markdown 实时渲染编辑器，灵感来源于 Typora。

rustmd 基于 [writ](https://github.com/wilfreddenton/writ) 编辑器内核构建，提供所见即所得的 Markdown 编辑体验——光标离开时隐藏 `**`、`#`、`-` 等标记符号，仅显示渲染后的样式效果。

## 主要特性

### 实时渲染

Markdown 语法在光标离开时自动隐藏标记符号，直接显示渲染后的样式效果。

**支持实时渲染的语法（标记隐藏 + 样式生效）：**

| 语法 | 渲染效果 | 说明 |
|------|---------|------|
| `# 标题` | 标题文字 | `#` 标记隐藏，显示不同字号和样式 |
| `**粗体**` / `__粗体__` | **粗体** | `**` 或 `__` 隐藏 |
| `*斜体*` / `_斜体_` | *斜体* | `*` 或 `_` 隐藏 |
| `~~删除线~~` | ~~删除线~~ | `~~` 隐藏 |
| `` `行内代码` `` | 行内代码 | 反引号隐藏 |
| `- 列表项` / `1. 列表项` | 列表 | 标记符号隐藏 |
| `[ ]` / `[x]` 任务列表 | 复选框 | 可点击切换状态 |
| `> 引用` | 引用块 | `>` 隐藏，显示左边框 |
| `[链接](url)` | 可点击链接 | 显示链接文字 |
| `https://...` | 可点击链接 | 裸 URL 自动检测 |

**不支持实时渲染的语法（保持原样显示或块级处理）：**

| 语法 | 处理方式 |
|------|---------|
| ` ``` ` 代码块 | 光标移出后隐藏围栏线，内容保留为代码样式 + 语法高亮 |
| `\| 表格 \|` | 以原始 pipe 语法显示 |

### 代码高亮

基于 tree-sitter 的代码块语法高亮，支持 Rust、Python、JavaScript、C#、CSS、JSON、Bash。

### 中文输入法支持

针对 Windows 中文输入法（微软拼音、手心输入法等）进行了深度适配，包括：

- IME 组合输入与 writ KeyDown 的双路冲突解决
- UTF-16 与 UTF-8 字节偏移的精确转换
- 中文标点全角/半角自动处理
- IME 取消时的残留文本清理

### 多窗口

支持多窗口编辑，每个窗口独立管理文档。通过工具栏按钮或 `Ctrl+Shift+N` 创建新窗口。

### 工具栏图标对照

| 图标 | 功能 | 快捷键 | 鼠标悬停提示 |
|------|------|--------|-------------|
| 🦀 | 关于 rustmd | — | 显示版本信息和配置目录入口 |
| 📄 | 新建文件 | `Ctrl+Alt+N` | 清空编辑器新建文档 |
| 📂 | 打开文件 | `Ctrl+O` | 弹出系统文件对话框 |
| 💾 | 保存 | `Ctrl+S` | 保存当前文件 |
| 🔲 | 新建窗口 | `Ctrl+Shift+N` | 打开独立的新编辑窗口 |
| 🕐 | 历史文件 | — | 显示最近打开的 5 个文件；空列表时灰色不可用 |
| ⌨ Win/Mac | 键盘模式切换 | — | 切换 Mac/Win 风格快捷键 |

> 鼠标悬停在工具栏图标上会显示功能说明和快捷键提示。

### 主题与字体

内置 Dracula 和 Nord 两套预设色板，支持通过 JSON 配置文件自定义完整色值。

| 平台 | 配置文件路径 |
|------|-------------|
| Windows | `%APPDATA%\rustmd\config.json` |
| macOS | `~/Library/Application Support/rustmd/config.json` |
| Linux | `~/.config/rustmd/config.json` |

首次启动时自动生成默认配置文件，内容如下：

```json
{
  "theme": {
    "background": "#282A36",
    "foreground": "#F8F8F2",
    "selection": "#44475A",
    "comment": "#6272A4",
    "red": "#FF5555",
    "orange": "#FFB86C",
    "yellow": "#F1FA8C",
    "green": "#50FA7B",
    "cyan": "#8BE9FD",
    "purple": "#BD93F9",
    "pink": "#FF79C6"
  },
  "text_font": "Segoe UI",
  "code_font": "Consolas",
  "font_size_rem": 0.875,
  "recent_files": [
    "C:/Users/me/Documents/notes.md",
    "C:/Projects/rustmd/README.md"
  ]
}
```

**色值字段说明：**

| 颜色 | 用途 | Dracula | Nord |
|------|------|---------|------|
| `background` | 编辑区背景 | `#282A36` | `#2E3440` |
| `foreground` | 默认文字 | `#F8F8F2` | `#D8DEE9` |
| `selection` | 选中/高亮背景 | `#44475A` | `#434C5E` |
| `comment` | 注释/次要文字 | `#6272A4` | `#616E88` |
| `red` | 删除线/危险 | `#FF5555` | `#BF616A` |
| `orange` | 属性/数值 | `#FFB86C` | `#D08770` |
| `yellow` | 字符串 | `#F1FA8C` | `#EBCB8B` |
| `green` | 函数名 | `#50FA7B` | `#A3BE8C` |
| `cyan` | 类型/链接 | `#8BE9FD` | `#88C0D0` |
| `purple` | 关键字/常量 | `#BD93F9` | `#B48EAD` |
| `pink` | 运算符/属性 | `#FF79C6` | `#BF88BC` |

**切换主题：** 将对应列的色值复制到 `config.json` 的 `theme` 字段下即可。也可自由调整任意色值实现自定义主题。

### 历史文件

标题栏右侧的 🕐 按钮记录最近打开的 5 个文件，支持跨会话持久化：

- 点击 🕐 展开下拉列表，显示 `文件名 — 父目录名` 格式
- 点击条目快速打开该文件（如有未保存变更会弹确认框）
- 已删除的文件显示为灰色
- 底部「Clear Recent Files」清除全部历史
- 空列表时按钮灰色显示

### 键盘模式

默认使用 Mac 风格快捷键，可通过工具栏 ⌨ 按钮切换为 Win 风格。

**Mac 模式（Emacs 风格导航）：**

| 快捷键 | 功能 |
|--------|------|
| `Ctrl+A` | 移动光标到行首 |
| `Ctrl+E` | 移动光标到行尾 |
| `Ctrl+B` | 光标左移 |
| `Ctrl+F` | 光标右移 |
| `Ctrl+P` | 光标上移 |
| `Ctrl+N` | 光标下移 |
| `Ctrl+D` | 删除光标后字符 |
| `Ctrl+H` | 删除光标前字符 |
| `Ctrl+K` | 删除至行尾 |
| `Ctrl+L` | 垂直居中当前行 |

**通用快捷键：**

| 快捷键 | 功能 |
|--------|------|
| `Ctrl+S` | 保存 |
| `Ctrl+Shift+S` | 另存为 |
| `Ctrl+O` | 打开文件 |
| `Ctrl+Alt+N` | 新建文件 |
| `Ctrl+Shift+N` | 新建窗口 |
| `Ctrl+A` / `Shift+Ctrl+A` | 全选（Mac 模式需加 Shift 避免与行首冲突） |
| `Ctrl+C` | 复制 |
| `Ctrl+X` | 剪切 |
| `Ctrl+V` | 粘贴 |
| `Ctrl+Z` | 撤销 |
| `Ctrl+Shift+Z` / `Ctrl+Y` | 重做 |
| `Home` / `End` | 行首 / 行尾 |
| `Ctrl+Home` / `Ctrl+End` | 文档首 / 文档尾 |
| `Enter` | 换行 |
| `Shift+Enter` | 延续列表/引用块标记 |
| `Alt+Shift+Enter` | 在容器内缩进 |
| `Tab` / `Shift+Tab` | 缩进 / 取消缩进 |

### 其他功能

- **历史文件** — 标题栏 🕐 按钮快速打开最近 5 个文件，跨会话持久化
- **智能续行** — Shift+Enter 自动延续列表、引用块等语法
- **文件监听** — 外部修改自动检测
- **光标闪烁** — 500ms 周期闪烁，输入时自动重置
- **裸 URL 检测** — 自动识别文本中的 `https://` 并转为可点击链接

## 安装

### 环境要求

- Rust 工具链（edition 2024）
- Windows 10+（当前主要支持平台）

### 构建

```bash
git clone https://github.com/WangGakin/rustmd.git
cd rustmd
cargo build --release
```

编译产物在 `target/release/rustmd.exe`，可直接运行。

### 运行

```bash
# 空白新建（默认）
cargo run

# 打开指定文件
cargo run -- --file README.md

# 命令行参数
cargo run -- --help
```

| 参数 | 说明 |
|------|------|
| `--file <路径>` | 启动时打开指定文件 |
| `--autosave` | 每次编辑自动保存（适用于 GhostText 集成） |

## 架构

```
rustmd
├── src/
│   ├── main.rs          # 应用入口，窗口创建
│   ├── lib.rs           # 库模块导出
│   ├── editor/
│   │   ├── mod.rs       # 编辑器核心逻辑（5600+ 行）
│   │   ├── ime.rs       # 中文输入法适配
│   │   ├── theme.rs     # 主题定义与颜色映射
│   │   ├── config.rs    # 编辑器配置
│   │   └── action.rs    # 编辑器动作定义
│   ├── buffer.rs        # 基于 ropey 的文本缓冲区
│   ├── line.rs          # 行渲染与光标定位
│   ├── menu.rs          # 自绘菜单栏
│   ├── title_bar.rs     # 标题栏
│   ├── status_bar.rs    # 状态栏
│   ├── window.rs        # 窗口阴影与装饰
│   ├── file_ops.rs      # 文件操作封装
│   ├── user_config.rs   # JSON 配置管理
│   ├── highlight.rs     # tree-sitter 语法高亮
│   ├── parser.rs        # Markdown 解析
│   ├── marker.rs        # 行标记系统
│   ├── cursor.rs        # 光标与选择
│   ├── inline.rs        # 行内样式检测
│   ├── paste.rs         # 粘贴处理
│   ├── key_mode.rs      # 键盘模式
│   └── tooltip.rs       # 鼠标悬停提示
├── Cargo.toml
└── EXP.md               # 开发经验记录
```

## 依赖

| 依赖 | 用途 |
|------|------|
| gpui | GPU 加速的 UI 框架 |
| ropey | 高性能文本 rope 数据结构 |
| tree-sitter | 增量解析与语法高亮 |
| rfd | 原生文件对话框 |
| open | 跨平台打开文件/目录 |

## 许可证

详见项目仓库。
