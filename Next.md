# RustMD 重构进度总结 — 交接文档

> 分支: `refactor/perf-style` (基于 master)  
> 基线: 274 tests, 0 clippy warnings  
> 日期: 2026-06-15

---

## 一、已完成工作（已提交）

### Stage 1: 低风险风格清理 (commit 6fdeea5)
- `cargo clippy --fix` 自动修 10 条警告
- `sort_by` → `sort_unstable_by`
- 移除 `dbg!` (parser.rs:383)
- 移除死代码 `Config::validate` + 未用 `anyhow::Result` import
- 重命名 `toggle_checkbox_for_test` → `toggle_checkbox_state` (8 处)
- `Mutex::lock().unwrap()` → `.unwrap_or_else(|e| e.into_inner())` (user_config.rs ×3)
- 提取常量: `AUTOCOMPLETE_DEBOUNCE_MS=150`, `CURSOR_BLINK_MS=500`, `FILE_WATCHER_POLL_MS=100`, `DEFAULT_WIN_WIDTH=900.0`, `DEFAULT_WIN_HEIGHT=700.0`

### Stage 2: eprintln → log (commit 0f414c1)
- Cargo.toml 添加 `log = "0.4"`
- editor/mod.rs: 7× eprintln→log::error, 1×→log::warn, 6×→log::debug
- highlight.rs: 7× eprintln→log::warn, 添加 `use log::warn;`
- user_config.rs: 1× eprintln→log::debug
- 修复 main.rs 中注释的乱码字符
- **注意**: `env_logger` 依赖尚未添加（当时网络不可用），需要在 Cargo.toml 添加 `env_logger = "0.11"` 并在 main.rs 添加 `env_logger::init();`

### Stage 3: Line::new 18参数 → LineParams (commit 1a250a6)
- `src/line.rs` 新增 `pub struct LineParams` 含 18 个字段
- `Line::new(params: LineParams)` 新签名
- 更新唯一切点 `editor/mod.rs:3748` 的 `build_line` 闭包
- 0 clippy warnings

### editor/state.rs 提取 (commit 36b2dcf)
- 从 `editor/mod.rs` 提取 EditorState struct + impl (~1567 行) 到 `src/editor/state.rs`
- 包含: LineContext, TabCycleCache, AutocompleteTrigger/Suggestion/State, 及所有 EditorState 方法
- state.rs 零 GPUI 依赖 — 纯编辑逻辑
- `cursor_in_code_block` 和 `delete_selection` 改为 `pub(crate)`
- mod.rs 从 ~5222 行减至 ~3666 行

---

## 二、当前代码结构

```
src/
  buffer.rs       (1308 行) - Buffer + Rope + tree-sitter + 缓存
  config.rs       (73 行)   - CLI 配置 + 常量
  cursor.rs       (389 行)  - 光标/选择
  file_ops.rs     (91 行)   - 文件对话框
  highlight.rs    (454 行)  - 语法高亮
  inline.rs       (543 行)  - 行内样式提取
  key_mode.rs     (27 行)
  lib.rs          (58 行)
  line.rs         (2151 行) - 行渲染 + LineParams
  main.rs         (453 行)  - 入口 + RootView 渲染
  marker.rs       (2359 行) - 行标记 + parse_continuation 热路径
  menu.rs         (124 行)
  parser.rs       (438 行)  - Markdown 解析器
  paste.rs        (189 行)
  status_bar.rs   (404 行)
  title_bar.rs    (144 行)
  tooltip.rs      (20 行)
  user_config.rs  (239 行)  - 用户配置 + recent_files mutex
  window.rs       (185 行)
  editor/
    action.rs     (63 行)   - 编辑器 action 定义
    config.rs     (56 行)   - EditorConfig
    ime.rs        (254 行)  - IME 输入处理 (P0 优化目标)
    mod.rs        (3666 行) - 主编辑器模块 (仍需继续拆分)
    state.rs      (1567 行) - EditorState (已提取)
    theme.rs      (93 行)   - EditorTheme
```

---

## 三、待完成工作

### ── 模块拆分（继续拆 editor/mod.rs）──

#### 1. 提取 `editor/render.rs`

从 `editor/mod.rs` 提取以下内容到 `editor/render.rs`:

**需要提取的 impl 块:**
- `impl Render for Editor` — 整个 trait 实现（在 mod.rs 中搜索 `impl Render for Editor`）
- `impl Focusable for Editor` — 整个 trait 实现

**需要提取的辅助方法（放到 `impl Editor` 块中，Rust 允许跨文件拆分 impl 块）:**
- `render_scrollbar` — 滚动条渲染
- `compute_total_content_height` — 计算内容总高度
- `compute_scroll_offset_pixels` — 计算滚动偏移像素
- `render_snapshot` — 渲染快照
- `render_autocomplete` — 自动补全渲染

**操作步骤:**
1. 在 `editor/mod.rs` 中找到 `impl Render for Editor { ... }` 和 `impl Focusable for Editor { ... }` 的完整块（从 `impl` 到对应的闭合 `}`），剪切到 `editor/render.rs`
2. 把 `render_scrollbar`、`compute_total_content_height`、`compute_scroll_offset_pixels`、`render_snapshot`、`render_autocomplete` 这些方法从 `impl Editor { }` 块中剪切出来，放到 `editor/render.rs` 中的新 `impl Editor { }` 块里
3. 在 `editor/mod.rs` 顶部添加 `mod render;`
4. 在 `editor/render.rs` 中添加必要的 `use` 语句（`use super::*;` + 其他缺失的 import）
5. 运行 `cargo check` 修复编译错误
6. 运行 `cargo test` 确认 274 测试全过
7. `cargo clippy` 确认 0 warnings

**⚠️ 注意事项:**
- Rust 允许同一类型在不同文件中有多个 `impl Editor {}` 块
- 不要把 `impl Editor {}` 主块拆断！只能从完整的闭合块中提取方法
- 需要仔细跟踪花括号匹配，确保每个 impl 块都正确闭合
- **不要使用 Python 做文件 I/O**（在 Windows/CRLF 环境下会静默失败）
- 使用 PowerShell `.ps1` 脚本文件做文件操作（见下方"环境约束"）

#### 2. 提取 `editor/persistence.rs`

从 `editor/mod.rs` 的 `impl Editor {}` 块中提取文件操作方法到 `editor/persistence.rs`:

**需要提取的方法（行号参考，可能因 render.rs 提取而偏移）:**
- `watch_file` (原 ~308 行)
- `reload_file` (原 ~398 行)
- `save` (原 ~1549 行)
- `save_as` (原 ~1569 行)
- `open_file_at` (原 ~1599 行)
- `open_file` (原 ~1623 行)
- `new_file` (原 ~1639 行)
- `is_dirty` (原 ~1539 行)
- `mark_clean` (原 ~1544 行)
- `can_undo` (原 ~1658 行)
- `can_redo` (原 ~1663 行)
- `undo` (原 ~1668 行)
- `redo` (原 ~1676 行)

**操作步骤同上**，放到 `editor/persistence.rs` 的 `impl Editor {}` 块中。

---

### ── 性能优化（原始 P0-P2 计划）──

#### 阶段4: IME utf16 增量缓存 (P0 核心)

**问题:** `src/editor/ime.rs` 中有 6 处调用 `self.state.buffer.text()` 做 Rope→String 全量拷贝，然后线性扫描 UTF-16 偏移转换：
- 第 33 行: `selected_text_range` → `byte_to_utf16`
- 第 41 行: `marked_text_range` → `byte_to_utf16`
- 第 51 行: `text_for_range` → `byte_to_utf16` + `utf16_to_byte`
- 第 146 行: `replace_text_in_range` → `utf16_to_byte`
- 第 157 行: `replace_and_mark_text_in_range` → `byte_to_utf16` / `utf16_to_byte`
- 第 191 行: `unmark_text` → `buffer.text()`

**方案:** 在 Buffer 中添加 UTF-16 缓存:
```rust
// src/buffer.rs - 在 BufferContent 中添加:
utf16_cache: Option<(u64, Vec<u32>)>,  // (version, cumulative_utf16_offsets)
```

- 懒构建: 首次需要时计算 `Vec<u32>` (每行一个累计 UTF-16 偏移)
- 版本失效: Buffer 每次编辑 `version += 1`，缓存检测到 version 不匹配则重建
- 查找: 二分搜索 `Vec<u32>` → O(log n) 替代 O(n) 线性扫描
- ime.rs 改为调用 `self.state.buffer.utf16_offset(byte_pos)` 而不是 `buffer.text()` + `byte_to_utf16`

**具体步骤:**
1. 在 `src/buffer.rs` 的 `BufferContent` struct 添加 `utf16_cache` 字段
2. 实现 `BufferContent::ensure_utf16_cache(&mut self)` — 根据 version 判断是否重建
3. 实现 `Buffer::utf16_offset_from_byte(&mut self, byte_offset: usize) -> usize` — 二分查找
4. 实现 `Buffer::byte_offset_from_utf16(&mut self, utf16_offset: usize) -> usize` — 二分查找
5. 修改 `src/editor/ime.rs` 中 6 处调用，替换为新的 Buffer 方法
6. 删除 ime.rs 中的 `byte_to_utf16` 和 `utf16_to_byte` 自由函数
7. 运行 `cargo test` + 手动测试 IME 中文/日文/韩文输入

#### 阶段5: parse_continuation 渲染热路径消除 (P0)

**问题:** `src/marker.rs:953` 在渲染热路径中调用 `parse_continuation`，该函数运行完整 markdown 解析器:
```rust
// marker.rs:953
markers.extend(parse_continuation(rope, start, end));
```
每帧渲染 blockquote 的每一行都会触发，严重影响性能。

**方案:** 预计算 `parse_continuation` 结果，存入 `ParsedNodes`:
1. 在 `ParsedNodes` (marker.rs) 中新增字段存储 blockquote continuation 的预计算标记
2. 在 `collect_node_infos` 阶段预计算所有 continuation 标记
3. 渲染路径直接从缓存读取，不再调用 `parse_continuation`
4. 修改 `parse_markers_at_line` 使用预计算结果

#### 阶段6: normalize_ordered_lists 二次解析消除 (P1)

**问题:** `src/buffer.rs:300` `normalize_ordered_lists()` 在需要修正列表编号时，直接修改 rope 后重新 `self.parser.parse_rope(...)` 做完整重解析。

**方案:** 
- 收集所有修正后批量应用，然后只做一次重解析（当前已经是这样做的）
- 更进一步: 直接修改 tree-sitter tree 中的节点范围，避免重解析
- 或者: 用更轻量的方式验证编号正确性，避免不必要的 normalize 调用

#### 阶段7: recent_files 渲染副作用移除 (P1)

**问题:** `src/main.rs` 的 render() 方法中有 3 处调用 `rustmd::user_config::recent_files()`:
- 第 186 行: `self.file_info.recent_files = rustmd::user_config::recent_files();`
- 第 253 行: `let files = rustmd::user_config::recent_files();`
- 第 369 行: `let files = rustmd::user_config::recent_files();`

每次渲染都会 Mutex lock + Vec clone，在热路径中。

**方案:**
1. 在 RootView 中添加 `recent_files: Rc<RefCell<Vec<String>>>` 字段
2. 文件打开/保存时更新此缓存
3. render() 中直接读取缓存，不锁 Mutex
4. 或者用 GPUI Model 包装 recent_files，通过 observe 追踪变化

#### 阶段8: 其它性能小项

- `update_caches` 增量更新: `src/buffer.rs:225` 每次编辑遍历整个 AST，可改为增量更新
- 考虑将 `RenderSnapshot` 从 `Rc<RefCell<...>>` 改为更高效的结构
- 检查 `build_line` 闭包中是否有不必要的分配

---

## 四、环境约束（重要！）

| 约束 | 说明 |
|------|------|
| **平台** | Windows + cmd.exe，CRLF 行尾 |
| **Python 文件 I/O** | **完全不可用** — `open().write()` 静默失败，不要用 Python |
| **PowerShell 内联操作** | `Replace()` 在 CRLF 文件上经常匹配失败，不要用内联 `-replace` |
| **PowerShell .ps1 脚本** | **可靠** — 用 `[System.IO.File]::ReadAllLines()` + `WriteAllText()` |
| **Edit 工具** | 大文件 (>150KB) 可能报 "file has been modified since read"，state.rs 提取后 mod.rs ~130KB 应该可控 |
| **网络** | 之前不可用，可能现在已恢复。需添加 `env_logger = "0.11"` 依赖 |
| **测试** | `cargo test` — 274 个测试是安全网，每次改动后都要跑 |

**可靠的文件操作模式:**
```powershell
# 创建 .ps1 脚本文件
$script = @'
$src = "C:\Users\Benai\rustmd\src\editor\mod.rs"
$lines = [System.IO.File]::ReadAllLines($src)
# ... 处理 $lines ...
$out = $lines -join "`r`n"
[System.IO.File]::WriteAllText($src, $out, [System.Text.Encoding]::UTF8)
'@
[System.IO.File]::WriteAllText("C:\Users\Benai\rustmd\_work.ps1", $script, [System.Text.Encoding]::UTF8)
powershell -ExecutionPolicy Bypass -File "C:\Users\Benai\rustmd\_work.ps1"
```

---

## 五、失败经验（避免重蹈覆辙）

1. **tests.rs 提取失败** — 测试模块嵌套复杂，`use super::*` 作用域问题，建议不要提取测试
2. **render.rs 提取失败** — 上次从 `impl Editor {}` 块中间截取方法导致花括号不匹配。正确做法: 只提取完整的 `impl Trait for Editor` 块，或者把方法提取到新的 `impl Editor {}` 块
3. **Python 文件写入静默失败** — 绝对不要用 Python 做文件 I/O
4. **PowerShell 内联 Replace 不可靠** — CRLF 行尾导致字符串匹配失败，用 .ps1 脚本文件

---

## 六、Git 状态

```
Branch: refactor/perf-style
Commits:
  36b2dcf refactor: extract EditorState into editor/state.rs
  1a250a6 refactor: stage 3 - Line::new 18 params -> LineParams struct
  0ce754a chore: remove stray debug file
  0f414c1 refactor: stage 2 - eprintln to log crate
  6fdeea5 refactor: stage 1 - low-risk style cleanup

Working tree: CLEAN
```

---

## 七、推荐执行顺序

1. ✅ ~~Stage 1-3: 风格清理~~ (已完成)
2. ✅ ~~state.rs 提取~~ (已完成)
3. 🔲 **render.rs 提取** — 下一步
4. 🔲 **persistence.rs 提取**
5. 🔲 **添加 env_logger 依赖** (网络恢复后)
6. 🔲 **阶段4: IME utf16 增量缓存** (P0)
7. 🔲 **阶段5: parse_continuation 热路径消除** (P0)
8. 🔲 **阶段6: normalize_ordered_lists 优化** (P1)
9. 🔲 **阶段7: recent_files 渲染优化** (P1)
10. 🔲 **阶段8: 其它小项**
11. 🔲 **最终验证: cargo test + clippy + release build**
