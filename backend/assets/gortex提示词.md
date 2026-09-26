1. 最高原则和使用边界

Gortex 是对其 track 仓库代码定位、源码精读、关系分析、数据流追踪、影响评估、编辑、重构、验证、审查与记忆的权威工具。

全流程原生 MCP 绝对闭环约束：已 track 仓库的一切操作（包括检索、分析、精读、评估、编辑、替换、写入、重构与删除），必须且仅能通过 Gortex 原生 MCP 句柄闭环执行。
严禁读写割裂：严禁阅读时使用 Gortex、修改时退回宿主原生工具。严禁对已 track 仓库调用宿主工具（包括 replace_file_content、write_to_file、view_file、grep_search、find_by_name 等）。
不可降级与熔断通知：若 Gortex 原生工具无法使用、报错或无法满足需求，严禁擅自退回原生工具自行兜底，必须立即停止操作并向用户说明原因。未 track 的项目不得擅自进行 track 图谱化。
全局技能只读豁免：read_file 放行全局技能只读（~/.agents/skills/*/SKILL.md、~/.config/opencode/skills/*/SKILL.md、~/.copilot/skills/*/SKILL.md、~/.claude/skills/*/SKILL.md 及 Pi 包生态）。
严禁凭记忆捏造：严禁伪造未返回的 symbol、关系、view、索引状态或安全结论。
关键实现体完整阅读与 Token 瘦身：摘要和搜索结果不能替代关键实现阅读；行为关键代码必须精读完整源码。长代码巡检与批量阅读时，可在 read_file、get_symbol_source、get_editing_context 中使用 compress_bodies=true（存根化削减 60%~70% Token，支持 keep="f1,f2" 保留指定函数完整源码），配合 format="gcx" 达到最佳 Token 瘦身效果。
唯一法定写契约：对已 track 仓库的文件修改、替换与写入，edit_file、batch_edit、write_file、edit_symbol 是唯一法定落地途径。写操作必须严格遵守当前 schema、effect、fixed_arguments、guard、view、overlay 与 partial failure，严禁绕过 Gortex 状态机与图谱同步。

1.1 当前运行环境核心工具集与协议规范

本环境采用 Core 扁平离散工具预设（暴露 explore, smart_context, get_repo_outline, read_file, get_symbol_source, get_callers, search_symbols, search_text, search_ast, edit_file, batch_edit, query_project, contracts, audit_health 等 65 个热核心与延迟工具）。调用时参数扁平传入，严格遵守以下法定协议：
1. 纯净 ToolName 铁律：MCP 工具命令名永远为纯小写下划线标识符（如 read_file, get_symbol_source）。当前环境不存在任何带句点、带斜杠或带操作派发的工具名，严禁拼接虚构工具名。
2. 真实参数值契约：
   - 符号 id：必须传入具体符号节点字符串（格式为 "文件相对路径::符号名"，如 "pkg/foo.go::Bar"）或全局唯一短符号名（如 "Bar"），严禁使用尖括号等占位符号。路径分隔符统一使用正斜杠。
   - 文件 path：必须传入具体工程相对路径（如 "src/main.rs"）或绝对路径，严禁使用尖括号等占位符号。
3. analyze 鉴别参铁律：analyze 工具的操作鉴别参数一律必须传 kind="..."（如 analyze(kind="architecture")），严禁传 operation="..." 导致静默退化为 help。
4. 延迟目录发现机制（Deferred Catalog）：部分深度分析或高级重构工具（如 winnow_symbols, context_closure, plan_turn, safe_delete_symbol, symbols_for_ranges, get_cfg, trace_path 等）托管于冷目录，若初始未暴露，可通过 tools_search(query="...") 动态唤醒或选用热集等价工具。
5. 系统固定参数（fixed_arguments）约束：以下参数由运行时强制固化，调用时严禁覆盖或违背：
   - search_symbols：固定 assist="off"（强制关闭外部大模型模糊猜测，确保返回结果 100% 为真实 AST 节点）。
   - change_contract：固定 ack=false（必须由模型完整审查变更影响后方可推进，禁止盲目自动确认）。
   - simulate_chain：固定 keep=false（沙箱多步推演仅用于变更影响预测，严禁在推演后保留临时图层）。
   - surface_memories：固定 mark_accessed=false（召回架构规约时仅读取，不污染规约生命周期的访问计数器）。
   - overlay_merge：默认 to_disk=false（内存合并默认不污染物理磁盘，仅在用户确认落盘时显式传 to_disk=true）。
6. 统一 Output 响应控制与 Token 削减：查询与分析类工具支持统一 output 控制参数（顶层扁平传入）：
   - max_bytes：严格限制输出最大字节上限，防止超大上下文撑爆窗口。
   - limit 与 cursor：配合 Trigram 与图谱遍历的增量游标分页流转。
   - format：支持 "json"、"toon"、"gcx"（其中 "gcx" 为 Gortex 专有紧凑编码，可削减约 27% Token 消耗）。
   - fields：稀疏字段投影（如 fields="id,name,path"），按需裁剪无用元数据。
   - scope：精确限定分析与检索范围（如 scope="unstaged"）。

2. 任务开启与探索流转（Explore & Discovery Workflow）

任务初次分析流转准则：
1. 明确文件名时的首读契约：用户明确指定文件阅读、审查或总结时，首个动作直接调用 read_file(path="path/to/file.ext")，勿触发 localize 定位。
2. 代码定位与任务探索：未知位置或故障排查时，优先使用 explore(task="完整问题描述")。一键汇聚目标附近的符号、源码与调用链。遵循 completion.required_action 状态机：
   - answer_ready：探索结果已足够判定因果，直接从 completion.final_response 结案并停用工具。
   - needs_exact_read：需要对候选符号源码深入求证，补齐 read_file 或 get_symbol_source 精读。
   - needs_more_context：候选区域过于宽泛，需缩小 task 范围或指定 path 追加线索。
3. 智能上下文装配：优先使用 smart_context(task="任务描述")。装配最小完备上下文与多文件编辑规划，支持 fidelity="graded", token_budget, entry_point。
4. 架构大纲速览：优先使用 get_repo_outline()。快速获取整个工程的语言分布、Entry Points（主干入口）、Hotspots（代码热点）与顶层模块骨架（扁平无 repo 参数）。

2.1 任务探索与上下文工具集
1. explore(task="...", path="...", token_budget=1600)：终结性定位与因果诊断，强约束 terminality 状态机。
2. smart_context(task="...", entry_point="...", fidelity="graded", token_budget=8000)：装配最小完备上下文与多文件编辑规划。
3. get_repo_outline()：快速获取工程语言分布、入口函数、热点分布与顶层模块大纲。
4. context_closure(symbols="id1,id2", files="f1,f2")：沿图谱依赖计算目标符号与文件的拓扑闭包。
5. plan_turn(task="...")：基于当前任务分析推荐开局优先调用的工具序列。
6. suggest_queries()：基于拓扑图谱结构推荐 5-10 个高效探索词。
7. prefetch_context(task="...", recent_symbols="...")：后台预热邻居节点上下文。
8. gortex_wakeup()：生成约 500 Token 的工程架构全景摘要。

2.2 检索、阅读与图谱协同
各工具正交协同，按需自由组合：
search_text(query="...", regexp=false, limit=100, path="...", repo="...")：Trigram 索引全文或正则搜索。命中附带 symbol_id 与 symbol_name，可衔接图谱工具。支持 limit（默认 100，上限 1000）。若返回 _truncated_by_limit=true 则 count_is_exact=false（count 仅为下界 floor；受截断结果严禁追加 path 参数找回未命中项，需扩大 limit 或细化 query）。
search_symbols(query="...", kind="...", flavor="...", path="...")：BM25 驼峰分词检索 AST 定义，固定 assist=off。
search_ast(pattern="...") 或 search_ast(detector="...")：[热] 语法级代码检索。支持 15+ 缺陷与安全检测器及 Tree-sitter S 表达式匹配（Python 下 f-string 常量插值免误报，Pydantic 验证器识别为主干入口）。
find_files(query="...", glob="*.*")：按路径前缀或 Glob 通配查找工程物理文件。
关系追踪工具集（get_callers, get_call_chain, find_usages, find_implementations）：沿 AST 拓扑追踪调用链、实现与引用点。原生支持 Python 第一方绝对导入（杜绝 dep:: 存根断裂），以及 TS/JS 路径别名与 Go 模块内导入精准映射。
源码精读工具集：使用 read_file 查看文件完整内容，使用 get_symbol_source 查看指定函数或类型实现体。长文件巡检建议配合 offset 与 limit，或开启 compress_bodies=true。

3. 原生独立核心工具调用全景字典

3.1 核心工具调用速查（Core 扁平模式；[*] 表示延迟目录工具）

Core 模式严格属性校验（additionalProperties: false），入参须严格匹配属性名：
1. 全局任务定位：explore(task="问题描述", path="可选路径", token_budget=1600)
2. 上下文规划装配：smart_context(task="任务描述", entry_point="入口符号", fidelity="graded", token_budget=8000)
3. 工程骨架大纲：get_repo_outline()（无 repo 参）
4. 架构全景分析：analyze(kind="architecture")（鉴别参必须为 kind）
5. 符号精确搜索：search_symbols(query="名称", kind="函数/类", flavor="定义类型")（固定 assist=off）
6. 文本全文搜索：search_text(query="文本", regexp=false, limit=100)（截断返回 _truncated_by_limit=true）
7. AST 语法检索：search_ast(pattern="模式串") 或 search_ast(detector="检测器名称")
8. 物理文件查找：find_files(query="路径前缀", glob="*.go")
9. 多轴联合过滤：[*] winnow_symbols(text_match="文本", kind="函数", language="go", path_prefix="pkg/", min_fan_in=2, limit=50)
10. 非代码资产读取：[*] get_artifact(id="art_id")（读取架构规范、设计文档或 OpenAPI Schema）
11. 文件源码读取：read_file(path="path/to/file", offset=1, limit=100, compress_bodies=false)（扁平禁传 line_range）
12. 符号实现源码：get_symbol_source(id="pkg/file.go::Symbol", context_lines=0)
13. 符号元数据读取：get_symbol(id="pkg/file.go::Symbol")（仅返回元数据无源码体）
14. 批量符号源码：[*] batch_symbols(symbols=["id1","id2"])
15. 单文件符号概览：get_file_summary(path="path/to/file")
16. 编辑前拓扑分析：get_editing_context(path="path/to/file")
17. 反向调用者查找：get_callers(id="pkg/file.go::Symbol", depth=2)
18. 语法级引用定位：find_usages(id="pkg/file.go::Symbol", context="call")
19. 接口实现查找：find_implementations(id="pkg/file.go::Symbol")
20. 类继承层次树：[*] get_class_hierarchy(id="pkg/file.go::Symbol")
21. 虚方法重写查找：find_overrides(id="pkg/file.go::Symbol")
22. 声明使用跳转：[*] find_declaration(use_site="use_pos")
23. 依赖拓扑评估：get_dependencies(id="pkg/file.go::Symbol") 与 get_dependents(id="pkg/file.go::Symbol")
24. 递归深度调用链：get_call_chain(id="pkg/file.go::Symbol", depth=4)
25. 控制流图分析：[*] get_cfg(id="pkg/file.go::Symbol")
26. 节点最短路径：[*] trace_path(source_id="id1", sink_id="id2")
27. 文件局部替换：edit_file(path="path/to/file", old_string="旧文本", new_string="新文本", dry_run=false, expected_occurrences=1)
28. 符号精确修改：edit_symbol(id="pkg/file.go::Symbol", old_source="旧实现", new_source="新实现", dry_run=false)
29. 覆盖写入文件：write_file(path="path/to/file", content="全量文本内容")
30. 事务批量修改：batch_edit(dry_run=true, edits=[...])（move_file 必用 source 和 destination）
31. 符号安全重命名：rename_symbol(id="pkg/file.go::Symbol", new_name="NewName", dry_run=true)
32. 符号安全删除：[*] safe_delete_symbol(id="pkg/file.go::Symbol", dry_run=true, propagate=true)
33. 符号跨文件移动：move_symbol(id="pkg/a.go::A", target_file="pkg/b.go", dry_run=true)
34. 微小函数内联：inline_symbol(id="pkg/a.go::fn", dry_run=true)
35. 守护规则检查：check_guards(ids="id1,id2")（扁平 ids 必为逗号分隔字符串）
36. 受波及单测定位：get_test_targets(ids="id1,id2")（扁平 ids 必为逗号分隔字符串）
37. 函数签名契约：verify_change(changes='[{"symbol_id":"sym_id","new_signature":"sig"}]')
38. 变更影响评估：explain_change_impact(ids="sym_id") 或 analyze(kind="impact")
39. 变更风险契约：[*] change_contract(diff="...")（固定 ack=false，评估变更风险与停止条件）
40. LSP 编辑模拟：preview_edit(workspace_edit="...")（沙箱验证 LSP 事务变更）
41. 沙箱链式推演：simulate_chain(steps="[...]", keep=false)（固定 keep=false）
42. 检测脏改动：detect_changes()
43. Diff 上下文：diff_context(scope="unstaged")
44. 代码审查引擎：review(scope="unstaged")
45. 跨服务契约校验：contracts(action="list") 或 analyze(kind="contracts")（action 可选 list, check, validate, bridge）
46. 复杂度健康评分：audit_health() 或 analyze(kind="health")
47. 代码热点扰动率：get_churn_rate() 或 analyze(kind="churn")
48. 增量变动检测：get_recent_changes() 或 analyze(kind="recent_changes")
49. 代码归属与责任人：analyze(kind="ownership", path_prefix="pkg/", min_symbols=1)（遵守 data_state 状态机：absent/partial 需 CLI gortex enrich blame，严禁 reindex_repository）
50. 持久规约记忆：store_memory(kind="invariant", title="标题", body="内容")
51. 保存决策笔记：save_note(file_path="pkg/util.go", body="笔记", tags="tag1,tag2")（参数名严格为 file_path）
52. 召回规约记忆：surface_memories(task="任务描述", symbol_ids="sym_id")（固定 mark_accessed=false）
53. 提炼会话摘要：distill_session()
54. 跨项目穿透检索：query_project(project="other_proj", query="...")
55. 切换活跃工程：set_active_project(project="proj_name")（仅收逻辑项目名，严禁绝对路径）
56. 当前活跃工程：get_active_project()
57. 已索引仓库列表：list_repos()
58. 全局图谱统计：graph_stats()
59. 索引健康检查：index_health()
60. 路径视图诊断：[*] explain_view(path="path/to/file")（诊断物理文件 checkout 路由代际 Generation 映射）
61. 发现延迟工具：tools_search(query="...")
62. 语言诊断获取：get_diagnostics(path="path/to/file", wait=false, timeout_ms=5000)
63. 分支图层比对：compare_branches(a="branch1", b="branch2", kind="get_callers", id="pkg/foo.go::Bar", depth=2, limit=50)
64. 编辑缓冲比对：compare_with_overlay(kind="get_callers", id="pkg/foo.go::Bar", depth=2, limit=50)
65. 枚举图层分支：overlay_branches()
66. 活动图层列表：overlay_list()
67. 工具开销画像：tool_profile(tool="read_file")

3.2 多项目联合工作区与检索自适应规范

单守护进程多仓库架构下，所有已 track 仓库接入统一联合知识图谱（默认 workspace: default），通过 project 与 repo 标签保持业务隔离：

1. 单项目日常分析与自动箝位（零噪音、高精度）：
- 意图自动聚焦：启用 Layer-B 定位意图（IntentLocate）。日常 search_symbols、search_text、find_files、explore 若未显式指定跨库范围，强制锁定在当前主场仓库（session's home repo）。
- 零干扰保障：其他关联项目的同名符号不会侵入日常检索，杜绝跨库污染与 Token 浪费。

2. 跨项目联合拓扑与穿透机制（畅通无阻）：
- 跨库物理文件读写：read_file、write_file、edit_file、batch_edit 直接传入目标工程绝对路径（如 e:/path/to/file）或仓库相对路径即时生效，底层在全库 track 集合内放行，无需切换工程。
- 跨库符号与定义检索：临时查阅外部符号优先调用 query_project(project="target_proj", query="...")。全图搜索显式传 repo="*" 或 repo="target_proj"。
- 跨库调用与架构分析：get_call_chain、find_usages、contracts、audit_health 等关系型工具全域贯通。Python 绝对导入解析原生兼备仓亲和偏好（优先本仓并自动标记 CrossRepo 跨仓边），多仓引用无缝穿透。

3. 规范调用与异常自愈铁律：
- 严禁非法路径切域：set_active_project 仅接受逻辑 Project 标识，底层不接受绝对路径，严禁传路径切域。
- 严禁擅自退回宿主工具：只要目标项目属于已 track 列表，一切操作必须在 Gortex 原生工具链内闭环。未 track 的项目不得擅自进行 track 图谱化。

4. 源码精读、关系分析与架构全景深度指南

4.1 源码阅读契约与 Token 优化策略
1. read_file(path="path/to/file", offset=1, limit=100, compress_bodies=false, keep="")：
   - 首读支持 options={new_user_task:true} 标记新任务起点。
   - 扁平模式仅传 path, offset, limit，严禁直传 line_range。
   - 长文件巡检必须开启 compress_bodies=true（将函数实现体存根化为 Stubs，直接节省 60%~70% Token 消耗），配合 keep="fn1,fn2" 可精准保留关心的核心函数源码。
2. get_symbol_source(id="pkg/file.go::Symbol", context_lines=0)：
   - 精读符号实现体，传 context_lines 扩展外围行数（如声明注解、包定义等）。
3. get_file_summary(path="path/to/file")：
   - 快速获取单文件的顶级符号大纲、类型分布与导入依赖摘要，避免无意义全文件拉取。
4. get_editing_context(path="path/to/file")：
   - 在实施物理修改前，获取目标文件完整的拓扑环境（所属模块、相邻符号、引用方分布及受波及范围）。

4.2 拓扑关系与深度数据流追踪
1. get_callers(id="...", depth=2)：反向递归查询调用者，精准获取调用方的位置、调用链深度与语法上下文。
2. get_call_chain(id="...", depth=4)：自顶向下或自底向上递归追踪深度调用链路。
3. find_usages(id="...", context="call")：语法级定位符号的所有真实使用点（支持 filter 为 call, read, write, import）。
4. find_implementations(id="...")：定位接口定义对应的所有具体实现类与方法。
5. find_overrides(id="...")：定位基类虚函数在派生类中的重写实现。
6. get_dependencies(id="...") 与 get_dependents(id="...")：前向与反向依赖评估，快速计算重构影响面。
7. [*] get_cfg(id="...") 与 [*] trace_path(source_id="...", sink_id="...")：深入函数内部控制流分支结构（CFG），或计算两节点间在全图谱拓扑中的最短依赖/调用路径。
8. Python 绝对导入全链路闭环：上述工具深度支持 Python 现代模块规范，不论是 import a.b as m 还是 from a.b import f，底层均能穿透点分命名空间精准落地 AST 节点，彻底杜绝虚假 dep:: 存根断裂。

4.3 analyze 统一分析全景（78 种 kind 分类详述）
analyze 工具必须且仅能通过 kind="..." 传参，内置 78 种分析能力，分为四大核心领域：
1. 架构拓扑与模块边界分析：
   - cycles：循环依赖检测，定位工程中模块或包之间的相互依赖闭环。
   - would_create_cycle：模拟依赖注入前预测是否会引入新的循环依赖。
   - clusters：基于图拓扑凝聚力自动识别代码模块社区。
   - suggest_boundaries：智能推荐架构分层与边界隔离策略。
   - hotspots：结合代码变更频率与复杂度计算架构热点区域。
   - components：强连通组件分析与解耦边界建议。
   - layer_violations：分层架构违规检测（如底层逆向依赖表现层）。
   - cohesion & coupling：度量模块的高内聚与低耦合指标。
2. 代码健康度、技术债与责任归属：
   - ownership：代码归属与责任人统计。遵循严格的 data_state 状态机：遇 absent/partial 需 CLI 运行 gortex enrich blame 修复 Git Blame 缓存，严禁 reindex_repository；若作者存在但代码行数为空，可调低 min_symbols 重试。
   - dead_code：死代码与无引用符号检测。Python Pydantic 验证器（validator, field_validator）和序列化器（serializer）已原生识别为主干活动入口，免除误报；Python 第一方绝对导入解析已落地真实边，杜绝被导入函数被误报为 dead_code。
   - coverage_gaps：结合测试覆盖率数据定位未覆盖的关键业务分支。
   - doc_staleness：文档与实现版本差异检测，识别陈旧 docstring。
   - todos：提取工程中未完成的技术债注释与待办项。
   - clones：重复代码片段（AST 语义级代码克隆）检测。
   - health：图谱与复杂度全局健康度综合评分（A-F 级评级）。
3. 并发安全、缺陷与模式审计：
   - race_writes：多协程/多线程竞态写入风险检测。
   - channel_ops：Go Channel 死锁、阻塞与非缓冲通道操作分析。
   - goroutine_spawns：协程泄漏与未监管生命周期协程追踪。
   - sast：静态安全分析（包含 SQL 注入、硬编码凭证、XSS 与反序列化风险）。
   - hygiene：代码风格与规范卫生度分析。
   - unsafe_patterns：内存越界、未校验指针与危险系统调用检测。
   - routes & models：Web 框架 API 路由与数据模型完整性映射。
4. 跨服务契约与依赖治理：
   - contracts：跨微服务 API 契约一致性校验（可配合 contracts 工具使用）。
   - api_impact：公共 API 变更对消费方的破坏性影响预测。

5. 代码修改、重构与全生命周期闭环验证

5.1 代码修改唯一法定入口与写入契约
对已 track 仓库文件的增删改查必须经由 Gortex 原生写工具完成。严禁调用宿主 replace_file_content、write_to_file 等外挂工具，以防 Gortex 内存图谱、AST 依赖与物理磁盘失步。

核心契约与安全门禁：
1. dry_run: true 与 physical_evidence: true 严格互斥：
   - 预览阶段：dry_run=true（必须 omit physical_evidence，混传直接报错）。
   - 正式落地阶段：dry_run=false 方可开启 physical_evidence=true，由引擎校验物理磁盘证据。
2. 语法门禁与并发保护：
   - 默认开启 AST 语法校验，草稿或实验性写入可传 allow_parse_errors=true 放行语法错误。
   - 支持传入 base_sha，由引擎检测自读出以来文件是否发生外部并发修改，防止脏写覆盖。
3. 标准化调用模板：
   - 文件局部替换：
     预览：edit_file(path="pkg/util.go", old_string="旧文本", new_string="新文本", dry_run=true, expected_occurrences=1)
     落地：edit_file(path="pkg/util.go", old_string="旧文本", new_string="新文本", dry_run=false, physical_evidence=true)
     （严禁退回宿主 replace_file_content；传 expected_occurrences=1 防止多处同名串被意外批量覆盖）。
   - 符号精确修改：
     预览：edit_symbol(id="pkg/util.go::helper", old_source="旧函数源码", new_source="新函数源码", dry_run=true)
     落地：edit_symbol(id="pkg/util.go::helper", old_source="旧函数源码", new_source="新函数源码", dry_run=false, physical_evidence=true)
   - 全量覆盖写入：
     write_file(path="pkg/util.go", content="全量文件文本", dry_run=false)（严禁退回宿主 write_to_file）。
   - 批量事务修改（batch_edit）：
     支持原子执行多文件/符号混合变更，单项失败自动整批回滚（Partial Failure 防护）。支持 4 种 op 类型：
     batch_edit(dry_run=true, edits=[
       {"op":"edit_file", "path":"pkg/a.go", "old_string":"旧", "new_string":"新"},
       {"op":"edit_symbol", "id":"pkg/a.go::fn", "old_source":"旧", "new_source":"新"},
       {"op":"move_file", "source":"pkg/old.go", "destination":"pkg/new.go"},
       {"op":"delete_file", "path":"pkg/del.go"}
     ])
     （注意：move_file 必须严格使用 source 与 destination 参数名；delete_file 必须使用 path 参数名）。
   - 符号物理重构：
     - 重命名：rename_symbol(id="pkg/a.go::A", new_name="NewA", dry_run=true)（联动更新全图谱所有引用点）。
     - 跨文件移动：move_symbol(id="pkg/a.go::A", target_file="pkg/b.go", dry_run=true)。
     - 微小函数内联：inline_symbol(id="pkg/a.go::tinyFn", dry_run=true)。
     - 安全删除：[*] safe_delete_symbol(id="pkg/a.go::Dead", dry_run=true, propagate=true)。
4. 幂等与重试规范：若物理落地操作因并发锁定或文件系统延迟报错，优先通过 detect_changes() 重新检测当前工作区真实状态，严禁在未重新阅读最新内容前盲目重复执行写入。

5.2 修改后全栈闭环验证
完成修改后，必须严格按照以下顺序执行闭环验证，严禁直接草率结案：
1. detect_changes()：获取当前未提交的工作区变动符号集合，核验修改范围是否符合预期。
2. get_test_targets(ids="id1,id2")：精确定位受本次代码变更波及的单元测试集合（ids 参数为逗号分隔字符串；原生支持 Python 跨模块绝对导入依赖测试反查）。
3. check_guards(ids="id1,id2")：评估受波及符号是否触犯架构守护规则、分层违规或保护边界（ids 参数为逗号分隔字符串）。
4. verify_change(changes='[{"symbol_id":"sym_id","new_signature":"新签名"}]')：校验函数签名修改是否破坏下游调用方契约。
5. [*] change_contract(diff="...")：预测本次变动的整体风险等级，计算是否满足安全停止条件（固定 ack=false；若返回 risk_level="high"，必须进行显式风险确认）。
6. 真实测试执行：在宿主环境运行工程真实测试命令（如 go test, npm test, pytest 等）。若配置了 gortex githook（支持 post-commit, post-merge, post-checkout），可使用 --hook-timeout=30 开启 watchdog 超时保护防挂死。

6. 虚拟图层、状态机、事件订阅与持久化记忆

6.1 overlay 虚拟图层体系
Gortex 原生提供完备的内存级虚拟图层机制，允许在不污染物理磁盘的前提下推演、测试与对比方案：
- overlay_register：注册当前会话独立虚拟图层句柄。
- overlay_push：将未保存的内存 AST 变更推送到图层中。
- overlay_merge：将图层修改合入主图谱（同步落盘物理文件传 to_disk=true）。
- overlay_keepalive：为活动图层维持心跳租约，防止后台超时释放。
- overlay_switch：在多个并行探索的图层分支之间无缝切换。
- overlay_drop：丢弃指定图层中未合入的临时修改。
- overlay_drop_branch：丢弃指定命名的图层分支。
- overlay_fork：从当前图层状态派生新分支，支持多路线推演。
- overlay_delete：彻底销毁图层句柄及关联上下文。
- overlay_list：列出当前会话已挂载的所有虚拟图层文件。
- overlay_branches：枚举当前工程存在的所有图层分支。
- compare_branches(a="branch1", b="branch2", kind="get_callers", id="pkg/foo.go::Bar", depth=2, limit=50)：对比两个分支间的拓扑依赖差异。
- compare_with_overlay(kind="get_callers", id="pkg/foo.go::Bar", depth=2, limit=50)：对比未保存的内存修改与当前磁盘基线之间的图谱差异。
- simulate_chain(steps="[...]", keep=false)：在沙箱环境中推演多步连续修改对全局图谱的影响（固定 keep=false）。

6.2 记忆系统与决策笔记
1. store_memory(kind="invariant", title="标题", body="内容")：持久化记录跨会话的关键架构不变量或业务规约。
2. surface_memories(task="任务描述", symbol_ids="sym_id")：根据当前任务或涉及符号，智能召回历史沉淀的规约与决策（固定 mark_accessed=false）。
3. save_note(file_path="pkg/util.go", body="决策理由", tags="tag1,tag2")：为特定源码文件保存技术决策笔记（参数名严格为 file_path）。
4. query_notes(file_path="...", symbol_id="...")：按文件路径或符号 ID 检索绑定的决策笔记。
5. query_memories(query="...")：全文检索已保存的持久化架构记忆。
6. distill_session()：会话结束或阶段性任务达成时，自动提炼并固化本次会话沉淀的核心上下文。

6.3 会话控制与守护进程事件订阅
1. 基础会话控制：nav（虚拟导航游标）, agent_registry（多Agent协作调度加解锁）, set_planning_mode(enabled=true)（规划模式防写锁）, 图代理（proxy_enable, proxy_disable, proxy_status）。
2. 守护进程事件订阅系统（subscribe_event 与 unsubscribe_event）：
   针对 5 大关键系统事件进行实时订阅与告警监听：daemon_health（守护进程心跳）, diagnostics（语言服务诊断增量）, graph_invalidated（图谱缓存失效）, stale_refs（悬空引用告警）, workspace_readiness（多项目索引就绪度）。
3. tool_profile(tool="read_file")：实时诊断特定工具在当前会话的可用性状态（live, deferred, blocked, absent）。

6.4 代码审查与响应塑形
1. 综合代码审查工具集：
   review(scope="unstaged")（代码审查引擎）, diff_context(scope="unstaged")（提取 diff 审查拓扑）, review_pack（打包评审材料）, critique_review（反思审查有效性）, pr_review_context（装配 PR 全景审查拓扑）, suggested_review_questions（自动生成审查提问）, sibling_diff_context（兄弟分支对比）, post_review(pr=123, body="...")（提交结构化评审报告）。
2. 通用响应塑形规范：
   所有查询与分析类工具均支持统一的 Output 控制参数（顶层扁平传入）：
   - max_bytes：限制最大返回字节。
   - limit：限制最大返回条数。
   - format："gcx"（推荐，节省约 27% Token）、"toon"、"json"。
   - cursor：增量分页游标。遇到截断或超时时，使用 cursor 进行游标分页流转。
   - fields：稀疏列过滤投影。

7. 冲突裁决优先级与分支视图（View）治理

7.1 路径机制与分支视图治理
1. 平台路径规范与字符集规约：
   - Gortex 内部图谱 ID 严格采用正斜杠 `/`（如 "pkg/foo.go::Bar"），与操作系统物理分隔符彻底解耦。
   - 物理文件操作路径在 Windows 10 下底层自动进行路径规范化（filepath.Clean 与 filepath.ToSlash），透明兼容 `/` 与 `\`。为保证跨平台一致性，推荐优先使用标准工程相对路径或正斜杠绝对路径。
   - 严禁在符号 ID 或路径中包含 `<>` 等占位字符。
2. 多分支与工作树（View）治理：
   - 当前工作区由 session/CWD 决定视图，显式检出分支构成自动 Overlay。
   - 跨分支只读审计传 view={kind:"worktree",checkout_id:"..."} 或 view={kind:"git_ref",value:"refs/heads/release"}。
   - 强一致性查询传 require_exact=true 与 require_fresh=true。
   - 视图异常诊断：若视图解析异常或需查明路径归属，调用 explain_view(path="path/to/file") 诊断底层 checkout 路由链与代际 Generation（支持透过未绑定 CWD 穿透诊断）。

7.2 自顶向下裁决优先级
当规则、源码实现与运行时表现出现歧义时，严格按以下层级自顶向下裁决：
1. 当前工具返回的真实响应（error、completion、view、guard、effect 信息）。
2. 当前运行时 capabilities 与 Schema 规范（request_shape、fixed_arguments、available）。
3. 真实文件系统与代码仓库状态（workspace、index、repository 物理状态）。
4. 当前 Gortex 运行时源码实现。
5. 本规范文档。

多仓库全图谱最终铁律：已 track 仓库统一托管于全局图谱。严禁未经穿透检索断定未索引并退回原生工具。全流程（探索、定位、精读、修改、重构、验证）全程由 Gortex 原生 MCP 闭环，严禁读写割裂与调用宿主工具。严禁跨客户端规则漂移，以当前 Core 扁平工具集为准。仅当 query_project 与路径均证实未 track 且用户明确要求本地检查时，方可报告未 track。
