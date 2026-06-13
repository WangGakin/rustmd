# 经验文档索引

按主题拆分到 `exp/` 目录，方便 agent 按需查询。

## [IME 实现](exp/ime.md)
`grep("IME|composition|marked_range|WM_CHAR|UTF-16", path="exp")`

- 架构总览、注册时机
- WM_CHAR 与 KeyDown 双路插入
- IME 组合冲突、UTF-16/字节转换
- Buffer 越界崩溃
- 经验总结 16 条
- IME 相关修复批次

## [编辑器功能](exp/features.md)
`grep("file_ops|menu|scrollbar|CenterLine|RefCell", path="exp")`

- 文件操作（打开/保存/新建/另存为）
- 自绘菜单与客户端装饰
- 窗口拖动与红绿灯（Win32 API）
- Ctrl+L 居中
- JSON 配置与主题
- 多窗口 + 图标工具栏 + About 浮层
- 终端窗口修复
- 图标嵌入、Tooltip、高亮语言扩展
- 滚动条
- RefCell/Async Task 安全模式汇总

## [视觉行导航](exp/visual-line-navigation.md)
`grep("visual_line|wrap_offsets|visual_cross_line|soft_wrap", path="exp")`

- compute_wrap_offsets / move_in_direction_visual / visual_cross_line
- 换行点计算、相对 x 位置保持
- 边界条件与已知问题

## 搜索技巧

```powershell
# 在所有经验文件中搜索
rg "关键字" .\exp\

# 在特定主题中搜索
rg "RefCell" .\exp\features.md

# 列出所有经验文件
ls .\exp\
```
