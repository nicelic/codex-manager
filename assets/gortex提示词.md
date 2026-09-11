1. 最高原则和使用边界

Gortex 是对其 track 仓库代码定位、源码精读、关系分析、数据流追踪、影响评估、编辑、重构、验证、审查与记忆的权威工具。

全流程原生 MCP 绝对闭环约束：已 track 仓库的一切操作（包括检索、分析、精读、评估、编辑、替换、写入、重构与删除），必须且仅能通过 Gortex 原生 MCP 句柄闭环执行。
严禁读写割裂：严禁阅读时使用 Gortex、修改时退回宿主原生工具。严禁对已 track 仓库调用宿主工具（包括 replace_file_content、write_to_file、view_file、grep_search、find_by_name 等）。
不可降级与熔断通知：若 Gortex 原生工具无法使用、报错或无法满足需求，严禁擅自退回原生工具自行兜底，必须立即停止操作并向用户说明原因。未 track 的项目不得擅自进行 track 图谱化。
全局技能只读豁免：read_file 放行全局技能只读（~/.agents/skills/*/SKILL.md、~/.config/opencode/skills/*/SKILL.md、~/.copilot/skills/*/SKILL.md、~/.claude/skills/*/SKILL.md）。
严禁凭记忆捏造：严禁伪造未返回的 symbol、关系、view、索引状态或安全结论。
关键实现体完整阅读与 Token 瘦身：摘要和搜索结果不能替代关键实现阅读；行为关键代码读完整源码。长代码巡检时，可在 read_file、get_symbol_source、get_editing_context 中使用 compress_bodies=true（存根化削减 60%~70% Token，支持 keep="f1,f2" 保留指定函数完整源码）。
唯一法定写契约：对已 track 仓库的文件修改、替换与写入，edit_file、batch_edit、write_file、edit_symbol 是唯一法定落地途径。写操作必须严格遵守当前 schema、effect、fixed_arguments、guard、view、overlay 与 partial failure，严禁绕过 Gortex 状态机与图谱同步。

1.1 协议名与双模感知（Core 扁平离散 vs Facade-v1 门面）

根据 MCP 连接预设不同，Gortex 暴露两种工具形态，以当前 session inventory 自适应路由：
1. Core 扁平离散工具模式（Antigravity 默认预设）：直接暴露 explore, smart_context, get_repo_outline, read_file, get_symbol_source, get_callers, search_symbols, search_text, search_ast, edit_file, batch_edit, query_project, contracts, audit_health 等 65 个热核心工具。参数扁平传入。
   热核心集（Hot Eager Tools）：常用基础工具开局立即可用（search_ast, contracts, audit_health, get_churn_rate 均已入热集）。
   延迟目录（Deferred Catalog）：冷工具（如 winnow_symbols, context_closure, plan_turn, safe_delete_symbol 等）托管于冷目录，若未暴露，通过 tools_search(query="...") 唤醒或选用热集等价工具（如 analyze(kind="...")）。
2. Facade-v1 紧凑门面模式（21 个顶级工具）：暴露 read, explore, search, relations, trace, edit, change, refactor, workspace 等门面，使用 operation 与 target 单一选择器规范。

多项目图谱一致性铁律：Gortex 统一维护所有已 track 仓库的全局知识图谱。必须通过第 3.4 节机制在 Gortex 内精准执行。

2. 任务开启与探索流转（Explore & Discovery Workflow）

任务初次分析流转准则：
1. 明确文件名时的首读契约：用户明确指定文件阅读/审查/总结时，首个动作直接调用 read_file(path="<path>")（门面：read(operation="file", target={file:"<path>"}, options={new_user_task:true})），勿触发 localize 定位。
2. 代码定位与任务探索：位置未知或故障排查时，优先使用 explore(task="<完整问题>")（门面：explore(operation="localize", task="...") 用于终结性定位，或 explore(operation="task", task="...") 用于因果诊断）。一键汇聚目标附近的符号、源码与调用链。遵循 completion.required_action：状态为 answer_ready 时直接从 completion.final_response 结案并停用工具；为 needs_exact_read 时补齐精读。
3. 智能上下文装配：优先使用 smart_context(task="<任务>")（门面：explore(operation="context", task="...")）。装配最小完备上下文与编辑规划，支持 fidelity="graded", token_budget, entry_point。
4. 架构大纲速览：优先使用 get_repo_outline()（门面：explore(operation="outline")）。快速获取语言分布、Entry Points、Hotspots 与顶层大纲（扁平无 repo 参）。

2.1 explore 9 大子命令（operations）深度指南
9 个子命令职责与参数：
1. localize（终结性定位）：门面 explore(operation="localize", task="...")（固定 localize=true）；扁平 explore(task="...", path="...", token_budget=1600)。强约束 terminality 状态机。
2. task（诊断与任务分析）：门面 explore(operation="task", task="...")；扁平 explore(task="...")。故障诊断与多步因果分析。
3. context（智能上下文装配）：门面 explore(operation="context", task="...")；扁平 smart_context(task="...", entry_point="...", fidelity="graded", token_budget=8000)。
4. outline（工程骨架）：门面 explore(operation="outline")；扁平 get_repo_outline()。
5. closure（图依赖闭包）：门面 explore(operation="closure")；扁平 context_closure(symbols="id1,id2", files="f1,f2")（参数为 symbols/files）。
6. plan（开局路径路由）：门面 explore(operation="plan")；扁平 plan_turn(task="...")。推荐开局优先调用的工具序列。
7. suggest（冷启动查询词推荐）：门面 explore(operation="suggest")；扁平 suggest_queries()。基于拓扑推荐 5-10 个探索词。
8. prefetch（上下文预取）：门面 explore(operation="prefetch")；扁平 prefetch_context(task="...", recent_symbols="...")。后台预热邻居节点。
9. wakeup（架构全景摘要）：门面 explore(operation="wakeup")；扁平 gortex_wakeup()。生成约 500 Token 架构摘要。

2.2 检索、阅读与图谱协同
各工具正交协同，按需自由组合：
search_text(query="...", regexp=false, path="...", repo="...")：Trigram 索引全文/正则搜索。命中附带 symbol_id 与 symbol_name，可衔接图谱工具。
search_symbols(query="...", kind="...", flavor="...", path="...")：BM25 驼峰分词检索 AST 定义。
search_ast(pattern="...") 或 search_ast(detector="...")：[热] 语法级代码检索。支持 15+ 缺陷/安全检测器及 Tree-sitter S 表达式匹配。
find_files(query="...", glob="*.*")：按路径前缀或 Glob 通配查找工程物理文件。
get_callers / get_call_chain / find_usages / find_implementations：沿 AST 拓扑追踪调用链、实现与精准引用点。
read_file / get_symbol_source：阅读文件或具体函数源码。长文件巡检建议配合 offset/limit 或 compress_bodies=true。

3. 双模调用对照字典与参数防错规范

3.1 双模核心对照字典（基于 Gortex 0.64.3 源码事实）

Core 扁平离散模式下严格属性校验（additionalProperties: false）。入参属性名必须严格匹配。若遇冷目录工具，调用 tools_search 唤醒或选用热核心等价工具。

1. 全局任务定位：explore(task="...") / 门面 explore(operation="localize", task="...")。[热] 单任务入口
2. 智能上下文装配：smart_context(task="...") / 门面 explore(operation="context", task="...")。[热] 装配工作集
3. 工程骨架大纲：get_repo_outline() / 门面 explore(operation="outline")。[热] 模块大纲，无 repo 参
4. 架构统一分析：analyze(kind="architecture") / 门面 analyze(kind="architecture")。[热] 架构全景(门面鉴别参为kind)
5. 符号精确搜索：search_symbols(query="...", kind="...") / 门面 search(operation="symbols", query="...")。[热] 固定 assist=off
6. 文本全文搜索：search_text(query="...", regexp=false) / 门面 search(operation="text", query="...")。[热] Trigram加速
7. AST 语法检索：search_ast(pattern="...") / 门面 search(operation="ast", query="...")。[热] 门面 query 自动别名转 pattern
8. 查找文件：find_files(query="...", glob="*.*") / 门面 search(operation="files", query="...")。[热] 文件名与 Glob 查找
9. 多轴约束过滤：winnow_symbols(text_match="...") / 门面 search(operation="winnow", query="...")。[冷] BM25与图属性联合过滤
10. 读取非代码资产：get_artifact(id="...") / 门面 read(operation="artifact", target={artifact:"..."})。[冷] 读设计规范/文档
11. 读取文件：read_file(path="...", offset=1, limit=100) / 门面 read(operation="file", target={file:"..."}, options={...})。[热] 支持 new_user_task 与 offset/limit
12. 读取符号源码：get_symbol_source(id="<id>") / 门面 read(operation="source", target={symbol:"<id>"})。[热] 读源码实现
13. 读取符号元数据：get_symbol(id="<id>") / 门面 read(operation="symbol_metadata_compat", target={symbol:"<id>"})。[热] 仅元数据
14. 批量读取符号源码：batch_symbols(symbols=["id1","id2"]) / 门面 read(operation="symbols", target={symbols:[...]})。[冷] 批量读源码
15. 单文件符号概览：get_file_summary(path="<path>") / 门面 read(operation="summary", target={file:"<path>"})。[热] 扁平参为 path
16. 编辑前拓扑分析：get_editing_context(path="<path>") / 门面 read(operation="editing_context", target={file:"..."})。[热] 扁平参为 path
17. 反向调用者：get_callers(id="<id>", depth=2) / 门面 relations(operation="callers", target={symbol:"<id>"})。[热] 查调用者
18. 符号引用点：find_usages(id="<id>", context="call") / 门面 relations(operation="usages", target={symbol:"<id>"})。[热] 语法级引用定位
19. 接口实现查找：find_implementations(id="<id>") / 门面 relations(operation="implementations", target={symbol:"..."})。[热] 查接口实现
20. 类/接口继承层次：get_class_hierarchy(id="<id>") / 门面 relations(operation="hierarchy", target={symbol:"<id>"})。[冷] 继承树展开
21. 方法重写查找：find_overrides(id="<id>") / 门面 relations(operation="overrides", target={symbol:"<id>"})。[热] 查虚函数/方法重写
22. 声明跳转：find_declaration(use_site="...") / 门面 relations(operation="declaration", target={query:"..."})。[冷] 使用点反查声明
23. 前向/反向依赖：get_dependencies(id="<id>") / get_dependents(id="<id>") / 门面 relations(operation="dependencies", target={symbol:"<id>"})。[热] 依赖拓扑评估
24. 深度调用链路：get_call_chain(id="<id>", depth=4) / 门面 trace(operation="call_chain", target={symbol:"<id>"})。[热] 递归调用路径追踪
25. 控制流分析：get_cfg(id="<id>") / 门面 trace(operation="cfg", target={symbol:"<id>"})。[冷] 控制流图分析
26. 节点最短路径：trace_path(source_id="...", sink_id="...") / 门面 trace(operation="path", target={symbol:"..."}, to={symbol:"..."})。[冷] 拓扑最短路径
27. 修改文件：edit_file(path="...", old_string="...", new_string="...", dry_run=false) / 门面 edit(operation="file", target={file:"..."}, match="...", replacement="...", dry_run=false)。[热] 预览 dry_run=true 禁带 physical_evidence；严禁宿主 replace_file_content
28. 精确修改符号：edit_symbol(id="...", old_source="...", new_source="...", dry_run=false) / 门面 edit(operation="symbol", target={symbol:"..."}, match="...", replacement="...", dry_run=false)。[热] 精确替换 AST 节点
29. 覆盖写入文件：write_file(path="...", content="...") / 门面 edit(operation="write", target={file:"..."}, content="...")。[热] 全量覆盖写入；严禁宿主 write_to_file
30. 事务型批量修改：batch_edit(edits=[...], dry_run=true) / 门面 edit(operation="batch", changes=[...], dry_run=true)。[热] 支持 edit/move/delete_file
31. 符号安全重命名：rename_symbol(id="...", new_name="...", dry_run=true) / 门面 refactor(operation="rename", target={symbol:"..."}, new_name="...", dry_run=true)。[热] 全图引用联动重构
32. 安全删除符号：safe_delete_symbol(id="...", dry_run=true) / 门面 refactor(operation="delete", target={symbol:"<id>"}, dry_run=true)。[冷] 门禁校验无残留引用
33. 符号物理移动：move_symbol(id="...", target_file="...", dry_run=true) / 门面 refactor(operation="move", target={symbol:"..."}, destination="...")。[热] 跨文件移动(Go)
34. 内联符号：inline_symbol(id="...", dry_run=true) / 门面 refactor(operation="inline", target={symbol:"..."}, dry_run=true)。[热] 内联展开微小函数(Go)
35. 守护规则检查：check_guards(ids="id1,id2") / 门面 change(operation="guards", target={symbols:[...]})。[热] 逗号分隔 ids 字符串
36. 受波及单测定位：get_test_targets(ids="id1,id2") / 门面 change(operation="tests", target={symbols:[...]})。[热] 逗号分隔 ids 字符串
37. 函数签名契约校验：verify_change(changes='[{"symbol_id":"...","new_signature":"..."}]') / 门面 change(operation="verify", source={changes:[...]})。[热] 签名破坏性校验
38. 变更影响评估：explain_change_impact(ids="<id>") / analyze(kind="impact") / 门面 change(operation="impact", target={symbol:"<id>"})。[热] 扁平参为 ids
39. 变更风险契约：change_contract(diff="...") / 门面 change(operation="contract", source={...})。[冷] 预测风险与停止条件
40. LSP 编辑模拟：preview_edit(workspace_edit="...") / 门面 change(operation="preview", source={workspace_edit:"..."})。[热] 专用于 WorkspaceEdit
41. 链式编辑沙箱模拟：simulate_chain(steps="[...]", keep=false) / 门面 change(operation="simulate")。[热] 变更链路沙箱推演
42. 检测未提交改动：detect_changes() / 门面 change(operation="detect")。[热] 工作区脏改动检测
43. Diff 上下文提取：diff_context(scope="unstaged") / 门面 review(operation="diff_context", source={scope:"unstaged"})。[热] 提取 diff 拓扑上下文
44. 图增强代码审查：review(scope="unstaged") / 门面 review(operation="run", source={scope:"unstaged"})。[热] 综合审查引擎
45. API 跨服务契约：contracts(action="list") / 门面 analyze(kind="contracts")。[热] 跨服务契约校验(action 可选 list, check, validate, bridge)
46. 复杂度健康评分：audit_health() / 门面 analyze(kind="health")。[热] A-F 级图健康度评分
47. 代码热点与扰动率：get_churn_rate() / 门面 analyze(kind="churn")。[热] 函数级代码提交扰动率
48. 未重读增量变动：get_recent_changes() / 门面 analyze(kind="recent_changes")。[热] Watch 增量变更检测
49. 持久化规约记忆：store_memory(kind="invariant", title="...", body="...") / 门面 remember(operation="memory", arguments={...})。[热] 记录跨会话不变量
50. 保存代码决策笔记：save_note(file_path="...", body="...", tags="tag1,tag2") / 门面 remember(operation="note", arguments={...})。[热] 扁平参为 file_path
51. 召回规约记忆：surface_memories(task="...", symbol_ids="...") / 门面 recall(operation="surface", arguments={task:"..."})。[热] 召回历史记忆
52. 提炼会话摘要：distill_session() / 门面 recall(operation="distill")。[热] 提炼关键上下文
53. 跨项目穿透检索：query_project(project="...", query="...") / 门面 workspace(operation="project", arguments={project:"...", query:"..."})。[热] 跨库穿透免切检索
54. 热切换活跃工程：set_active_project(project="...") / 门面 workspace_admin(operation="set_active_project", arguments={...})。[热] 动态重定向主工程
55. 当前活跃工程：get_active_project() / 门面 workspace(operation="active_project")。[热] 查询当前绑定工程
56. 已索引仓库列表：list_repos() / 门面 workspace(operation="repos")。[热] 查询已 track 仓库
57. 全局图谱统计：graph_stats() / 门面 workspace(operation="graph")。[热] 节点边与容量统计
58. 守护进程健康检查：index_health() / 门面 workspace(operation="index")。[热] 索引健康度诊断
59. 发现延迟目录工具：tools_search(query="...") / 门面 capabilities(operation="legacy_search")。[热] 动态激活冷目录工具

3.2 参数容器与 arguments 规范

冷门面（8 个）：publish_review, pr, recall, remember, workspace, workspace_admin, overlay, response 在 Schema 中声明了 arguments 属性，参数封装在 arguments: {...} 中。
热门面：explore, search, read, relations, trace, analyze, ask, change, review, edit, refactor 以及 session 严禁包裹外层 arguments，参数放顶层或对应容器（target / options / guard / source / context）。

3.3 固定参数（fixed_arguments）全景

以下参数由运行时强制固化，调用时无需或禁止覆盖：
explore.localize: 固定 localize=true。
search.symbols: 固定 assist=off。
analyze.co_change: 固定 refresh=false。
analyze.help: 固定 kind="help"。
change.contract: 固定 ack=false。
change.simulate: 固定 keep=false。
edit.wiki: 固定 enhance=false。
edit.apply_overlay: 固定 to_disk=true。
recall.surface: 固定 mark_accessed=false。
overlay.simulate: 固定 keep=true。
overlay.merge: 固定 to_disk=false。
remember.risk_ack: 固定 ack=true。
workspace_admin.{blame,coverage,sql_rebuild,temporal_verify}: 固定 kind=<name>。


3.4 多项目联合工作区与双模检索自适应规范

在单守护进程多仓库架构下，所有已 track 仓库通过同一联合工作区（如 workspace: default）构建全局知识图谱，并由各仓库独有的 project 与 repo 标签保持业务独立。MCP 会话在此架构下实现“日常分析极致纯净、跨库协作畅通无阻”。Agent 必须严格遵守以下执行规范：

1. 单项目日常分析与自动箝位（零噪音、高精度）：
- 意图自动聚焦：Gortex 原生启用 Layer-B 定位意图（IntentLocate）。日常执行 search_symbols、search_text、find_files、explore 时，若未显式指定跨库范围，系统默认强制锁定在当前会话绑定的本地主场仓库（session's home repo）。
- 零干扰保障：其他关联项目的同名类、函数或配置不会侵入当前日常检索结果，彻底杜绝跨库符号污染与 Token 浪费，保证单项目分析的高精度与低消耗。

2. 跨项目联合拓扑与穿透机制（畅通无阻）：
- 跨库物理文件读写首选：对已 track 仓库的源码精读与修改，read_file、write_file、edit_file、batch_edit 传入目标工程绝对路径（如 e:/path/to/file）或仓库相对路径（如 kwor/path/to/file）直接生效。底层文件解析在全库已 track 集合内放行，无需切换工程。
- 跨库符号与定义检索：临时查阅外部项目符号时，优先调用 query_project(project="<目标项目>", query="...") 穿透读取。如需全工作区范围符号/文本搜索，显式传入 repo="*" 或 repo="<目标项目>"。
- 跨库调用与架构分析：get_call_chain、find_usages、contracts、audit_health 等关系型工具（IntentReach/Analyze）在联合工作区下天然全域贯通，直接支撑微服务调用链追踪与接口契约校验。

3. 规范调用与异常自愈铁律：
- 严禁调用非法路径参数：set_active_project 仅接受逻辑 Project 标识，底层源码完全不接受绝对路径作为参数，严禁传入绝对路径尝试切域。
- 严禁擅自退回宿主工具：只要目标项目属于已 track 列表，一切阅读、搜索、拓扑与写入操作必须在 Gortex 原生工具链内闭环解决，严禁擅自退回宿主工具（view_file、grep_search、find_by_name、write_to_file、replace_file_content 等）。未 track 的项目不得擅自进行 track 图谱化。

4. 检索、阅读与拓扑分析

4.1 search 7 大操作与高级过滤
可用操作：artifacts, ast, completion, files, symbols, text, winnow。
symbols, text, completion 必须提供非空 query。
ast：通过语法模式或检测器检索（支持 pattern, detector, language，门面下 query 自动别名转 pattern）。
artifacts：搜设计文档、架构规范、OpenAPI 模式等非代码资产（支持 query, kind）。
winnow：多轴结构化检索，结合 BM25 文本与图属性筛选（支持 text_match、kind、language、path_prefix、min_fan_in、limit 等）。

4.2 read 7 大操作与单一选择器
可用操作：artifact, editing_context, file, history, source, summary, symbols。
选择器单一性：target 必须为对象且仅能有单一键（file, symbol, symbols, query, artifact, repo 选 1 个）。
read.file 支持 options={offset:1, limit:100} 或 context={start_line:1, end_line:100}（底层自动换算）。首读明确文件支持 options={new_user_task:true}。
read.source：精读符号实现体，可指定 context_lines 扩展外围行数。

4.3 relations (11个) 与 trace (7个)
relations（11个）：callers（调用者）, cluster（连通社区）, declaration（声明定位，扁平入参 use_site）, dependencies（前向依赖）, dependents（反向依赖）, hierarchy（类继承树）, implementations（接口实现）, import_path（导入路径）, overrides（虚方法重写）, references（引用完整性）, usages（代码使用点）。
trace（7个）：call_chain（调用链路追踪）, cfg（控制流图）, flow（端到端数据流向）, graph（通用图谱查询）, path（节点最短路径，入参 source_id/sink_id）, taint（污点传播分析）, walk（图漫游拓扑步进）。

4.4 analyze 统一分析全景（Core 78 种 kind vs Facade 22 种 kind）
Core 模式 analyze(kind="...")：内置 78 种分析 kind。涵盖架构拓扑（cycles, would_create_cycle, clusters, suggest_boundaries, hotspots, components）、代码健康（dead_code, coverage_gaps, doc_staleness, todos）、并发与安全（race_writes, channel_ops, goroutine_spawns, sast, hygiene, unsafe_patterns, routes, models）等。传 kind="help" 获取全量清单。
Facade 门面 analyze(kind="...")：鉴别参数固化为 kind，包含 22 个子种类，聚合路由至离散分析（如 contracts, health, churn, recent_changes, architecture, clones, untested, why, lint 等）。切勿传 operation="..." 导致静默退化为 help。

5. 修改、重构与验证

5.1 修改前评估：change 19 大操作
可用操作：api_impact, code_actions, compare_branches, compare_overlay, contract, detect, diagnostics, edit_plan, guards, impact, overlay_branches, overlay_state, pattern, preview, ranges, receipt, simulate, tests, verify。

5.2 edit (10个) 与 refactor (6个) 代码修改
可用 edit 操作：apply_overlay, batch, docs, export_graph, file, scaffold, skill, symbol, wiki, write。
可用 refactor 操作：apply_code_action, delete, fix_all, inline, move, rename。

代码修改唯一性铁律：对已 track 仓库文件的增删改查必须经由 edit 与 refactor 体系完成。严禁调用宿主 replace_file_content、write_to_file 等外挂工具，以防 Gortex 内存图谱、AST 依赖与物理磁盘失步。

核心契约与模板：
1. dry_run: true 与 physical_evidence: true 互斥：预览必须 dry_run=true（禁带 physical_evidence）；正式写入 dry_run=false 可开启 physical_evidence=true。
2. 语法门禁：默认语法校验，草稿写入可传 allow_parse_errors=true；传 base_sha 防并发脏写。门面模式 edit 下支持 match 与 replacement 字段（底层自动映射）。
3. 标准化调用模板：
   文件修改：预览 edit_file(path="<f>", old_string="<o>", new_string="<n>", dry_run=true, expected_occurrences=1)；写入改 dry_run=false, physical_evidence=true。严禁退回宿主 replace_file_content。
   符号修改：预览 edit_symbol(id="<id>", old_source="<o>", new_source="<n>", dry_run=true)；写入改 dry_run=false, physical_evidence=true。
   批量事务（支持 4 种 op）：batch_edit(dry_run=true, edits=[{"op":"edit_file","path":"<f>","old_string":"<o>","new_string":"<n>"},{"op":"edit_symbol","id":"<id>","old_source":"<o>","new_source":"<n>"},{"op":"move_file","source":"<s>","destination":"<d>"},{"op":"delete_file","path":"<p>"}])。
   移动与内联：move_symbol(id="<id>", target_file="<path>", dry_run=true)；inline_symbol(id="<id>", dry_run=true)。
   重命名与删除：rename_symbol(id="<id>", new_name="<name>", dry_run=true)；safe_delete_symbol(id="<id>", dry_run=true, propagate=true)。

5.3 修改后闭环验证
1. detect_changes()：获取变动符号集合。
2. get_test_targets(ids="id1,id2")：定位受波及单测。
3. check_guards(ids="id1,id2")：评估防护规则与架构边界。
4. change_contract(...)：评估变更风险契约与停止条件。
5. 真实测试执行：在宿主环境运行构建与测试命令（go test, npm test 等）。

6. 状态机、隔离层与持久化记忆

6.1 overlay（10个）虚拟图层
操作：register, push, merge(固定 to_disk=false), keepalive, switch, drop, drop_branch, fork, delete, simulate(固定 keep=true)。
落盘调用 edit(operation="apply_overlay")（固定 to_disk=true）。

6.2 recall (8个) 与 remember (8个) 记忆系统
recall：distill_session()（会话摘要）；surface_memories(task="...", symbol_ids="...")（召回规约，固定 mark_accessed=false）；query_notes(file_path="..." 或 symbol_id="...")（查代码笔记）；query_memories(query="...")（查持久规约）；notebook_find / notebook_list / notebook_show（笔记本检索）；check_onboarding_performed（入职导引状态）。
remember：store_memory(kind="invariant", title="...", body="...")（存关键架构不变量）；save_note(file_path="...", body="...", tags="tag1,tag2")（记录决策笔记）；remember(operation="risk_ack")（确认破坏性变动，固定 ack=true）；edit_memory / rename_memory（编辑规约）；suppress_finding（抑制已知告警）；notebook_save / notebook_used（笔记本持久化与续签）。

6.3 session（16个）会话控制与事件订阅
基础会话控制（6个）：agents（多Agent协调，action: list, register, heartbeat, lock, unlock, unregister）, cursor（虚拟导航游标）, planning_mode（规划模式开关，arguments: {enabled:true}）, proxy_enable / proxy_disable（图代理开关）, workflow（工作流调度）。
事件订阅与退订（10个）：针对 daemon_health, diagnostics, graph_invalidated, stale_refs, workspace_readiness 5 类事件的 subscribe_<event> / unsubscribe_<event>。门面模式支持 session(operation="subscribe", channel="<event>")。

6.4 review (7个), pr (6个), response (5个)
review：review(scope="unstaged"), diff_context(scope="unstaged"), review_pack, critique_review, pr_review_context, suggested_review_questions, sibling_diff_context。
pr：list_prs, get_pr_impact, conflicts_prs, suggest_reviewers, pr_risk, triage_prs。
publish_review：post_review(pr=123, body="...")。门面模式为 publish_review(operation="post", arguments={pr:123, body:"..."})。
response：超大响应裁剪 response(operation="slice", arguments={start:1, end:50})；grep（正则过滤）；peek（首尾预览）；stats（句柄统计）；export_context（导出上下文）。

7. 通用响应控制与冲突裁决

7.1 通用 Output 响应塑形参数
门面与查询类扁平工具支持统一 output 控制对象与顶层参数：max_bytes（最大字节）、limit（最大条数）、format（json、gcx、toon，gcx 节约约 27% Token）、cursor（增量分页游标）、fields（稀疏列投影）、scope（保存范围限定）。

7.2 冲突裁决优先级与分支视图（View）
多分支与工作树（View）治理：当前工作区由 session/CWD 决定视图，显式检出分支构成自动 Overlay；跨分支只读审计传 view={kind:"worktree",checkout_id:"..."} 或 view={kind:"git_ref",value:"refs/heads/release"}；强一致性传 require_exact=true 与 require_fresh=true。
自顶向下裁决优先级：
1. 当前工具返回的真实响应（error、completion、view、guard、effect 信息）。
2. 当前运行时 capabilities / Schema 规范（request_shape、fixed_arguments、available）。
3. 真实文件系统与代码仓库状态（workspace、index、repository 物理状态）。
4. 当前 Gortex 0.64.3 运行时源码实现。
5. 本规范文档。

多仓库全图谱最终铁律：已 track 仓库统一托管于全局图谱。严禁未经穿透检索或静默切域直接断定未索引并退回原生工具。全流程（探索、定位、精读、修改、重构、验证）全程由 Gortex MCP 闭环，严禁读写脱节。严禁调用 replace_file_content、write_to_file、view_file、grep_search 等宿主工具。仅当 query_project 与带前缀路径均证实未 track 且用户明确要求本地检查时，方可报告未 track。
