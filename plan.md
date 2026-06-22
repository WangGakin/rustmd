# rustmd 插件系统 —— 细化实施方案

## 0. 总体架构决策

### 0.1 JS 引擎选型

`boa_engine = "0.21"`，纯 Rust、零系统依赖（无 clang/node），`cargo add boa_engine` 即用。

### 0.2 插件唯一入口对象

JS 插件激活后只获得**一个**顶层对象 `ctx`，所有能力都挂在这个对象上：

```
ctx.editor.getBuffer()          // 读 buffer
ctx.editor.getCursor()          // 读光标
ctx.editor.insertText(str)      // 写文本
ctx.editor.setStatusBar(str, persistent?)  // 显示状态栏消息
ctx.editor.moveCursor(line, col)

ctx.commands.register(name, handler)       // 注册命令
ctx.commands.registerKeybinding(keys, name) // 绑定快捷键

ctx.events.on(event, callback)             // 订阅事件
ctx.events.off(event, callback)            // 取消订阅
```

不再有 Phase 2 `editor.*` 和 Phase 3 `ctx.*` 混用的问题。Phase 2 和 Phase 3 合并为一个统一的「JS API 实现」阶段。

### 0.3 插件目录

使用 `dirs::config_dir()` 跨平台解析：

| 平台 | 路径 |
|------|------|
| Windows | `~/AppData/Roaming/rustmd/plugins/` |
| macOS | `~/Library/Application Support/rustmd/plugins/` |
| Linux | `~/.config/rustmd/plugins/` |

### 0.4 插件格式

标准格式：`plugin.json` + `main.js`（放在以插件名命名的子目录里）。

```json
// plugin.json
{
  "name": "word-count",
  "version": "1.0.0",
  "description": "Count words in current buffer",
  "author": "...",
  "main": "main.js",
  "api_version": 1
}
```

IDE 会为缺少 `plugin.json` 的孤立 `.js` 文件自动生成最小元数据（name 取自文件名、version `"0.0.0"`、description `""`、author `"unknown"`），让用户能以最快的"丢一个 js 文件"方式上手，后续需要版本管理时再补 `plugin.json`。

### 0.5 超时保护

放弃 `tokio::time::timeout`（无法中断同步 JS 执行）。改用 `boa_engine::RuntimeLimits::set_loop_iteration_limit`，设置合理的循环迭代上限（例如 `10_000_000`），超限时 Boa 引擎自身抛出 JS 错误，Rust 侧捕获后卸载该插件。配合 `recursion_limit`、`stack_size_limit` 形成三层保护。

### 0.6 键盘拦截插入点

在 `on_key_down` 中，插件快捷键拦截位于以下检查**之后**、主 key match **之前**：

```
1. input_blocked 检查 → 提前返回（IME 组合中，全键盘输入阻塞）
2. find bar 焦点路由 → 找找栏消费输入，提前返回
3. ★ 插件快捷键查找 → 命中则执行 JS 回调，返回
4. autocomplete 导航
5. Mac mode Ctrl+ 快捷键
6. 主 key match 块（backspace/left/right/enter/tab/ctrl+c/v/...）
```

这与 IME 无冲突：IME 组合期间 `input_blocked == true`，步骤 3 永远不会到达。组合结束后恢复正常派发。

### 0.7 状态栏插件消息

不修改现有 `StatusBarInfo` 结构。在 `Editor` 上新增字段：

```rust
struct PluginStatus {
    message: String,
    persistent: bool,
    since: std::time::Instant,
}
```

渲染时在状态栏右侧叠加显示。非持久消息 5 秒后自动清除；持久消息由插件调用 `ctx.editor.clearStatusBar()` 手动清除。

---

## 1. Rust 侧数据结构设计

### 1.1 模块划分

```
src/plugin/
├── mod.rs          # PluginManager: 扫描、加载、卸载、重载、查询
├── manifest.rs     # PluginManifest: plugin.json 反序列化 + 隐式补全
├── context.rs      # PluginContext: 单个插件的 boa Context 封装 + RuntimeLimits
├── api.rs          # JsEditorApi: ctx.editor.* 的 Rust 实现（闭包注册）
├── registry.rs     # CommandRegistry: 命令名→JS函数 映射、快捷键→命令名 映射
├── events.rs       # EventBus: ctx.events.on/off + Rust侧emit触发JS回调
```

### 1.2 核心数据结构

```rust
// src/plugin/manifest.rs
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub main: String,           // JS 入口文件名（相对于插件目录）
    pub api_version: u32,       // 编辑器 API 版本号
}
```

```rust
// src/plugin/context.rs
pub struct PluginContext {
    pub manifest: PluginManifest,
    pub boa: boa_engine::Context, // 独立的 JS 运行时
    pub commands: CommandRegistry,
    pub event_handlers: HashMap<String, Vec<JsFunction>>,
    pub loaded: bool,
}
```

```rust
// src/plugin/registry.rs
pub struct CommandRegistry {
    // 命令名 -> JS 可调用对象
    commands: HashMap<String, boa_engine::object::JsObject>,
    // 快捷键字符串（如 "ctrl-alt-w"） -> 命令名
    keybindings: HashMap<String, String>,
}
```

```rust
// src/plugin/mod.rs
pub struct PluginManager {
    pub plugins: Vec<PluginContext>,
    // 快捷键 -> (&PluginContext, 命令名) 的快速查找缓存
    pub keybinding_index: HashMap<String, (usize, String)>,
    plugin_dir: PathBuf,
}
```

### 1.3 Editor 侧新增字段

```rust
// 在 Editor struct 中新增：
plugin_manager: Option<Rc<RefCell<PluginManager>>>,
plugin_status: Option<PluginStatus>,
```

`PluginManager` 在编辑器创建时初始化一次，由编辑器持有。每个插件快捷键触发时，Editor 通过 `plugin_manager` 找到对应 `PluginContext` 并执行 JS。

---

## 2. 分阶段实施计划

### 阶段 0：环境验证（0.5 天）

**目标**：确认 `boa_engine = "0.21"` 在项目中可编译、可运行最小 JS。

**要做的事**：
1. `Cargo.toml` 添加 `boa_engine = "0.21"`
2. 写一个 `#[test]`：创建 `boa_engine::Context`，`eval("1 + 2")`，断言返回 `3`
3. 测试 `RuntimeLimits::set_loop_iteration_limit`：写一个 `while(true){}` 脚本，断言 Boa 抛出错误而非死循环
4. 测试 `console.log` 绑定：注册 Rust 函数作为 `console.log`，验证输出能到 `log::info!`

**验收**：`cargo test` 通过，依赖无编译问题。

**不涉及**：编辑器代码、GPUI 渲染。

---

### 阶段 1：插件加载器骨架（2-3 天）

**目标**：扫描 → 解析 manifest → 创建独立 JS Context → 加载 main.js → 生命周期管理。

**要做的事**：

1. **插件目录扫描**（`manifest.rs`）
   - 用 `dirs::config_dir()` 解析平台路径，拼接 `rustmd/plugins/`
   - 遍历一级子目录，查找 `plugin.json`
   - 对于没有 `plugin.json` 但有 `.js` 文件的目录，生成隐式 manifest

2. **manifest 解析**（`manifest.rs`）
   - `serde_json` 反序列化 `plugin.json` → `PluginManifest`
   - 验证 `name` 不为空、`main` 指向的文件存在
   - 版本比较：`api_version` 不匹配时打印 warning，仍尝试加载

3. **JS Context 创建**（`context.rs`）
   - 每个插件创建独立的 `boa_engine::Context`
   - 设置 `RuntimeLimits`（循环迭代 10M、递归 256、栈 1024*10）
   - 注入全局对象 `ctx`（目前为空壳，API 在阶段 2 填充）

4. **生命周期**（`mod.rs` → `PluginManager`）
   - `load_all()`：扫描目录 → 创建 Context → eval main.js
   - `unload(plugin_name)`：drop Context，从 index 移除
   - `reload(plugin_name)`：unload + load
   - 每个操作的结果通过 `log` 输出

**验收**：
- 启动编辑器时终端看到 `[plugin] Loaded "word-count" v1.0.0`
- 插件目录下放一个 `console.log("loaded!")` 的 js，终端能看到输出
- 插件目录下放一个语法错误的 js，终端看到解析错误但编辑器正常运行（隔离）
- 删除插件目录后重启，无 panic

**不涉及**：编辑器 API 暴露、键盘拦截、事件系统。完全自包含的新模块。

---

### 阶段 2：统一 JS API 实现（3-4 天，原 Phase 2+3 合并）

**目标**：`ctx.editor.*` + `ctx.commands.*` + 键盘拦截一次到位，让第一个"字数统计"插件跑通。

#### 2.1 Rust 侧 API 注册（`api.rs`）

在 `PluginContext::new()` 中，向 Boa Context 注册全局对象 `ctx`：

```
ctx
├── .editor
│   ├── .getBuffer()          → String                       只读副本
│   ├── .getCursor()          → { line: u32, col: u32 }     1-indexed
│   ├── .insertText(String)   → void                         通过 EditorAction 队列安全写入
│   ├── .setStatusBar(String, persistent?) → void           默认 false（5s 超时）
│   ├── .clearStatusBar()     → void                         手动清除持久消息
│   └── .moveCursor(line, col)→ void                         1-indexed，触发滚动
├── .commands
│   ├── .register(String name, Function handler) → void
│   └── .registerKeybinding(String keys, String commandName) → void
└── .events
    ├── .on(String event, Function callback)    → void
    └── .off(String event, Function callback)   → void
```

注册方式（示例）：

```rust
// api.rs
pub fn register_editor_api(context: &mut boa_engine::Context, editor_handle: ...) {
    let editor_obj = boa_engine::object::JsObject::default();
    
    // getBuffer
    let get_buffer = boa_engine::NativeFunction::from_fn(|_this, _args, _ctx| {
        // 通过 editor_handle 拿到 buffer 的只读副本
        Ok(JsValue::String(buffer_text.into()))
    });
    editor_obj.set("getBuffer", get_buffer, ...);
    
    // ... 其他方法同理 ...
    
    // 顶层 ctx 对象
    let ctx_obj = ...;
    ctx_obj.set("editor", editor_obj, ...);
    context.register_global_property("ctx", ctx_obj, ...);
}
```

**关键安全点**：
- `getBuffer()` 返回 `String`（Rust 侧 clone）—— 插件拿不到 `&mut`
- `insertText()` 不是直接操作 buffer，而是通过 `mpsc::Sender<EditorAction>` 发到编辑器主线程，编辑器在下一帧执行。这样所有写入都经过 `Editor::execute()` 的统一路径，undo/redo 正常工作
- `moveCursor()` 同理，通过 action 队列派发

#### 2.2 命令注册与快捷键（`registry.rs`）

```rust
impl CommandRegistry {
    fn register(&mut self, name: &str, handler: JsObject) { ... }
    fn register_keybinding(&mut self, keys: &str, command_name: &str) { ... }
    fn find_by_keystroke(&self, keystroke: &str) -> Option<(&str, &JsObject)> { ... }
}
```

快捷键字符串规范：
- 小写，`ctrl-` / `alt-` / `shift-` 前缀，字母键用单个字符
- 示例：`"ctrl-alt-w"`, `"ctrl-shift-t"`, `"f5"`
- 在 `PluginManager` 中维护一个 `HashMap<String, (plugin_index, command_name)>`，O(1) 查找

#### 2.3 键盘拦截（修改 `on_key_down`）

在 `Editor::on_key_down` 中，插入位置（行 939 之后、行 950 之前）：

```rust
// --- 新增：插件快捷键拦截 ---
if let Some(ref pm) = self.plugin_manager {
    let ks = format_keystroke(&event.keystroke);
    if let Some((plugin_idx, cmd_name)) = pm.keybinding_index.get(&ks) {
        if let Ok(mut pm) = pm.try_borrow_mut() {
            if let Some(ctx) = pm.plugins.get_mut(*plugin_idx) {
                let result = ctx.execute_command(cmd_name);
                if let Err(e) = result {
                    log::error!("Plugin command '{}' failed: {}", cmd_name, e);
                }
            }
        }
        cx.notify();
        return; // 插件消费了该按键
    }
}
// --- 新增结束 ---
```

`format_keystroke` 辅助函数：将 GPUI 的 `Keystroke` 转为 `"ctrl-alt-w"` 格式字符串。

#### 2.4 状态栏消息渲染

在 `RootView::render()` 中（行 560），`status_bar(...)` 调用之前：
- 从 `editor.status_info()` 之外，额外读取 `editor.plugin_status()`
- 传给 status_bar 渲染函数一个新的可选参数
- 渲染为一个独立的 `div`，右对齐，半透明背景，叠加在状态栏右侧

非持久消息在 Editor 的 `render()` 中检查 `since.elapsed() > 5s` → `self.plugin_status = None`

**验收**：
- 写 `word-count` 插件（约 15 行 JS），按 `Ctrl+Alt+W`，状态栏显示 `Word count: 123`
- 写 `timestamp` 插件，按 `Ctrl+Shift+T`，光标处插入当前时间字符串
- 两个插件同时加载，快捷键互不干扰
- IME 输入中文时按 Ctrl+Alt+W 不会误触发插件（因为 `input_blocked` 先行返回）
- `while(true){}` 插件不卡死编辑器

---

### 阶段 3：事件系统（1-2 天，原 Phase 4）

**目标**：插件能响应编辑器生命周期事件。

#### 3.1 事件定义

```rust
// src/plugin/events.rs
pub enum PluginEvent {
    FileOpen { path: PathBuf },
    FileSave { path: PathBuf },
    FileClose { path: PathBuf },
    BufferChanged,   // 每次编辑后触发（有节流，每 500ms 最多一次）
    EditorReady,     // 编辑器初始化完成，插件加载后立即触发
}
```

#### 3.2 JS 侧 API

```js
ctx.events.on('file-save', (payload) => {
    ctx.editor.setStatusBar(`Saved: ${payload.path}`);
});
```

`payload` 是 JSON 对象，Rust 侧在 emit 时将 `PluginEvent` 序列化为 `JsValue` 传给回调。

#### 3.3 Rust 侧 emit 点

- `FileOpen` → `Editor::open_file()` / `Editor::new()` 完成时
- `FileSave` → `Editor::save()` 成功后
- `FileClose` → window close 前
- `BufferChanged` → `Editor::render()` 中，距上次 emit > 500ms 才触发（避免频繁调用 JS）

#### 3.4 实现要点

`EventBus` 结构：
```rust
pub struct EventBus {
    handlers: HashMap<String, Vec<boa_engine::object::JsObject>>,
}
```

`PluginManager::emit_event(name, payload_json)`:
1. 遍历所有已加载插件
2. 找到该事件的回调列表
3. 依次调用 JS 函数，每个 try-catch，任一崩溃不影响后续

**验收**：
- 保存文件后状态栏显示 `Saved: C:\...\note.md`
- 事件回调中的 JS 错误不导致插件卸载（仅 log error）
- 快速连续编辑 10 次，BufferChanged 只触发 1-2 次

---

### 阶段 4：打磨与工具（可后续迭代）

- 命令面板：列出所有已安装插件及命令（Ctrl+Shift+P 风格）
- 插件市场/安装引导
- 热重载：文件监控 → 自动 reload
- 插件沙箱：考虑为每个插件创建独立目录隔离（e.g. `~/AppData/Local/rustmd/plugins/<name>/data/`）

---

## 3. 工程化保障

### 3.1 超时/死循环保护

```rust
// context.rs — 每个 PluginContext 创建时
let mut limits = RuntimeLimits::default();
limits.set_loop_iteration_limit(10_000_000);
limits.set_recursion_limit(256);
limits.set_stack_size_limit(1024 * 10);
boa_context.set_runtime_limits(limits);
```

### 3.2 错误隔离

```rust
fn execute_command(&mut self, cmd_name: &str) -> Result<(), String> {
    let handler = self.commands.get(cmd_name).ok_or("command not found")?;
    // Boa 内部已处理 JS 异常，这里捕获 Rust 侧 panic
    match std::panic::catch_unwind(AssertUnwindSafe(|| {
        handler.call(&JsValue::Undefined, &[], &mut self.boa)
    })) {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(js_err)) => Err(format!("JS error: {}", js_err)),
        Err(_) => Err("plugin panicked".into()),
    }
}
```

### 3.3 内存控制

每次命令执行后调用 `self.boa.run_gc()`。Boa 0.21 提供 `Context::run_gc()`。

### 3.4 安全边界

| 能力 | 策略 |
|------|------|
| `getBuffer()` | 返回 String clone，不可变 |
| `insertText(s)` | 通过 `mpsc::Sender<EditorAction>` 异步投递，主线程下一帧执行 |
| `moveCursor(l,c)` | 同上，通过 action 队列 |
| `setStatusBar(s)` | 仅写入 `Editor::plugin_status` 字段 |
| 文件系统 | 不暴露给插件。如有需求后续加 `ctx.fs` 并限制沙箱路径 |
| 网络 | 不暴露 |
| `console.log` | 重定向到 Rust `log::info!` |

---

## 4. 插件 API 速查表（最终形态）

```js
// === ctx.editor ===
ctx.editor.getBuffer()                      // → String
ctx.editor.getCursor()                      // → { line: u32, col: u32 }
ctx.editor.insertText(str)                  // → void
ctx.editor.setStatusBar(str, persistent?)   // → void
ctx.editor.clearStatusBar()                 // → void
ctx.editor.moveCursor(line, col)            // → void

// === ctx.commands ===
ctx.commands.register(name, handler)           // → void
ctx.commands.registerKeybinding(keys, name)    // → void

// === ctx.events ===
ctx.events.on(event, callback)    // event: 'file-open'|'file-save'|'file-close'|'buffer-changed'|'editor-ready'
ctx.events.off(event, callback)   // → void
```

---

## 5. 快捷键字符串规范

格式：`modifier-modifier-key`，全小写。

| 修饰键 | 写法 |
|--------|------|
| Ctrl | `ctrl` |
| Alt | `alt` |
| Shift | `shift` |
| Meta/Win | `meta` |

按键名：
- 字母：`a`-`z`
- 数字：`0`-`9`
- 功能键：`f1`-`f12`
- 特殊键：`escape`, `enter`, `tab`, `space`, `backspace`, `delete`, `home`, `end`, `pageup`, `pagedown`, `up`, `down`, `left`, `right`

示例：`"ctrl-alt-w"`, `"ctrl-shift-t"`, `"alt-enter"`, `"f5"`

GPUI Keystroke 到该格式的转换由 `format_keystroke(&Keystroke) -> String` 完成。

---

## 6. 目录结构（最终）

```
rustmd/
├── src/
│   ├── editor/          # 现有编辑器核心（不改动结构，仅 on_key_down 加一行调用 + 新增字段）
│   │   ├── mod.rs       # 新增：plugin_manager 字段、on_key_down 拦截点
│   │   ├── action.rs    # 可能新增：PluginAction 相关变体（如 insertText 用的 action）
│   │   └── ...
│   ├── plugin/          # 新增插件模块
│   │   ├── mod.rs       # PluginManager: 扫描、加载、卸载、快捷键索引
│   │   ├── manifest.rs  # PluginManifest 结构 + 反序列化 + 隐式补全
│   │   ├── context.rs   # PluginContext: 每个插件的独立 boa Context + RuntimeLimits
│   │   ├── api.rs       # ctx.editor.* / ctx.commands.* 的 Rust 闭包实现
│   │   ├── registry.rs  # CommandRegistry: 命令存储 + 快捷键→命令映射
│   │   └── events.rs    # EventBus: on/off/emit, PluginEvent 枚举
│   └── main.rs
├── Cargo.toml           # + boa_engine = "0.21"
└── (用户) ~/AppData/Roaming/rustmd/plugins/
    ├── word-count/
    │   ├── plugin.json
    │   └── main.js
    └── timestamp/
        └── main.js      # 隐式插件（无 plugin.json，自动补全）
```
