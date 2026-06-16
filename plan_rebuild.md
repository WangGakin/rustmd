拆分 editor/mod.rs 方案
当前状态
editor/mod.rs = 5224 行 / 201KB，是编辑器问题的根源——大到 harness 的 Edit 工具反复超时。

拆分方案（4 个新子模块）
1. editor/state.rs (~1560 行)
内容：EditorState struct + 全部 impl（行 59-1618）

LineContext、TabCycleCache、AutocompleteTrigger/Suggestion/State 等 helper structs
所有纯编辑逻辑方法（insert_text、delete_backward、tab、checkbox 传播等）
依赖：仅 crate 内部纯数据类型（buffer、cursor、marker、inline、line），零 GPUI

import 需求：


text
use std::ops::Range;
use crate::buffer::Buffer;
use crate::cursor::{Cursor, Selection};
use crate::marker::{LineMarkers, MarkerKind, OrderedMarker, UnorderedMarker};
use crate::line::LineTheme;
2. editor/file_ops.rs (~350 行)
内容：Editor impl 中的文件操作方法（行 ~1864-3210 中的文件相关部分）

watch_file、reload_file
save、save_as、open_file_at、open_file、new_file
is_dirty、mark_clean、can_undo、can_redo、undo、redo
注意：项目已有 src/file_ops.rs（全局文件对话框），这个新模块是 Editor 上的文件方法。

为避免命名冲突，命名为 editor/persistence.rs（persist = 持久化操作）

依赖：需要 &mut Context<Editor> + Window

3. editor/render.rs (~560 行)
内容：渲染相关 impl（行 ~3335-4083）

compute_total_content_height
compute_scroll_offset_pixels
render_scrollbar
impl Render for Editor（主 render 方法，~560 行）
impl Focusable for Editor
依赖：需要几乎所有 Editor 状态 + GPUI render 类型

4. editor/tests.rs (~1140 行)
内容：行 4083-5224 的 #[cfg(test)] mod tests 全部

依赖：通过 use super::* 引用 EditorState

拆后 editor/mod.rs 保留的内容（~800 行）
模块声明 + pub use 重导出
顶层 imports + helper structs（SelectionDrag、ScrollbarDrag、EmptyDragView）
Editor struct 定义（行 1620-1682）
Editor impl 中的核心方法（new、with_config、start_cursor_blink、cursor/selection accessor、text/len/is_empty、set_text、sync_list_state、insert/append、on_key_down、on_modifiers_changed、detect_autocomplete、render_autocomplete、execute/handle_action 等公共 API）
AutocompleteState 等小 helper structs
实现步骤
创建 editor/state.rs：剪切 EditorState 及其全部 impl + LineContext/TabCycleCache/Autocomplete* 等辅助类型。在 mod.rs 中 mod state; pub use state::*;
创建 editor/persistence.rs：剪切 save/save_as/reload_file/watch_file/open_file_at/open_file/new_file/is_dirty/mark_clean/can_undo/can_redo/undo/redo。在 mod.rs 中 mod persistence;，这些是 Editor 的 impl 方法，用 impl Editor { include!(...) } 或直接在 persistence.rs 写 impl Editor { ... }（Rust 允许跨文件 impl 同一个 struct）
创建 editor/render.rs：剪切 Render + Focusable impl + render_scrollbar + compute_total/scroll_offset。Rust 允许跨文件 impl trait。
创建 editor/tests.rs：剪切 #[cfg(test)] mod tests。在 mod.rs 底部 #[cfg(test)] mod tests;
修正 imports：每个新文件头部加自己需要的 use 语句，mod.rs 中移除不再需要的。
验证：cargo test + cargo clippy
预期效果
editor/mod.rs：~800 行（公共 API + Editor struct + input handling）
editor/state.rs：~1560 行（纯编辑逻辑）
editor/persistence.rs：~350 行（文件操作）
editor/render.rs：~560 行（渲染）
editor/tests.rs：~1140 行（测试）
Edit 工具不再因为 200KB 文件超时！