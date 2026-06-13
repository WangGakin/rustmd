# 视觉行导航（软换行 Up/Down）

## 背景

编辑器已有 GPUI `StyledText` 的自动软换行（`flex_1().min_w_0()` 容器中自动 wrap），但 `Cursor::move_up/move_down` 始终按 buffer 行跳转。Mac 模式下 `Ctrl+P/N` 和上下箭头键需要按**视觉行**（soft-wrapped line）移动，编辑长段落更方便。

## 架构

```
on_key_down (arrow / Ctrl+P/N)
  → move_in_direction_visual(Up/Down)
      ├─ compute_wrap_offsets()           ← shape_line 计算换行点
      ├─ shaped.x_for_index()             ← 确定光标在视觉行中的相对 x 位置
      ├─ 同 buffer 行内移动                ← offset_at_x 定位到相邻视觉行
      └─ visual_cross_line()              ← 跨 buffer 行边界
           ├─ compute_wrap_offsets()       ← 目标行的换行点
           └─ shaped.index_for_x()         ← 保持 visual_x 进入目标行
```

## 关键实现

### `compute_wrap_offsets(text, available_width, font, font_size) → Vec<usize>`
- 用 `window.text_system().shape_line()` 整形一行文本
- 通过 `shaped.width` 检测是否换行
- 用 `shaped.index_for_x(start_x + available_width)` 二分定位换行点
- 换行点是行内 byte offset（从行首计算）

### `move_in_direction_visual(direction, extend, window)`
- Left/Right 直接走原有 `move_in_direction`
- Up/Down：
  1. 取 `buffer.slice_cow(line_range)` 获得行文本
  2. `compute_wrap_offsets` 算出换行点
  3. `position(|&o| o > cursor_in_line)` 确定当前在第几视觉行
  4. `x_for_index(cursor_in_line) - x_for_index(row_start)` 计算相对 x（相对于视觉行首，非行绝对位置）
  5. 同 buffer 行：`shaped.index_for_x(row_start_x + relative_x)` 找到新位置
  6. 跨行：调用 `visual_cross_line`

### `visual_cross_line(target_line, visual_x, ..., from_end) → Option<Cursor>`
- 根据 `from_end` 选择目标行的第一个或最后一个视觉行
- `target_x = shaped.x_for_index(row_start) + visual_x`
- 用 `index_for_x(target_x)` 定位，然后 clamp 到行范围

## 边界条件

| 场景 | 行为 |
|------|------|
| 文档首行按 Up | 回退到 `Cursor::move_up`（到文档顶不动） |
| 文档末行按 Down | 回退到 `Cursor::move_down`（到文档底不动） |
| 不换行的短行 | `wrap_offsets` 为空 → `visual_cross_line` 进入目标行首 |
| 空行 | `visual_cross_line` 检测 target_text 为空，直接返回行首 |
| 含 marker 的行（列表/引用） | `available_width` 未减去 marker 宽度，换行点略微偏移 |
| code block 行 | 始终用 `text_font` 而非 `code_font`，换行点有微小偏差 |
| Shift+方向键 | 通过 `extend` 参数传递，`move_cursor(new_cursor, extend)` 处理选择扩展 |
| 窗口缩放 | 每次按键重新 `shape_line`，天然适配新宽度 |

## 已知问题

- **"invalid text run" panic**：发生在 `StyledText::with_runs()`（渲染管线），是 `display_text` 与 `runs` 长度不一致导致的预存 bug，新光标位置可能触发。修复需排查 `line.rs` 的 `build_styled_content` 中 runs 映射逻辑。
- **双次 shape**：同一行文本 `shape_line` 两遍（`compute_wrap_offsets` 内 + 手动）。单行成本极低，可忽略。
- **noop glyph positions**：当 `available_width` 极窄（< 1 个字符宽），`compute_wrap_offsets` 返回空，视觉导航退化为 buffer 行跳转。

## 涉及文件

| 文件 | 改动 |
|------|------|
| `src/editor/mod.rs` | 新增 `TextRun`/`SharedString`/`Font` 导入；新增 `compute_wrap_offsets` 静态方法；新增 `move_in_direction_visual` 方法；新增 `visual_cross_line` 方法；修改 `on_key_down` 用 `window` 替代 `_window`；箭头键和 Ctrl+P/N 改为 `move_in_direction_visual` |

**代码量：** ~150 行（`src/editor/mod.rs` 单个文件）
