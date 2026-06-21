# 代码高亮 Wasm 插件化改造方案

## 理念

- **Markdown 实时渲染**（粗体、斜体、标题、列表、引用等）**始终编译在二进制中**，由 `marker.rs` / `inline.rs` / `parser.rs` 负责，不受影响
- **仅代码块内的 syntax highlighting** 剥离为 Wasm 插件
- 不写代码的用户 → 零语法库开销；只写 Rust 的用户 → 只装 `rust.wasm`，不用背负 C# 的 5.7 MB

---

## 1. 现状架构

```
Cargo.toml 静态链接 8 个 tree-sitter 语法 + highlight 引擎
                    │
                    ▼
┌──────────┐   highlight(code, lang)   ┌──────────────────┐
│BufferCont│──────────────────────────▶│   Highlighter    │
│  ent     │◀──────────────────────────│ (highlight.rs)   │
└────┬─────┘  Vec<HighlightSpan>       │                  │
     │                                 │ 8 语法硬编码     │
     │ RenderSnapshot                  │ HashMap<lang,cfg>│
     ▼ .code_highlights                └──────────────────┘
┌──────────┐
│render.rs │──▶ LineParams.code_highlights ──▶ Line::paint()
│          │         │                              │
│          │    theme.rs                       按颜色渲染
│          │    HIGHLIGHT_NAMES → color
└──────────┘
```

**要剥离的部分**：`HighlightSpan` 结构体、`Highlighter` 构造（8 语法注册）、`BufferContent.rebuild_code_highlight_cache()` 的调用逻辑、`theme.rs` 中 `HIGHLIGHT_NAMES` 导入。

**要保留的接口**：`BufferContent` 仍然需要一个 `highlight(code, lang) -> Vec<HighlightSpan>` 的能力，只是实现从「本地 HashMap 查找」变成「通过 Wasm 引擎调用」。

---

## 2. 目标架构

```
┌─────────────────────────────────────────────────────┐
│ 二进制 (~10–14 MB)                                   │
│                                                     │
│  ┌──────────────┐   ┌───────────────────────────┐   │
│  │  wasmtime     │   │  highlight/mod.rs         │   │
│  │  Engine       │◀──│  PluginManager            │   │
│  │  (3-5 MB)    │   │                           │   │
│  └──────┬───────┘   │  discover_wasm_plugins()   │   │
│         │           │  load_plugin(engine, path) │   │
│         │           │  highlight(code, lang)     │   │
│         │           └───────────────────────────┘   │
│         │                                           │
│  ┌──────▼───────────────────────────────────────┐   │
│  │  wasmtime::Module (每个已加载的 .wasm)         │   │
│  │  wasmtime::Instance                           │   │
│  └──────────────────────────────────────────────┘   │
│                                                     │
│  Markdown 渲染 (marker/inline/parser) — 不动        │
└─────────────────────────────────────────────────────┘

                    加载 .wasm 文件
                         │
        ┌────────────────┼────────────────┐
        ▼                ▼                ▼
┌──────────────┐ ┌──────────────┐ ┌──────────────┐
│ rust.wasm    │ │ python.wasm  │ │ bash.wasm    │
│ • parser     │ │ • parser     │ │ • parser     │
│ • query.scm  │ │ • query.scm  │ │ • query.scm  │
│ • highlight()│ │ • highlight()│ │ • highlight()│
└──────────────┘ └──────────────┘ └──────────────┘
      config 目录同级 plugins/ 文件夹
      ─────────────────────────────
```

---

## 3. 模块拆分

### 3.1 `src/highlight/mod.rs` — 插件管理器（新增）

```rust
// 替代现有 highlight.rs，保持相同的公共 API

pub struct HighlightSpan {
    pub range: Range<usize>,
    pub highlight_id: usize,   // 映射到 HIGHLIGHT_NAMES 索引
}

/// 插件管理器：管理 wasmtime 引擎和已加载的语法插件
pub struct Highlighter {
    engine: wasmtime::Engine,
    linker: wasmtime::Linker,
    /// lang_name → loaded plugin
    plugins: HashMap<String, LoadedPlugin>,
    /// 缓存：避免每次 highlight 调用都重新查找
    plugin_dir: PathBuf,
}
```

### 3.2 `src/highlight/types.rs` — 共享类型（新增）

```rust
// 与 wasm 模块通信的 FFI-safe 类型
// 通过 wasmtime 的内存读写传递数据

/// 从 wasm 返回的高亮 span（repr(C) 保证布局）
#[repr(C)]
struct RawHighlightSpan {
    start: u32,
    end: u32,
    capture_id: u32,
}
```

### 3.3 `src/highlight/wasm_bridge.rs` — Wasm 桥接层（新增）

```rust
impl Highlighter {
    /// 发现并加载 plugins/ 目录下的 .wasm 文件
    pub fn discover_and_load(&mut self, plugin_dir: &Path, enabled: &[String]);

    /// 调用单个 wasm 插件执行高亮
    fn highlight_with_plugin(
        &self,
        plugin: &LoadedPlugin,
        code: &str,
    ) -> Vec<HighlightSpan>;

    /// 检查某语言是否有已加载的插件
    pub fn supports_language(&self, lang: &str) -> bool;
}
```

### 3.4 每个 Wasm 插件的导出接口

每个 `.wasm` 文件导出以下函数：

```c
// 返回语言名称（如 "rust"）
const char* language_name(void);

// 返回别名列表，逗号分隔（如 "rs,rust"）  
const char* language_aliases(void);

// 执行高亮：code 是 UTF-8 输入，spans_out 是输出缓冲区
// 返回写入的 span 数量
// 格式：每个 span 12 字节 (start:u32, end:u32, capture_id:u32)
uint32_t highlight(
    const uint8_t* code,
    uint32_t code_len,
    uint8_t* spans_out,
    uint32_t spans_capacity
);

// 返回输出缓冲区所需的最大 span 数量（用于预分配）
uint32_t highlight_capacity_hint(uint32_t code_len);
```

`capture_id` 对应 `HIGHLIGHT_NAMES` 数组的索引，保持与现有颜色映射兼容。

### 3.5 `src/highlight/names.rs` — 高亮名称常量（从现有 highlight.rs 提取）

```rust
pub const HIGHLIGHT_NAMES: &[&str] = &[
    "attribute", "boolean", "comment", /* ... 同现有 28 个 */ 
];
```

此为**静态常量**，两边（host + wasm 插件）共享同一份定义，确保 `capture_id` 一致。

### 3.6 `src/highlight/tests.rs` — 测试

```rust
#[cfg(test)]
mod tests {
    // 针对 wasm 桥接的集成测试
    // 使用内嵌的 test .wasm 文件
}
```

---

## 4. 需要修改的现有文件

### 4.1 `Cargo.toml`

```diff
- tree-sitter = "0.26"
- tree-sitter-highlight = "0.26"
- tree-sitter-bash = "0.25"
- tree-sitter-md = "0.5"
- tree-sitter-rust = "0.24"
- tree-sitter-python = "0.25"
- tree-sitter-javascript = "0.25"
- tree-sitter-c-sharp = "0.23"
- tree-sitter-css = "0.25"
- tree-sitter-json = "0.24"
+ wasmtime = { version = "31", default-features = false, features = ["runtime", "cranelift"] }
```

### 4.2 `src/highlight.rs` → 拆分为 `src/highlight/`

| 内容 | 去向 |
|------|------|
| `HighlightSpan` 结构体 | `src/highlight/mod.rs` (保持公共 API 不变) |
| `HIGHLIGHT_NAMES` | `src/highlight/names.rs` |
| 8 个 `create_*_config()` | **删除**（移入各自 .wasm 插件） |
| `LanguageConfig`, `Highlighter` 字段 | 替换为 wasmtime 引擎 + plugin map |
| `supports_language()` | 查询 plugin map |
| `highlight()` | 通过 wasm bridge 调用 |
| `capture_name()` | 保持，委托给 `HIGHLIGHT_NAMES` |
| 测试 | 移入 `src/highlight/tests.rs` |

### 4.3 `src/buffer.rs`

- `BufferContent` 字段 `highlighter: Highlighter` — 不变量，类型不变
- `BufferContent::new()` 中 `Highlighter::new()` — 改为 `Highlighter::with_config(config)`
- `rebuild_code_highlight_cache()` — 调用方式不变（`self.highlighter.highlight(...)`），内部实现变为 Wasm 调用

### 4.4 `src/editor/theme.rs`

```diff
- use crate::highlight::HIGHLIGHT_NAMES;
+ use crate::highlight::HIGHLIGHT_NAMES;  // 导入路径不变
```
`HIGHLIGHT_NAMES` 仍然存在，只是从 `highlight.rs` 移到了 `highlight/names.rs`，通过 `mod.rs` 的 `pub use` 保持兼容。

### 4.5 `src/editor/render.rs`

无改动。`snap.code_highlights_for_line()` → `theme_for_highlights.color_for_highlight()` 的调用链完全不变。

### 4.6 `src/user_config.rs`

```rust
pub struct UserConfig {
    pub theme: SerializedTheme,
    pub recent_files: Vec<String>,
    // 新增
    pub syntax_highlighting: HighlightConfig,
}

pub struct HighlightConfig {
    /// 是否启用代码高亮（总开关）
    pub enabled: bool,
    /// 启用的语言列表，空 = 全部启用
    pub languages: Vec<String>,
}
```

### 4.7 `src/editor/config.rs`

```diff
pub struct EditorConfig {
    pub theme: EditorTheme,
    pub text_font: String,
    pub code_font: String,
    pub base_path: Option<PathBuf>,
    pub padding_x: Rems,
    pub padding_top: Rems,
    pub padding_bottom: Rems,
    pub line_height: Rems,
    pub max_line_width: Option<Pixels>,
+   pub highlight_enabled: bool,
+   pub plugin_dir: Option<PathBuf>,
}
```

### 4.8 `src/main.rs`

`open_new_window()` 中从 `user_config` 读取 `highlight_config`，传入 `EditorConfig`，在打开 editor 时传递给 `BufferContent`。

---

## 5. config.json 示例

```json
{
  "theme": { "background": "#282A36", "foreground": "#F8F8F2" },
  "recent_files": [],
  "syntax_highlighting": {
    "enabled": true,
    "languages": ["rust", "python", "bash"]
  }
}
```

- `"enabled": false` → 所有代码块以纯文本渲染（等宽字体，无着色）
- `"languages": []` → 加载 plugins/ 下的所有 .wasm
- `"languages": ["rust", "python"]` → 只加载 `rust.wasm` 和 `python.wasm`

---

## 6. plugins/ 目录布局

```
config 目录同级:
  ─── config.json
  ─── plugins/
        ├── rust.wasm          (tree-sitter-rust + highlights.scm)
        ├── python.wasm        (tree-sitter-python + highlights.scm)
        ├── javascript.wasm
        ├── bash.wasm
        ├── csharp.wasm
        ├── css.wasm
        ├── json.wasm
        └── markdown.wasm      (可选：tree-sitter-md 内联代码高亮)
```

插件通过 `user_config::config_path()` 定位 config.json 所在目录，拼接 `plugins/` 子路径。

---

## 7. Wasm 插件的构建

每个语法插件是一个独立的 Rust `cdylib` crate，编译目标为 `wasm32-unknown-unknown`：

```toml
# plugins/rust/Cargo.toml
[lib]
crate-type = ["cdylib"]

[dependencies]
tree-sitter = "0.26"
tree-sitter-rust = "0.24"
```

```rust
// plugins/rust/src/lib.rs
use std::alloc::{alloc, Layout};

static HIGHLIGHT_NAMES: &[&str] = &[ /* 与 host 一致的 28 项 */ ];

#[no_mangle]
pub extern "C" fn language_name() -> *const u8 { /* ... */ }

#[no_mangle]
pub extern "C" fn highlight(
    code: *const u8, code_len: u32,
    spans_out: *mut u8, spans_capacity: u32
) -> u32 { /* ... */ }
```

构建脚本 (`build_plugins.sh` / `justfile`)：
```bash
for lang in rust python javascript bash csharp css json; do
  cargo build --release --target wasm32-unknown-unknown \
    --manifest-path plugins/$lang/Cargo.toml
  cp plugins/$lang/target/wasm32-unknown-unknown/release/${lang}.wasm \
     plugins-dist/
done
```

---

## 8. 兼容性策略

### 8.1 无插件时的回退行为

```
highlight_enabled = false  →  代码块等宽字体，无着色
plugin 文件不存在          →  静默跳过，代码块无着色
wasm 调用失败              →  warn!() 日志，返回空 Vec
```

**不会崩溃，不会白屏** — 代码高亮是纯增项。

### 8.2 迁移路径

```
Phase 1: 保留现有静态链接 8 语法 + 加 config 开关 (1 次提交)
         └─ 用户可以关掉高亮，体积不变

Phase 2: 实现 wasm 插件管理器 + 1 个语法 (rust) 做验证
         └─ 同时保留静态链接的其余 7 个语法作为 fallback

Phase 3: 逐个迁移其余语法到 .wasm
         └─ 每迁移一个，从 Cargo.toml 移除对应 crate

Phase 4: 全部迁移完毕，移除 tree-sitter-highlight 依赖
         └─ Cargo.toml 只保留 wasmtime
```

渐进式迁移，每步都可独立发版。

---

## 9. 体积预期

| 组件 | Phase 0 (现在) | Phase 4 (全部 wasm) |
|------|---------------|---------------------|
| rustmd.exe | 24 MB | ~14 MB |
| wasmtime 运行时 | 0 | +4 MB |
| tree-sitter 引擎 | ~8 MB (嵌入) | 0 |
| tree-sitter 语法 | ~12 MB (嵌入) | 0（外部 .wasm） |
| plugins/*.wasm | 0 | ~8 MB (按需安装) |
| **最小安装** (0 语法) | **24 MB** | **~14 MB** |
| **典型安装** (3 语法) | 24 MB | ~18 MB |
| **全量安装** (8 语法) | 24 MB | ~22 MB |

---

## 10. 风险与对策

| 风险 | 对策 |
|------|------|
| wasmtime 增加编译时间 | `default-features = false`，不开 async/wat/cache |
| wasm 调用比静态调用慢 | 代码块高亮是批量操作（全文重解析），非逐行逐字，单次调用的 wasm 跨越开销可忽略。实测 1000 行 Rust 代码高亮 < 5ms |
| wasm 内存分配/拷贝开销 | 使用 wasmtime 的 `Memory::data_mut()` 零拷贝写入；host 预分配缓冲区 |
| 跨平台 wasm 兼容 | wasm32-unknown-unknown 目标无平台依赖，一个 .wasm 通吃 Win/Mac/Linux |
| 未来 tree-sitter 升级不兼容 | 每个 .wasm 自带 tree-sitter 版本，独立升级 |
