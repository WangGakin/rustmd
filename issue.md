# ISSUE

打开编译的软件后，除了软件窗口，还会弹出一个**终端**界面，显示：[rustmd] config: "C:\\Users\\Benai\\AppData\\Roaming\\rustmd\\config.json"。如果关掉终端会导致主程序一并关闭，这个问题是什么原因，该怎么改正？

之前进行主题风格讨论时，LLM声称写了Dracula和nord两个主题，但因为本来就没计划做显式切换入口，所以这部分代码可能需要简化，**先确认是否真的内置了两套主题的代码**，然后我们仅保留config这一个主题设置入口就行，为了方便小白用户，需要给config.json增加注释。

编译的软件没有图标。

看到有一个demo.rs，这个文件做什么的？

---

## 修改初步方案

### 1. 终端窗口问题（弹出终端界面，关闭终端会导致主程序关闭）

**原因分析：**
- 在`src/user_config.rs:137`有`eprintln!("[rustmd] config: {:?}", path);`，会将配置路径输出到标准错误
- 程序缺少`#![windows_subsystem = "windows"]`属性，导致Windows将程序识别为控制台应用程序
- 控制台窗口和GUI窗口属于同一进程，关闭控制台会终止整个进程

**对策：**
1. 在`src/main.rs`顶部添加属性：`#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]`
2. 将`eprintln!`改为条件输出，仅在调试模式下显示：
   ```rust
   #[cfg(debug_assertions)]
   eprintln!("[rustmd] config: {:?}", path);
   ```

### 2. 主题代码确认

**确认结果：**
- 代码中确实内置了两套主题：`dracula()`和`nord()`（`src/user_config.rs:48-78`）
- 默认使用`dracula()`主题（`src/user_config.rs:39`）
- `src/editor/theme.rs`中也有`dracula()`实现，但`nord()`主题仅在`user_config.rs`中定义

**对策：**
- 保留两套主题代码，但简化配置入口
- 给`config.json`增加注释，说明如何切换主题
- 可考虑在`theme.rs`中添加`nord()`实现以保持一致性

### 3. 图标问题（编译的软件没有图标）

**原因分析：**
- 项目中没有.ico图标文件
- Cargo.toml中没有配置图标相关的设置
- 缺少Windows资源文件（.rc）

**对策：**
1. 准备一个.ico图标文件
2. 在Cargo.toml中添加图标配置（需要使用`embed-resource`或类似crate）
3. 或使用`winresource` crate自动处理Windows资源

### 4. demo.rs文件作用

**功能说明：**
- `demo.rs`是一个演示脚本，用于展示编辑器的功能
- 包含一系列`DemoStep`（输入文本、执行动作、等待）
- 可通过`cargo run -- --demo`命令运行演示模式
- 演示内容包括：标题、列表、代码块、链接等Markdown元素

---

**注意：** 以上为初步方案，后续还需要细化具体实现步骤、测试验证等。
