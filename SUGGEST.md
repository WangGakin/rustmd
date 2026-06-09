# GPUI + writ 中文 IME 实现建议（更新版）

## 一、已解决问题回顾

根据 EXP.md 第十节改进记录，以下问题已全部修复：

### 2026-06-09（第一批）

| 问题 | 修复方案 | 效果 |
|------|----------|------|
| 空格无法输入 | `on_key_down` 增加 `"space"` 分支 | ✓ 已解决 |
| IME 结束后 backspace 失效 | `unmark_text` 末尾增加 `cx.notify()` | ✓ 已解决 |
| 中文标点无法输入 | 收窄丢弃逻辑：仅丢弃单 ASCII，非 ASCII 直接插入 | ✓ 已解决 |

### 2026-06-09（第二批）

| 问题 | 修复文件 | 修复摘要 | 效果 |
|------|----------|----------|------|
| 中文模式下标点双打 `,，` | `src/editor/ime.rs` | 插入非 ASCII 前检测光标前一字节，若为 ASCII 标点则先删除 | ✓ 已解决 |
| 中文退格光标消失/闪退 | `src/buffer.rs`, `src/editor/mod.rs` | 新增 `prev_char_boundary`/`next_char_boundary` | ✓ 已解决 |
| 光标从不闪烁 | `src/editor/mod.rs` | 500ms 定时器切换可见性 + 输入时 `reset_cursor_blink()` | ✓ 已解决 |
| 输入 `~~~~` 闪退（byte_at panic） | `src/buffer.rs` | `byte_at` 改用 char-based 安全访问 | ✓ 已解决 |

---

## 二、新增经验总结（EXP.md 第八节 10-12 条）

| 序号 | 经验 | 关键点 |
|------|------|--------|
| 10 | backspace 不能按字节删 | 多字节字符必须用 `prev_char_boundary` 找完整边界，`cursor_pos - 1` 只删 1 字节 → UTF-8 断裂 → ropey panic |
| 11 | byte_at 不能假设 char 对齐 | ropey 的 `byte_slice` 要求两端对齐 char 边界，落入多字节字符中间时 panic |
| 12 | IME 中文标点双路重叠 | `on_key_down` 先插入 ASCII 版，IME handler 后收到全角版，需检测并替换 |

---

## 三、当前遗留问题分析与建议

### 问题：markdown 语法输入时实时渲染-编辑状态反复切换导致闪烁

**现象：**

输入 markdown 语法（如 `**bold**`）时，文本框反复闪烁，编辑状态和渲染状态频繁转换。

**根因分析：**

通过对照 writ 原始代码（`C:\Users\Benai\writ-main\src\editor\mod.rs:3606-3654`）：

```rust
fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    // ... 其他逻辑 ...

    // 问题所在：每次 render 都执行重量级操作
    let (github_matches_by_line, naked_urls_by_line) =
        self.detect_links(first_visible_line, last_visible_line + 1);
    self.spawn_github_validation(&github_matches_by_line, cx);
    self.spawn_naked_url_validation(&naked_urls_by_line, cx);
}
```

**触发链路：**

```
光标 blink 定时器 (500ms)
  → cx.notify()
  → 触发 render
  → detect_links()  // 扫描可见行，解析 markdown
  → spawn_github_validation()  // 异步 HTTP 请求
  → 文本框重新渲染
  → 闪烁
```

用户已尝试修复：用 `content_changed` 守卫限制重量级操作，但仍有问题。

**深层问题：**

writ 的实时渲染特性会在输入时立即渲染 markdown 语法（如 `**` → 粗体），这与编辑状态冲突。每次光标 blink 都会触发重新解析和渲染，导致视觉上的反复切换。

**解决方案建议：**

方案 A：延迟渲染 — 编辑结束后才渲染 markdown

```rust
fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    // 判断是否处于编辑状态（光标在当前区域内）
    let is_editing = self.focus_handle.is_focused(cx);

    if !is_editing {
        // 光标移走后才执行重量级渲染
        let (github_matches, urls) = self.detect_links(first_visible_line, last_visible_line + 1);
        self.spawn_github_validation(&github_matches, cx);
    }

    // 光标 blink 不会触发 detect_links
    // ...
}
```

方案 B：内容变化守卫 + 版本号比对

```rust
fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let buffer_version = self.state.buffer.version();

    // 仅在内容真正变化时执行重量级操作
    if buffer_version != self.last_detect_version {
        self.last_detect_version = buffer_version;
        let (github_matches, urls) = self.detect_links(first_visible_line, last_visible_line + 1);
        self.spawn_github_validation(&github_matches, cx);
    }

    // ...
}
```

方案 C：分离编辑模式和渲染模式

```rust
pub struct Editor {
    edit_mode: bool,  // true = 编辑模式，false = 渲染模式
}

fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    if self.edit_mode {
        // 编辑模式：显示原始 markdown 文本，不渲染样式
        return self.render_raw_text();
    } else {
        // 渲染模式：显示渲染后的 markdown
        return self.render_markdown();
    }
}

fn on_focus_lost(&mut self, cx: &mut Context<Self>) {
    self.edit_mode = false;  // 光标移走后切换到渲染模式
    cx.notify();
}

fn on_focus_gained(&mut self, cx: &mut Context<Self>) {
    self.edit_mode = true;   // 光标进入时切换到编辑模式
    cx.notify();
}
```

**推荐方案：A + B 组合**

1. 用版本号比对（方案 B）防止光标 blink 触发重量级操作
2. 用 `is_focused` 判断（方案 A）在编辑时暂停渲染

---

## 四、后续优先级

| 优先级 | 问题 | 影响 | 建议 |
|--------|------|------|------|
| P0 | markdown 实时渲染闪烁 | 编辑体验受损 | 方案 A + B 组合 |
| P2 | UTF-16 转换模块化 | 可维护性 | 长期优化 |