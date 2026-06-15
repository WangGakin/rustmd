RustMD 性能 + 风格全量重构计划
分支
refactor/perf-style（基于 master）

依赖变更
Cargo.toml: 新增 log = "0.4" + env_logger = "0.11"（替换 22 处 eprintln，让 release 模式有日志）
main.rs 启动时初始化 logger
分阶段执行（每阶段后跑 cargo test 验证 259 个测试全绿）
阶段 1 — 低风险风格清理（安全网先建好）
cargo clippy --fix 自动修 10 条（collapsible if、 needless clone ×3、Option::map、sort_by_key 等）
手动修剩余 3 条（too_many_arguments 留到阶段 3 一起处理）
删 dbg! 残留（parser.rs:383）
删死代码 Config::validate（config.rs:62，体里只有 Ok(self)）
命名修正：toggle_checkbox_for_test → toggle_checkbox_state（mod.rs:1304，非测试专用）
lock().unwrap() 容错化（user_config.rs:193/206/215，防 mutex 毒化连锁炸 UI）
抽常量：150ms debounce / 500ms 光标闪烁 / 100ms 文件轮询 / 窗口尺寸 集中到 config 模块
阶段 2 — eprintln → log
22 处 eprintln! 改 log::error! / log::warn!（highlight.rs ×9 语言配置失败、mod.rs ×6 文件/watcher/save、user_config.rs:159）
测试内的诊断 println!/eprintln!（marker.rs/parser.rs 调试 dump）保留但加 #[ignore] 标记或 eprintln!→log::debug!
阶段 3 — Line::new 18 参数 → builder 结构体
新增 LineParams 结构体（line.rs），含全部 18 字段
Line::new(params: LineParams) + 保留字段语义不变
唯一调用点 mod.rs:3750 改用 LineParams { ... } 构造（消除 clippy too_many_arguments）
line.rs 的 7 个测试不直接构造 Line（用别的方式），验证不受影响
阶段 4 — IME utf16 增量缓存（P0 核心）
BufferContent 新增字段 utf16_cache: Option<(u64, Vec<u32>)>（version → utf16 前缀和数组）
新方法 byte_to_utf16_cached(offset) -> usize 和 utf16_to_byte_cached(utf16) -> usize：首次按版本构建 O(n)，同版本查询 O(log n) 二分
在 apply_edit 末尾置 cache 为 None（失效，下次重建）
6 处 IME 调用点（ime.rs:33/41/51/146/157/191）+ 2 处 reload（mod.rs:1974 比对）改用新 API，删除 self.state.buffer.text() 全量拷贝
保留 utf16 边界语义不变，cursor.rs 的现有测试覆盖正确性
阶段 5 — parse_continuation 渲染热路径消除（P0）
ParsedNodes（marker.rs）新增字段 blockquote_markers: HashMap<usize, Vec<Marker>>（按行起始 byte 缓存引用行 markers）
collect_node_infos 在遍历时同步填充该 map（一次解析覆盖所有引用行）
markers_at_from_infos（marker.rs:953）对 block_quote_marker/block_continuation 节点改成查 map，不再调 parse_continuation
parse_continuation 降级为 fallback / 测试用
marker.rs 的 73 个测试 + buffer.rs 的 40 个测试覆盖正确性
阶段 6 — update_caches / normalize 二次解析消除（P1）
normalize_ordered_lists（buffer.rs:300）发现有修正时，用 tree-sitter 增量编辑通知（InputEdit）而非 parse_rope(&text, None) 全量重解析
审视 update_caches（buffer.rs:225）能否用 changed_ranges 缩小 collect_node_infos / extract_all_inline_styles 的范围（保守处理，若复杂度过高则保留全量但在大文档场景加版本号短路）
buffer.rs 的 40 个测试 + inline.rs 的 10 个测试覆盖
阶段 7 — recent_files 缓存 + render 副作用移除（P1）
RootView（main.rs:169）新增 recent_files_cache: Vec<String>，render 内不再调 user_config::recent_files()
改成在 ToggleRecentFiles / OpenRecentFile / ClearRecentFiles / OpenFile 等事件回调里刷新缓存（main.rs 已有的 on_action 处理点）
user_config::add_recent_file / clear_recent_files 写文件部分可选移到后台线程（用 smol，项目已依赖）避免主线程 I/O
阶段 8 — 其它性能小项（P2）
reload_file（mod.rs:1955）：先用长度 + mtime 快速判定，再决定是否 text() 比对
detect_naked_urls_in_range（mod.rs:1789）：改用 rope slice 的 as_str() 零拷贝路径，减少 per-line String 分配
build_styled_content（line.rs:776）：hidden_ranges/style_ranges 预排序，windows(2) 内用双指针替代 .iter().any()
验证
每阶段结束跑 cargo test（259 测试）+ cargo clippy --all-targets（目标 0 warning）。 最后跑一次 release build cargo build --release 确认 windows_subsystem 正常。

风险与回退
阶段 4/5/6 改动核心热路径，是回归风险集中点 → 每阶段独立 commit，出问题可单阶段 revert
阶段 1/2/3 风险低，先做建立信心
全程不修改测试逻辑本身（只动被测函数的实现/签名），测试失败=实现回归