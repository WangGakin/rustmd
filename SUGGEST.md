# RUA 信息按钮

## 需求

标题栏最左侧增加 "RUA" 按钮，点击弹出浮层显示软件信息，并提供一个打开 config 目录的链接。

## 改动文件

| 文件 | 改动 |
|------|------|
| `src/title_bar.rs` | 标题栏左端添加 "RUA" text button + `about_open: bool` 状态 |
| `src/title_bar.rs` 或 `src/menu.rs` | About 浮层渲染（`if this.about_open { ... }`） |
| 无新依赖 | `open` crate 已存在，`env!("CARGO_PKG_VERSION")` 编译时宏 |

## 交互流程

```
标题栏: [RUA] [File] [Edit] [View]
         ~~~~
         on_mouse_down → toggle about_open

┌─────────────────────────────┐
│  rustmd v0.1.1              │
│                             │
│  作者: ...                  │
│  感谢: ...                  │
│                             │
│  [Open Config Directory →]  │
│                             │
│  点击外部任意位置关闭        │
└─────────────────────────────┘
```

## 按钮 + 状态（title_bar.rs）

`TitleBar` 结构体新增 `about_open: bool`，默认 `false`。

渲染时 `[RUA]` 放在菜单按钮之前，`on_mouse_down` 切换 `about_open`：

```rust
div()
    .cursor_pointer()
    .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, _cx| {
        this.about_open = !this.about_open;
        window.refresh();
    }))
    .child("RUA")
```

## 浮层

在 `TitleBar` 的 render 中，当 `about_open` 为 true 时，用 `anchored()` 定位在按钮下方：

```rust
if this.about_open {
    anchored()
        .snap_to_under()
        .child(
            div()
                .p_4()
                .bg(theme.background)
                .border_1()
                .border_color(theme.comment)
                .child(div().child(format!("rustmd v{}", env!("CARGO_PKG_VERSION"))))
                .child(div().child("作者: ..."))
                .child(div().child("感谢: ..."))
                .child(
                    div()
                        .cursor_pointer()
                        .on_mouse_down(cx.listener(|_, _, _, cx| {
                            let path = rustmd::user_config::config_path();
                            if let Some(parent) = path.parent() {
                                let _ = open::that(parent);
                            }
                        }))
                        .child("Open Config Directory →")
                )
        )
}
```

## 关闭方式

在 `RootView` 或 `Editor` 的 `on_mouse_down` 中点击外部区域时设置 `about_open = false`：

```rust
// 复用 MenuBarState 的 close_menu 模式或直接在 capture_any_mouse_down 中处理
let state = cx.global_mut::<MenuBarState>();
state.close_menu();
// 同时关闭 about
this.about_open = false;
```
