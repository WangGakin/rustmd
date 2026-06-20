# Rime 输入法在 GPUI 编辑器中的适配

> 项目：rustmd（基于 GPUI 0.2.2 的 Markdown 编辑器）
> IME：Rime 小狼毫 weasel 0.17.4
> 日期：2026-06-20

---

## 背景

rustmd 此前已完成微软拼音和手心输入法的 IME 适配。核心架构是 `on_key_down` 不插入可打印字符，统一走 `replace_text_in_range`（WM_CHAR）和 `replace_and_mark_text_in_range`（WM_IME_COMPOSITION）。

Rime 适配遇到三个 bug，根因是同一个：**Rime 的 IME 回调先于键事件到达，导致 `ime_marked_range` 在键事件处理前被清空**。

---

## 根因：事件时序倒置

Rime 的 `ImeProcessKey` 流程：

```
1. Windows 调用 ImeProcessKey(VK_SPACE)
2. Rime 后端处理空格 → 提交中文
3. _EndComposition → _AddIMEMessage(WM_IME_COMPOSITION, GCS_COMP|GCS_RESULTSTR)
4. ImeProcessKey 返回 TRUE
5. Windows 生成消息：WM_IME_COMPOSITION → WM_KEYDOWN（如果返回 FALSE）
```

关键：`WM_IME_COMPOSITION` 在步骤 3 入队，步骤 5 生成的 `WM_KEYDOWN` 紧随其后。GPUI 处理消息时**先处理 WM_IME_COMPOSITION，后处理 WM_KEYDOWN**。

GPUI 的 `handle_keydown_msg` 流程：

```rust
fn handle_keydown_msg(...) {
    let is_composing = input_handler.marked_text_range().is_some();
    if is_composing {
        translate_message(handle, wparam, lparam); // 路由到 IME
        return;
    }
    // 派发到 on_key_down
}
```

正常流程（微软拼音）：
1. 拼音阶段：`replace_and_mark_text_in_range("nihao")` → `ime_marked_range = Some(..)` → `marked_text_range()` 返回真实范围 → GPUI 路由键到 IME ✅

2. 提交阶段：空格被 IME 完全消费，不产生 `WM_KEYDOWN` → `on_key_down` 不被调用 ✅

Rime 的异常流程：
1. 拼音阶段：同正常流程 ✅
2. 提交阶段：
   - `WM_IME_COMPOSITION` 先到 → `replace_text_in_range("你好")` → 取走 `ime_marked_range` → `ime_marked_range = None`
   - `WM_KEYDOWN` 后到 → `marked_text_range()` 返回 `None` → GPUI 派发到 `on_key_down` → 空格插入 ❌

---

## 修复方案

**核心思路**：在 `ime_marked_range` 被取走后，提供一个短暂的合成范围，让 GPUI 继续将键事件路由到 IME。

### 新增字段

```rust
pub(crate) ime_composing: Cell<bool>,
pub(crate) last_ime_activity: Cell<Option<Instant>>,
```

### 在三个关键路径设置标志

```rust
// 1. replace_text_in_range 确认分支（CJK 提交）
self.ime_composing.set(true);
self.last_ime_activity.set(Some(Instant::now()));

// 2. replace_text_in_range 空字符串分支（composition 清空）
self.ime_composing.set(true);
self.last_ime_activity.set(Some(Instant::now()));

// 3. replace_and_mark_text_in_range 空字符串分支（composition 清空）
self.ime_composing.set(true);
self.last_ime_activity.set(Some(Instant::now()));
```

### marked_text_range 新增分支

```rust
fn marked_text_range(&self, ...) -> Option<Range<usize>> {
    let mark = if let Some(ref mark) = self.ime_marked_range {
        mark.clone()  // 真实范围（拼音阶段）
    } else if self.ime_composing.get() {
        // 合成范围（提交/清空后 50ms 窗口）
        if let Some(last) = self.last_ime_activity.get() {
            if last.elapsed().as_millis() >= 50 {
                self.ime_composing.set(false);
                return None;  // 超时，恢复正常
            }
        }
        self.state.cursor().offset..self.state.cursor().offset  // 零宽合成范围
    } else {
        return None;  // 无组合
    };
    // ... UTF-16 转换 ...
}
```

### 改动范围

全部在 `src/editor/ime.rs` 和 `src/editor/mod.rs` 中，约 30 行。`on_key_down` 完全不动。

---

## 为什么不影响其他输入法

| 输入法 | 提交键 | 是否产生 WM_KEYDOWN | 影响 |
|--------|--------|---------------------|------|
| 微软拼音 | `ImeProcessKey` 返回 TRUE | 否 | 50ms 窗口内无键事件 → 透明 |
| 微信输入法 | `ImeProcessKey` 返回 TRUE | 否 | 同上 |
| 手心输入法 | 走 `replace_text_in_range`（无 marked_range） | 进入 No composition 分支 | ime_composing 不在此路径设置 → 无影响 |
| Rime | `ImeProcessKey` 返回 TRUE，但 WM_IME_COMPOSITION 先于 WM_KEYDOWN | 是（时序问题） | 50ms 合成范围拦截延迟键 ✅ |

---

## 状态转换图

```
                 ┌─────────────────────┐
                 │  正常输入            │
                 │  ime_marked_range   │
                 │  = Some(拼音)       │
                 └────────┬────────────┘
                          │ 提交/清空
                          ▼
                 ┌─────────────────────┐
                 │  过渡窗口 (50ms)     │
                 │  ime_marked_range   │
                 │  = None             │
                 │  ime_composing=true │
                 │  → 合成零宽范围     │
                 └────────┬────────────┘
                          │ 50ms 超时
                          ▼
                 ┌─────────────────────┐
                 │  恢复正常            │
                 │  ime_composing=false│
                 │  → marked_range=None│
                 └─────────────────────┘
```

---

## 尝试过的弯路

### 弯路 1：全局 `on_key_down` 拦截退格/空格

使用 `suppress_next_backspace` / `ime_just_committed_text` 等布尔标志在 `on_key_down` 中拦截特定键。

**为什么失败**：标志是"粘性"的——composition 清空时设置的标志会残留到后续无关操作，误吞其他输入法的正常退格和空格。

### 弯路 2：动态超时

提交后 3 秒、清空后 500ms 的双超时策略。

**为什么失败**：超时过长导致 `marked_text_range` 持续返回合成范围，所有键被劫持到 IME。3 秒锁定期用户无法接受。过短则不够覆盖 Rime 的事件延迟。

### 弯路 3：Windows IME API 检测

使用 `ImmGetCompositionStringW` / `ImmGetOpenStatus` 检测 composition 状态。

**为什么失败**：`ImmGetOpenStatus` 在 composition 清空后仍返回 true，无法区分"正在组合"和"IME 打开但无组合"。

### 弯路 4：`last_commit_key` 回吞机制

在 `on_key_down` 记录空格/回车，在 `replace_text_in_range` 中反向吞掉。

**为什么失败**：对 Rime 无效——Rime 的 CJK 文本先于键到达，`last_commit_key` 还未设置。

---

## 核心教训

1. **利用 GPUI 已有的 `is_composing` 检查**——`marked_text_range()` 是 GPUI 判定的唯一入口，不引入额外的 `on_key_down` 守卫
2. **`Cell<T>` 是 IME 状态的正确容器**——`marked_text_range(&self)` 是不可变借用，`Cell` 提供内部可变性
3. **极短超时是安全的**——50ms 对人类无感知，对事件间延迟足够，对其他 IME 透明
4. **文件日志是调试 IME 的唯一手段**——Windows GUI 子系统无控制台，用 `ime_debug.log` 追踪事件时序
