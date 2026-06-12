# rustmd

一个基于 GPUI 框架的 Markdown 实时渲染编辑器，灵感来源于 Typora。

rustmd 基于 [writ](https://github.com/nicholasgasior/writ) 编辑器内核构建，提供所见即所得的 Markdown 编辑体验——光标离开时隐藏 `**`、`#`、`-` 等标记符号，仅显示渲染后的样式效果。

## 主要特性

### 实时渲染

Markdown 语法在光标离开时自动隐藏，直接显示加粗、标题、列表等渲染结果，实现真正的所见即所得编辑体验。

### 代码高亮

基于 tree-sitter 的代码块语法高亮，支持 Rust、Bash 等语言。

### 中文输入法支持

针对 Windows 中文输入法（微软拼音、手心输入法等）进行了深度适配，包括：

- IME 组合输入与 writ KeyDown 的双路冲突解决
- UTF-16 与 UTF-8 字节偏移的精确转换
- 中文标点全角/半角自动处理
- IME 取消时的残留文本清理

### 多窗口

支持多窗口编辑，每个窗口独立管理文档。通过工具栏按钮或 `Ctrl+Shift+N` 创建新窗口。

### 主题与字体

内置 Dracula 和 Nord 两套预设主题，支持通过 JSON 配置文件自定义完整色值。

| 平台 | 配置文件路径 |
|------|-------------|
| Windows | `%APPDATA%\rustmd\config.json` |
| macOS | `~/Library/Application Support/rustmd/config.json` |
| Linux | `~/.config/rustmd/config.json` |

### 文件操作

| 功能 | 快捷键 | 说明 |
|------|--------|------|
| 打开文件 | `Ctrl+O` | 弹出系统文件对话框 |
| 保存 | `Ctrl+S` | 保存当前文件 |
| 另存为 | `Ctrl+Shift+S` | 选择新路径保存 |
| 新建文件 | `Ctrl+Alt+N` | 清空编辑器新建文档 |
| 新建窗口 | `Ctrl+Shift+N` | 打开独立的新编辑窗口 |

### 其他功能

- **智能续行** — Shift+Enter 自动延续列表、引用块等语法
- **文件监听** — 外部修改自动检测
- **光标闪烁** — 500ms 周期闪烁，输入时自动重置
- **键盘模式切换** — 支持 Mac 风格快捷键
- **裸 URL 检测** — 自动识别文本中的 `https://` 并转为可点击链接

## 安装

### 环境要求

- Rust 工具链（edition 2024）
- Windows 10+（当前主要支持平台）

### 构建

```bash
git clone https://github.com/nicholasgasior/writ.git
cd rustmd
cargo build --release
```

> 注：项目基于 writ 编辑器内核，writ 本身嵌入在 GPUI 框架中。

### 运行

```bash
# 打开指定文件
cargo run -- --file README.md

# 演示模式
cargo run -- --demo

# 空白新建（默认）
cargo run
```

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
│   ├── diff.rs          # Diff 引擎
│   ├── git.rs           # Git 操作
│   ├── github.rs        # GitHub API 客户端
│   ├── inline.rs        # 行内样式检测
│   ├── paste.rs         # 粘贴处理
│   ├── key_mode.rs      # 键盘模式
│   └── demo.rs          # 演示内容
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
