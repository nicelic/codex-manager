1. 最高原则和使用边界

Gortex 是对其 track 仓库代码定位、源码精读、关系分析、数据流追踪、影响评估、编辑、重构、验证、审查与记忆的权威工具。

全流程原生 MCP 绝对闭环约束：已 track 仓库的一切操作（包括检索、分析、精读、评估、编辑、替换、写入、重构与删除），必须且仅能通过 Gortex 原生 MCP 句柄闭环执行。
严禁读写割裂：严禁阅读时使用 Gortex、修改时退回宿主原生工具。严禁对已 track 仓库调用宿主工具（包括 replace_file_content、write_to_file、view_file、grep_search、find_by_name 等）。
不可降级与熔断通知：若 Gortex 原生工具无法使用、报错或无法满足需求，严禁擅自退回原生工具自行兜底，必须立即停止操作并向用户说明原因。未 track 的项目不得擅自进行 track 图谱化。
全局技能只读豁免：read_file 放行全局技能只读（~/.agents/skills/ 及 Pi 包生态）。
严禁凭记忆捏造：严禁伪造未返回的 symbol、关系、view、索引状态或安全结论。
关键实现体完整阅读与 Token 瘦身：在 read_file、get_symbol_source、get_editing_context 中使用 compress_bodies=true（存根化削减 60%~70% Token，keep="f1,f2" 保留核心函数，支持 MQL/Go/TS），配合 format="gcx" 再削减约 27%。UTF-16 源文件 read_file 自动解码呈现（带 utf16_decoded 标记），写工具硬拦截拒绝写入。
唯一法定写契约：edit_file、batch_edit、write_file、edit_symbol 是唯一法定落地途径。严禁绕过 Gortex 状态机与图谱同步。

1.1 当前运行环境核心工具集与协议规范

本环境采用 Core 扁平离散工具预设（暴露 explore, read_file, get_symbol_source, search_symbols, search_text, edit_file, batch_edit 等 65 个热核心与延迟工具）。调用时参数扁平传入，严格遵守以下法定协议：
1. 纯净 ToolName 铁律：MCP 工具命令名永远为纯小写下划线标识符（如 read_file）。严禁拼接带句点、带斜杠或带操作派发的虚构工具名。
2. 真实参数值契约：
   - 符号 id：格式为 "文件相对路径::符号名"（如 "pkg/foo.go::Bar"），严禁使用尖括号等占位符号。路径分隔符统一使用正斜杠。
   - 文件 path：必须传入具体工程相对路径或绝对路径，严禁使用尖括号等占位符号。
3. analyze 鉴别参铁律：analyze 工具必须传 kind="..."（如 analyze(kind="architecture")），严禁传 operation="..." 导致静默退化为 help。
4. 延迟目录发现机制（Deferred Catalog）：winnow_symbols, context_closure, plan_turn, safe_delete_symbol, get_cfg, trace_path 等冷目录工具若初始未暴露，通过 tools_search(query="...") 动态唤醒。
5. 系统固定参数（fixed_arguments）——调用时严禁覆盖：
   search_symbols 固定 assist="off"；change_contract 固定 ack=false；simulate_chain 固定 keep=false；surface_memories 固定 mark_accessed=false；overlay_merge 默认 to_disk=false（落盘时显式传 to_disk=true）。
6. 统一 Output 响应控制（顶层扁平传入）：max_bytes（限制字节）、limit/cursor（分页）、format（推荐 "gcx"，削减约 27% Token）、fields（稀疏列）、scope（限定范围，如 scope="unstaged"）。search_text、find_declaration、graph_query 统一支持 _truncated_by_limit 截断与 count_is_exact:false 披露。

2. 任务开启与探索流转

流转准则与核心工具：
1. 明确文件名首读：用户明确指定文件时，首个动作直接调用 read_file(path="path/to/file.ext")，勿触发定位。
2. 代码定位与探索：未知位置或排障时优先 explore(task="...", path="...", token_budget=1600)。遵循 completion.required_action 状态机：
   - answer_ready：直接从 completion.final_response 结案并停用工具。
   - needs_exact_read：补齐 read_file 或 get_symbol_source 精读。
   - needs_more_context：缩小 task 范围或指定 path 追加线索。
3. 智能上下文装配：优先 smart_context(task="...", entry_point="...", fidelity="graded", token_budget=8000) 装配最小完备上下文与多文件编辑规划。
4. 架构速览与拓扑辅助：get_repo_outline() 获取工程语言分布、入口、热点与顶层大纲；context_closure(symbols, files) 计算拓扑闭包；plan_turn(task) 推荐开局工具序列；gortex_wakeup() 生成约 500 Token 架构摘要。

2.1 检索、阅读与图谱协同
search_text(query, regexp=false, limit=100, path, repo)：Trigram 全文搜索，命中附带 symbol_id 与 symbol_name。path 过滤前置于 limit 截断执行且自动归一化路由；若返回 _truncated_by_limit=true，count 仅为当前范围下界 floor，可追加 path 收窄或扩大 limit/细化 query 恢复完整结果。
search_symbols(query, kind, flavor, path)：BM25 驼峰分词检索 AST 定义，固定 assist=off。
search_ast(pattern) 或 search_ast(detector)：[热] 语法级代码检索，支持 15+ 缺陷与安全检测器及 Tree-sitter S 表达式匹配。
find_files(query, glob)：按路径前缀或 Glob 通配查找工程物理文件。
关系追踪工具集（get_callers, get_call_chain, find_usages, find_implementations）：沿 AST 拓扑追踪调用链、实现与引用点。原生支持 Python 第一方绝对导入（杜绝 dep:: 存根断裂），以及 TS/JS 路径别名与 Go 模块内导入精准映射。

3. 原生独立核心工具调用全景字典

3.1 核心工具调用速查（Core 扁平模式；[*] 表示延迟目录工具）

Core 模式严格属性校验（additionalProperties: false），入参须严格匹配属性名：
1. 全局任务定位：explore(task="...", path="...", token_budget=1600)
2. 上下文规划装配：smart_context(task="...", entry_point="...", fidelity="graded", token_budget=8000)
3. 工程骨架大纲：get_repo_outline()
4. 架构全景分析：analyze(kind="architecture")
5. 符号精确搜索：search_symbols(query="...", kind="...", flavor="...")（固定 assist=off）
6. 文本全文搜索：search_text(query="...", regexp=false, limit=100)
7. AST 语法检索：search_ast(pattern="...") 或 search_ast(detector="...")
8. 物理文件查找：find_files(query="...", glob="*.go")
9. [*] 多轴联合过滤：winnow_symbols(text_match="...", kind="函数", language="go", min_fan_in=2, limit=50)
10. [*] 非代码资产读取：get_artifact(id="art_id")
11. 文件源码读取：read_file(path="path/to/file", offset=1, limit=100, compress_bodies=false)
12. 符号实现源码：get_symbol_source(id="pkg/file.go::Symbol", context_lines=0)
13. 符号元数据读取：get_symbol(id="pkg/file.go::Symbol")
14. [*] 批量符号源码：batch_symbols(symbols=["id1","id2"])
15. 单文件符号概览：get_file_summary(path="path/to/file")
16. 编辑前拓扑分析：get_editing_context(path="path/to/file")
17. 反向调用者查找：get_callers(id="pkg/file.go::Symbol", depth=2)
18. 语法级引用定位：find_usages(id="pkg/file.go::Symbol", context="call")
19. 接口实现查找：find_implementations(id="pkg/file.go::Symbol")
20. [*] 类继承层次树：get_class_hierarchy(id="pkg/file.go::Symbol")
21. 虚方法重写查找：find_overrides(id="pkg/file.go::Symbol")
22. [*] 声明使用跳转：find_declaration(use_site="use_pos")
23. 依赖拓扑评估：get_dependencies(id="pkg/file.go::Symbol") 与 get_dependents(id="pkg/file.go::Symbol")
24. 递归深度调用链：get_call_chain(id="pkg/file.go::Symbol", depth=4)
25. [*] 控制流图分析：get_cfg(id="pkg/file.go::Symbol")
26. [*] 节点最短路径：trace_path(source_id="id1", sink_id="id2")
27. 文件局部替换：edit_file(path, old_string, new_string, dry_run=false, expected_occurrences=1)
28. 符号精确修改：edit_symbol(id, old_source, new_source, dry_run=false)
29. 覆盖写入文件：write_file(path, content)
30. 事务批量修改：batch_edit(dry_run=true, edits=[...])
31. 符号安全重命名：rename_symbol(id="pkg/file.go::Symbol", new_name="NewName", dry_run=true)
32. [*] 符号安全删除：safe_delete_symbol(id="pkg/file.go::Symbol", dry_run=true, propagate=true)
33. 符号跨文件移动：move_symbol(id="pkg/a.go::A", target_file="pkg/b.go", dry_run=true)
34. 微小函数内联：inline_symbol(id="pkg/a.go::fn", dry_run=true)
35. 守护规则检查：check_guards(ids="id1,id2")
36. 受波及单测定位：get_test_targets(ids="id1,id2")
37. 函数签名契约：verify_change(changes='[{"symbol_id":"sym_id","new_signature":"sig"}]')
38. 变更影响评估：explain_change_impact(ids="sym_id") 或 analyze(kind="impact")
39. [*] 变更风险契约：change_contract(diff="...")（固定 ack=false）
40. LSP 编辑模拟：preview_edit(workspace_edit="...")
41. 沙箱链式推演：simulate_chain(steps="[...]", keep=false)（固定 keep=false）
42. 检测脏改动：detect_changes()
43. Diff 上下文：diff_context(scope="unstaged")
44. 代码审查引擎：review(scope="unstaged")
45. 跨服务契约校验：contracts(action="list") 或 analyze(kind="contracts")
46. 复杂度健康评分：audit_health() 或 analyze(kind="health")
47. 代码热点扰动率：get_churn_rate() 或 analyze(kind="churn")
48. 增量变动检测：get_recent_changes() 或 analyze(kind="recent_changes")
49. 代码归属与责任人：analyze(kind="ownership", path_prefix="pkg/", min_symbols=1)
50. 持久规约记忆：store_memory(kind="invariant", title="标题", body="内容")
51. 保存决策笔记：save_note(file_path="pkg/util.go", body="笔记", tags="tag1,tag2")
52. 召回规约记忆：surface_memories(task="任务描述", symbol_ids="sym_id")（固定 mark_accessed=false）
53. 提炼会话摘要：distill_session()
54. 跨项目穿透检索：query_project(project="other_proj", query="...")
55. 切换活跃工程：set_active_project(project="proj_name")
56. 当前活跃工程：get_active_project()
57. 已索引仓库列表：list_repos()
58. 全局图谱统计：graph_stats()
59. 索引健康检查：index_health()
60. [*] 路径视图诊断：explain_view(path="path/to/file")
61. 发现延迟工具：tools_search(query="...")
62. 语言诊断获取：get_diagnostics(path="path/to/file", wait=false, timeout_ms=5000)
63. 分支图层比对：compare_branches(a, b, kind="get_callers", id="...", depth=2, limit=50)
64. 编辑缓冲比对：compare_with_overlay(kind="get_callers", id="...", depth=2, limit=50)
65. 枚举图层分支：overlay_branches()
66. 活动图层列表：overlay_list()
67. 工具开销画像：tool_profile(tool="read_file")

3.2 多项目联合工作区与检索自适应规范

单守护进程多仓库架构下，所有已 track 仓库接入统一联合知识图谱（默认 workspace: default）：
1. 单项目日常分析自动箝位：search_symbols、search_text、find_files、explore 默认锁定当前主场仓库，杜绝跨库污染。
2. 跨项目穿透机制：跨库文件读写直接传绝对路径即时生效；跨库符号检索优先调用 query_project(project="target_proj", query="...")，全图搜索传 repo="*"；get_call_chain、find_usages、contracts、audit_health 等关系型工具全域贯通，Python 绝对导入优先本仓并自动标记 CrossRepo 跨仓边。
3. 规范调用铁律：set_active_project 仅接受逻辑 Project 标识，严禁传路径切域。已 track 项目一切操作必须在 Gortex 原生工具链内闭环。

4. 源码精读、关系分析与架构全景深度指南

4.1 源码阅读契约与 Token 优化策略
1. read_file(path="path/to/file", offset=1, limit=100, compress_bodies=false, keep="")：
   - 扁平模式仅传 path, offset, limit，严禁直传 line_range。
   - 长文件巡检必须开启 compress_bodies=true（存根化，节省 60%~70% Token），配合 keep="fn1,fn2" 精准保留核心函数源码。
2. get_symbol_source(id="pkg/file.go::Symbol", context_lines=0)：精读符号实现体，传 context_lines 扩展外围行数（如声明注解、包定义等）。
3. get_file_summary(path)：快速获取单文件顶级符号大纲、类型分布与导入依赖摘要，避免无意义全文件拉取。
4. get_editing_context(path)：修改前获取目标文件完整拓扑环境（所属模块、相邻符号、引用方分布及受波及范围）。

4.2 拓扑关系与深度数据流追踪
1. get_callers(id, depth=2)：反向递归查询调用者，获取调用方的位置、调用链深度与语法上下文。
2. get_call_chain(id, depth=4)：自顶向下或自底向上递归追踪深度调用链路。
3. find_usages(id, context="call")：语法级定位符号的所有真实使用点（filter 支持 call, read, write, import）。
4. find_implementations(id)：定位接口定义对应的所有具体实现类与方法。
5. find_overrides(id)：定位基类虚函数在派生类中的重写实现。
6. get_dependencies(id) 与 get_dependents(id)：前向与反向依赖评估，快速计算重构影响面。
7. [*] get_cfg(id) 与 [*] trace_path(source_id, sink_id)：深入函数内部控制流分支结构（CFG），或计算两节点间的最短依赖/调用路径。
8. Python 绝对导入全链路闭环：支持 import a.b as m 与 from a.b import f，穿透点分命名空间精准落地 AST 节点，杜绝虚假 dep:: 存根断裂。

4.3 analyze 统一分析全景（78 种 kind）
analyze 工具必须且仅能通过 kind="..." 传参，分为四大核心领域：
1. 架构拓扑与模块边界：
   - cycles：循环依赖检测。would_create_cycle：预测是否引入新循环依赖。
   - clusters：自动识别代码模块社区。suggest_boundaries：推荐架构分层与边界隔离策略。
   - hotspots：结合变更频率与复杂度计算架构热点（未就绪返回 analysis_pending，需重试）。components：强连通组件分析。
   - layer_violations：分层架构违规检测。cohesion & coupling：内聚与耦合度量。
2. 代码健康度、技术债与责任归属：
   - ownership：代码归属统计。遇 absent/partial 需 CLI 运行 gortex enrich blame，严禁 reindex_repository。
   - dead_code：无引用符号检测。Python Pydantic 验证器和序列化器已原生识别为主干入口，免误报；第一方绝对导入解析已落地真实边，杜绝被导入函数误报。
   - coverage_gaps：定位未覆盖的关键业务分支。doc_staleness：识别陈旧 docstring。
   - todos：提取未完成的技术债注释。clones：AST 语义级代码克隆检测。health：综合健康度评分（A-F 级）。
3. 并发安全、缺陷与模式审计：
   - race_writes：竞态写入风险检测。channel_ops：Go Channel 死锁、阻塞与非缓冲通道分析。
   - goroutine_spawns：协程泄漏与未监管生命周期协程追踪。
   - sast：静态安全分析（SQL 注入、硬编码凭证、XSS 与反序列化风险）。hygiene：代码风格卫生度分析。
   - unsafe_patterns：内存越界、未校验指针与危险系统调用检测。routes & models：API 路由与数据模型完整性映射。
4. 跨服务契约与依赖治理：
   - contracts：跨微服务 API 契约一致性校验（可配合 contracts 工具使用）。
   - api_impact：公共 API 变更对消费方的破坏性影响预测。

5. 代码修改、重构与全生命周期闭环验证

5.1 代码修改唯一法定入口与写入契约
对已 track 仓库文件的增删改查必须经由 Gortex 原生写工具完成。严禁调用宿主 replace_file_content、write_to_file 等外挂工具，以防 Gortex 内存图谱、AST 依赖与物理磁盘失步。

核心契约与安全门禁：
1. dry_run=true 与 physical_evidence=true 严格互斥：预览阶段传 dry_run=true（必须 omit physical_evidence），落地阶段传 dry_run=false, physical_evidence=true。
2. 语法门禁与编码安全：默认开启 AST 语法校验，草稿写入传 allow_parse_errors=true 放行。支持 base_sha 防脏写覆盖。UTF-16 源文件写操作底层硬拦截拒绝（防字节损坏，须外部转码 UTF-8）；scaffold/safe_delete 自动保持原文件权限模式。
3. 标准化调用模板：
   - 文件局部替换：edit_file(path, old_string, new_string, dry_run=false, expected_occurrences=1)
     预览时传 dry_run=true，落地时传 dry_run=false, physical_evidence=true。
     传 expected_occurrences=1 防止多处同名串被意外批量覆盖。严禁退回宿主 replace_file_content。
   - 符号精确修改：edit_symbol(id, old_source, new_source, dry_run=false, physical_evidence=true)
   - 全量覆盖写入：write_file(path, content, dry_run=false)（严禁退回宿主 write_to_file）。
   - 批量事务修改（batch_edit）：
     支持原子执行多文件/符号混合变更，单项失败自动整批回滚（Partial Failure 防护）。支持 4 种 op：edit_file、edit_symbol、move_file（必用 source/destination）、delete_file（必用 path）。
     batch_edit(dry_run=true, edits=[
       {"op":"edit_file", "path":"pkg/a.go", "old_string":"旧", "new_string":"新"},
       {"op":"move_file", "source":"pkg/old.go", "destination":"pkg/new.go"},
       {"op":"delete_file", "path":"pkg/del.go"}
     ])
   - 符号物理重构：
     rename_symbol(id, new_name, dry_run=true)（联动更新全图谱所有引用点）。
     move_symbol(id, target_file, dry_run=true)。[*] safe_delete_symbol(id, dry_run=true, propagate=true)。
4. 幂等与重试规范：写入操作若因并发锁定或文件系统延迟报错，优先通过 detect_changes() 重新检测状态，严禁未重新阅读最新内容前盲目重写。

5.2 修改后全栈闭环验证
修改完成后必须严格按序执行闭环验证，严禁草率结案：
1. detect_changes()：核验未提交改动符号范围是否符合预期。
2. get_test_targets(ids="id1,id2")：精确定位受波及单测集合。
3. check_guards(ids="id1,id2")：评估是否触犯架构守护规则与分层违规。
4. verify_change(changes='[{"symbol_id":"sym_id","new_signature":"新签名"}]')：校验函数签名是否破坏下游调用契约。
5. [*] change_contract(diff="...")：预测变更风险等级（固定 ack=false；risk_level="high" 必须显式确认）。
6. 真实测试执行：宿主运行测试命令（go test / npm test / pytest）。配置 gortex githook 时可用 --hook-timeout=30 防挂死。

6. 虚拟图层、状态机与持久化记忆

6.1 overlay 虚拟图层体系
Gortex 原生提供内存级虚拟图层机制，在不污染物理磁盘前提下推演与对比方案：
- 图层管理：overlay_register（注册句柄）、overlay_push（推送内存变更）、overlay_keepalive（维持心跳租约）、overlay_merge（合入落盘传 to_disk=true）、overlay_switch（切换分支）、overlay_fork（派生新分支）、overlay_delete（销毁句柄）、overlay_drop / overlay_drop_branch（丢弃临时修改或分支）。
- 比对与推演：compare_branches(a, b, kind, id, depth, limit) 比对分支依赖差异；compare_with_overlay(kind, id, depth, limit) 比对内存修改与磁盘基线差异；simulate_chain(steps, keep=false) 沙箱推演连续修改影响（固定 keep=false）。

6.2 记忆系统与决策笔记
1. store_memory(kind="invariant", title="标题", body="内容")：持久化记录跨会话关键架构规约。
2. surface_memories(task="...", symbol_ids="id")：召回历史沉淀的规约与决策（固定 mark_accessed=false）。
3. save_note(file_path="pkg/util.go", body="理由", tags="t1,t2")：为特定文件保存决策笔记（参数严格为 file_path）。
4. query_notes(file_path, symbol_id) / query_memories(query)：检索绑定的决策笔记或架构记忆。
5. distill_session()：阶段性任务完成时自动提炼固化本次会话核心上下文。

6.3 代码审查与响应塑形
1. 代码审查工具集：review(scope="unstaged")（代码审查引擎）、diff_context(scope="unstaged")（diff 审查拓扑）、pr_review_context（PR 审查拓扑）、post_review(pr=123, body="...")（提交评审报告）。
2. 响应塑形规范：全查询/分析工具支持 max_bytes、limit/cursor、format（推荐 "gcx"）、fields 稀疏列投影。

7. 冲突裁决优先级与分支视图（View）治理

7.1 路径机制与分支视图治理
1. 平台路径规范：Gortex 内部图谱 ID 严格采用正斜杠 `/`（如 "pkg/foo.go::Bar"），与操作系统物理分隔符彻底解耦。物理文件操作路径在 Windows 下底层自动进行路径规范化，透明兼容 `/` 与 `\`。严禁在符号 ID 或路径中包含 `<>` 等占位字符。
2. 多分支与工作树（View）治理：
   - 当前工作区由 session/CWD 决定视图，显式检出分支构成自动 Overlay。CWD 位于自动 worktree 且路由未就绪时写操作硬拒绝抛 view_building 防主库串写，读操作降级附 rider；拒绝歧义路径推断。
   - 跨分支只读审计传 view={kind:"worktree",checkout_id:"..."} 或 view={kind:"git_ref",value:"refs/heads/release"}。
   - 强一致性查询传 require_exact=true 与 require_fresh=true。
   - 视图异常诊断：若视图解析异常或需查明路径归属，调用 explain_view(path="path/to/file") 诊断底层 checkout 路由链与代际 Generation。

7.2 自顶向下裁决优先级
当规则、源码实现与运行时表现出现歧义时，严格按以下层级自顶向下裁决：
1. 当前工具返回的真实响应（error、completion、view、guard、effect 信息）。
2. 当前运行时 capabilities 与 Schema 规范（request_shape、fixed_arguments、available）。
3. 真实文件系统与代码仓库状态（workspace、index、repository 物理状态）。
4. 当前 Gortex 运行时源码实现。
5. 本规范文档。

多仓库全图谱终极铁律：已 track 仓库统一托管于全局图谱。严禁未经穿透检索断定未索引并退回原生工具。全流程由 Gortex 原生 MCP 闭环，严禁读写割裂与调用宿主工具。严禁跨客户端规则漂移，以当前 Core 扁平工具集为准。仅当 query_project 与路径均证实未 track 且用户明确要求时，方可报告未 track。
