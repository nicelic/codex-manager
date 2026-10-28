【语法说明】命令示例中的 "|" 纯属文档排版的候选间隔说明，绝非命令参数值！实际执行时严禁原样照抄 "|"，必须且仅能选择传入单一合法值。带 "|" 的合并项，各操作专属必填字段不同，调用时须按具体 operation 契约填充。

1. 最高原则和使用边界

Gortex 是对其 track 仓库全生命周期代码定位、精读、分析、编辑、重构、验证与记忆的权威工具。

- 原生 MCP 绝对闭环：已 track 仓库的一切操作必须且仅能通过 Gortex 原生 MCP 闭环执行。
- 严禁读写割裂与降级：严禁阅读用 Gortex、修改退回宿主工具；严禁调用宿主工具（replace_file_content、write_to_file、grep_search、find_by_name 等）。报错或无法满足时须停用向用户说明，严禁退回宿主工具兜底。未 track 项目不得擅自图谱化。
- 全局技能只读豁免：read 放行全局技能只读（~/.agents/skills/*/SKILL.md、~/.config/opencode/skills/、~/.copilot/skills/、~/.claude/skills/ 及 Pi 包生态）。
- 严禁凭记忆捏造：严禁伪造未返回的 symbol、关系、view、索引状态或安全结论。
- 关键实现精读与 Token 瘦身：摘要/搜索不能替代关键实现阅读；行为关键代码必须精读完整源码。长代码巡检使用 compress_bodies=true（存根化省 60%~70% Token，keep="f1,f2" 保留关键源码），配合 format="gcx" 达到最佳瘦身。UTF-16 源文件 read 自动解码呈现，写工具硬拦截拒绝写入。
- 唯一法定写契约：edit、refactor 是唯一法定落地途径。写操作严格遵守当前 schema、guard、view、overlay 与 partial failure，严禁绕过状态机与图谱同步。
- 全生命周期高吞吐与单轮全量交付铁律：严禁“边走边改、逐处轮询、试探推演”的切香肠式低效交互。接收任务后必须全局通盘规划，严禁以“修改仅 1 处”为借口退回单入单出：查（explore.context 全局装配）、读（read.symbols 聚合多符号）、析（trace.path 单次找全链路）、写（edit.batch 聚合全部变更点单轮提交）、验（change.tests/guards 聚合多符号并发验证）。单任务原则上仅允许 1 次批量写交互，彻底消除偷懒轮询。
- 平台工具例外豁免：若用户明确指令“使用原生工具”时方可使用平台工具，避免频繁切换导致图谱失步。

1.1 核心工具集协议规范

支持双运行范式：默认客户端接入为「Facade-v1 紧凑门面模式」（21 个面向效果的领域工具）；配置 GORTEX_TOOLS=core/full 时切换为「Core 扁平离散模式」（34 个核心热工具，其余归入 Deferred Catalog）。协议规范：
1. ToolName 契约：
   - 默认 Facade-v1：命令名为 21 个领域动词（explore, search, read, relations, trace, analyze, ask, change, edit, refactor, review, publish_review, pr, recall, remember, workspace, workspace_admin, overlay, session, response, capabilities）。调用旧版扁平工具会被底层拦截；通过 operation 参数派发。
   - Core 扁平模式：命令名纯小写下划线标识符（如 read_file, edit_file, get_callers），严禁虚构。
2. 容器分层契约（Container Boundaries）：
   - 通用门面参数归入 options、source、target（file, symbol, symbols, query, artifact, repo 互斥单选）、to、context、output（format, max_bytes）；严禁平铺或随意注入 repo/project（触发 no_repository_selector 拦截）。
   - 冷门面强制 arguments 容器：remember, recall, workspace, workspace_admin, overlay, pr, publish_review, session, response 的业务参数必须且仅能封装在 arguments: { ... } 对象内（顶层仅接收 operation, arguments, output, view 及 session 的 channel）。
3. 真实参数值：符号 id 必为 "相对路径::符号名"（如 "pkg/foo.go::Bar"）或全局唯一短名，正斜杠分隔，严禁尖括号占位；文件 path 必为具体工程相对或绝对路径。
4. 固定参数（fixed_arguments）：search.symbols 固定 assist="off"；explore.localize 固定 localize=true；change.contract 固定 ack=false（确认走 remember.risk_ack）；change.simulate 固定 keep=false；edit.wiki 固定 enhance=false；recall.surface 固定 mark_accessed=false；overlay.merge 默认 to_disk=false（落盘走 edit.apply_overlay 显式固定 to_disk=true）。
5. 统一 Output 控制：max_bytes（限字节）、limit/cursor（分页）、format（推荐 "gcx" 专有编码省 27% Token）、fields（稀疏投影）、scope（如 "unstaged"）。截断工具统一支持 _truncated_by_limit=true、count_is_exact:false 及 truncation_note 披露。

2. 任务开启与探索流转

流转准则与核心工具：
1. 首动契约：用户指定明确文件阅读时首动直接调用 read(operation="file", target:{file:"具体路径"}, options:{new_user_task:true})（Core: read_file）；若首动为排查探索则 explore(options:{new_user_task:true})。严禁无端触发定位流程。
2. 代码定位与探索状态机：
   - 位置未知："where is X" 时调用 explore(operation="localize", task="...")。遵循 completion.required_action 状态机：
     * answer_ready：结果完备，直接从 completion.final_response 结案并停用工具。
     * needs_exact_read：补齐 read(operation="source", target:{symbol:"..."}) 精读核心实现体。
     * needs_more_context：缩小 task 范围或传入 path 补充线索。
   - 诊断排障或代码变更：调用 explore(operation="task", task="...")。探索后最多仅对 1 个未决符号追问一次（search, read, relations, trace），随后推进到 change(verify/impact) -> edit -> verify 阶段。严禁退回原生 Grep/Glob。
3. 智能上下文装配：复杂或多文件任务强制首选 explore(operation="context", task="任务描述", options:{entry_point:"入口符号", fidelity:"graded", token_budget:8000}) 单次装配最小完备上下文与跨文件 Edit Plan，替代 5-10 轮工具搜索往返。
4. 架构速览辅助：explore(operation="outline") 获取主干入口与骨架；explore(operation="closure", options:{symbols:"id1,id2"}) 拓扑闭包；explore(operation="plan", task="...") 推荐开局序列；explore(operation="wakeup") 生成 500 Token 架构摘要；explore(operation="suggest"|"prefetch") 辅助探索预热。

2.1 检索、阅读与图谱协同
- search(operation="text", query="...", options:{limit:100, path:"..."})（Core: search_text）：Trigram 全文搜索，带 symbol_id/name。path 前置过滤；若 _truncated_by_limit=true，count 为下界 floor，可追加 path 收窄或扩大 limit。
- search(operation="symbols", query="...", options:{kind:"function", flavor:"struct"})（Core: search_symbols）：BM25 驼峰检索 AST 定义，服务端固定 assist="off"。
- search(operation="ast", query="模式表达式", options:{detector:"检测器名", language:"go"})（Core: search_ast）：语法级检索，支持 15+ 缺陷安全检测器及 Tree-sitter S 表达式。
- search(operation="winnow", query="...", options:{limit:50, min_fan_in:2})（Core: search_winnow）：拓扑剪枝检索，按调用扇入/扇出中心度自动过滤噪声，严禁人肉翻找海量低优文本结果。
- search(operation="files"|"artifacts", query="...", options:{glob:"*.*"})：查找物理源码文件或架构资产。
- 关系追踪（relations 与 trace）：沿 AST 追踪调用链与引用点。原生支持 Python 第一方绝对导入及 TS/JS 别名、Go 模块。

3. 原生独立核心工具调用全景字典

3.1 核心工具调用速查（Facade-v1 21 领域门面）

严守属性校验，入参严格匹配容器与属性名（注："|" 仅为备选说明）：
1. explore(operation="localize"|"task", task="...", options:{new_user_task:true, token_budget:1600})
2. explore(operation="context", task="...", options:{entry_point:"...", token_budget:8000})
3. explore(operation="outline"|"plan"|"wakeup"|"suggest")
4. ask(question="深度代码研判描述")（LLM 深度研究门面）
5. analyze(kind="architecture")（鉴别参必须为 kind，支持 kind="help" 导出字典）
6. search(operation="symbols", query="...", options:{kind:"...", flavor:"..."})
7. search(operation="text", query="...", options:{limit:100})
8. search(operation="ast", query="模式", options:{detector:"检测器", language:"go"})
9. search(operation="files"|"artifacts"|"winnow", query="...", options:{glob:"*.*", limit:50})
10. read(operation="file", target:{file:"path"}, options:{offset:1, limit:100}, context:{compress_bodies:false})
11. read(operation="source", target:{symbol:"pkg/foo.go::Bar"}, context:{context_lines:0})
12. read(operation="symbols", target:{symbols:["id1","id2"]})（多符号批量精读，替代多次 read.source 轮询）
13. read(operation="summary"|"editing_context", target:{file:"path"}, options:{keep:"fn1,fn2"})
14. read(operation="history"|"artifact", target:{symbol:"id", artifact:"id"})
15. relations(operation="callers"|"dependencies"|"dependents"|"usages", target:{symbol:"id"}, options:{depth:2, context:"call"})
16. relations(operation="hierarchy"|"implementations"|"overrides"|"references", target:{symbol:"id"})（单次查询全继承树/引用点，禁逐级轮询）
17. trace(operation="path"|"flow", target:{symbol:"src"}, to:{symbol:"dst"}, options:{max_depth:8})（单次计算最短调用/数据流链路，禁多轮拼凑）
18. trace(operation="call_chain"|"cfg"|"taint", target:{symbol:"id", query:"src"}, to:{query:"sink"}, options:{depth:4})
20. edit(operation="batch", changes:[{"op":"edit_file","path":"..","old_string":"旧","new_string":"新"}], dry_run:false, options:{physical_evidence:true})（日常代码修改唯一默认入口）
21. edit(operation="write", target:{file:"path"}, content="全量文本", dry_run:false)（全新文件生成或全量重写）
22. edit(operation="file"|"symbol", target:{file:"path", symbol:"id"}, match="旧", replacement="新", dry_run:false)（底层应急兜底，日常禁用）
23. edit(operation="apply_overlay"|"scaffold"|"docs"|"skill"|"wiki"|"export_graph")
24. refactor(operation="rename"|"move"|"delete"|"inline"|"apply_code_action"|"fix_all", target:{symbol:"id", file:"path"}, dry_run:true)
25. change(operation="verify", source:{changes:[{"symbol_id":"sym_id","new_signature":"sig"}]})
26. change(operation="receipt"|"detect")（核验落盘凭据 mutation_status / 变动符号范围）
27. change(operation="impact"|"tests"|"guards", target:{symbols:["id1","id2"]})（聚合变动符号单次并发校验）
28. change(operation="contract"|"simulate", source:{scope:"unstaged", steps:"[...]"})
29. change(operation="code_actions"|"edit_plan"|"preview"|"diagnostics", target:{file:"path"}, source:{file:"path", steps:"[...]"})
30. change(operation="compare_branches"|"compare_overlay", target:{symbol:"id"}, options:{kind:"get_callers"})
31. review(operation="diff_context"|"run"|"pr_context"|"pack"|"critique", options:{scope:"unstaged"})
32. publish_review(operation="post", arguments:{pr:123, body:"..."})
33. pr(operation="list"|"impact"|"risk"|"triage"|"conflicts"|"reviewers", arguments:{pr:123})
34. recall(operation="surface"|"distill", arguments:{task:"任务描述", symbol_ids:"id1,id2"})
35. recall(operation="notes"|"memories"|"notebook_find"|"notebook_list"|"notebook_show", arguments:{symbol_id:"id", text:"关键词"})
36. remember(operation="note"|"memory", arguments:{file_path:"path", kind:"invariant", title:"标题", body:"内容", symbol_ids:"id", importance:5})
37. remember(operation="risk_ack"|"notebook"|"edit_memory"|"rename_memory"|"suppress_finding", arguments:{...})
38. workspace(operation="project"|"repos"|"graph"|"index"|"checkouts"|"info"|"scopes"|"active_project"|"proxy", arguments:{project:"target", query:"query"})
39. workspace_admin(operation="track"|"untrack"|"set_active_project"|"blame"|"coverage"|"sql_rebuild"|"temporal_verify"|"index"|"reindex"|"save_scope"|"delete_scope"|"feedback", arguments:{...})
40. overlay(operation="push"|"register"|"switch"|"fork"|"drop"|"delete"|"keepalive"|"simulate"|"merge", arguments:{path:"..", content:".."})
41. session(operation="planning_mode"|"cursor"|"workflow"|"agents"|"subscribe"|"unsubscribe", channel="diagnostics")
42. response(operation="slice"|"grep"|"peek"|"stats", arguments:{offset:0, limit:50, pattern:".."})（截断缓冲区精准切片，禁盲目重查）
43. capabilities(domain="read", operation="file", detail="schema")

3.2 多项目联合工作区与检索自适应规范

单守护进程多仓库接入统一联合知识图谱（默认 workspace: default）：
1. 单项目自动箝位：默认锁定主场仓库。严禁向未声明 repo 属性的工具随意塞入 repo/project/cwd/workspace 字段（0.64.11 会直接以 no_repository_selector 拒绝调用）。
2. 跨项目穿透机制：跨库文件读写直接传绝对路径；跨库符号检索调用 workspace(operation="project", arguments:{project:"target", query:"..."})；全图搜索传 repo="*"；关系工具全域贯通，Python 绝对导入自动标记 CrossRepo 跨仓边。切换主场工程通过 workspace_admin(operation="set_active_project", arguments:{project:"name"})。
3. 规范铁律：set_active_project 仅接受逻辑 Project 标识，严禁传路径。已 track 项目一切操作必须在 Gortex 原生工具链内闭环。

4. 源码精读、关系分析与架构全景深度指南

4.1 源码阅读契约与 Token 优化策略
1. read(operation="file", target:{file:"path"}, options:{offset:1, limit:100})：首读支持 options:{new_user_task:true}。扁平模式仅传 path, offset, limit，禁传 line_range。长文件巡检必开 compress_bodies=true（存根化省 60%~70% Token），配合 keep="fn1,fn2" 保留关键源码。
2. read(operation="symbols", target:{symbols:["id1","id2"]})：多符号精读必须一次性聚合传入，严禁连续发起多次 read(source) 往返。单符号精读传 read(operation="source", target:{symbol:"pkg/file.go::Symbol"}, context:{context_lines:0})。
3. read(operation="summary", target:{file:"path"})：快速获取顶级符号大纲、类型分布与导入依赖。
4. read(operation="editing_context", target:{file:"path"}, options:{keep:"fn1,fn2"})：修改前装配文件拓扑与相邻符号，keep 传入多个目标函数展开完整源码。
5. read(operation="history", target:{symbol:"id"})：获取符号演化历史与提交记录。
6. ETag 缓存复查：read、explore(context) 均返回 etag 并支持 options:{if_none_match:"etag"}。未变动返回 not_modified（~0 Token 开销）。

4.2 拓扑关系与深度数据流追踪
1. relations(operation="callers"|"dependencies"|"dependents", target:{symbol:"id"}, options:{depth:2})：递归查询调用者与出入度依赖。
2. trace(operation="call_chain", target:{symbol:"id"}, options:{depth:4})：递归追踪深度调用链路。
3. relations(operation="usages", target:{symbol:"id"}, options:{context:"call"})：定位真实使用点（filter: call, read, write, import）。
4. relations(operation="hierarchy"|"implementations"|"overrides", target:{symbol:"id"})：单次聚合查询整棵继承/实现树，禁逐级单步轮询。
5. trace(operation="path", target:{symbol:"src"}, to:{symbol:"dst"}, options:{max_depth:8}) 与 trace(operation="cfg", target:{symbol:"id"})：两点间路径强制单次计算最短链路，严禁多次 callers 顺藤摸瓜式试探。
6. Python 绝对导入支持：支持 import a.b as m 与 from a.b import f，穿透点分命名空间精准落地 AST 节点，彻底杜绝虚假 dep:: 存根断裂。

4.3 analyze 统一分析全景（78 种 kind 与 Admin 安全治理）
analyze 必传 kind="..."（kind="help" 导字典），确定性引擎优先铁律：宏观排查强制调用 analyze，严禁逐文件肉眼搜索猜测：
1. 拓扑边界：cycles（成环）、would_create_cycle、clusters、suggest_boundaries、hotspots（热点）、components、pagerank、bottlenecks。
2. 健康债项：dead_code（死代码，Pydantic 免误报）、ownership（责任人）、coverage_gaps、doc_staleness、todos、health_score。
3. 并发安全：race_writes（数据竞争）、channel_ops、goroutine_spawns、sast（190+ 跨语言安全漏洞规则）、hygiene、unsafe_patterns。
4. 禁令与写治理：blame, coverage, sql_rebuild, temporal_verify 涉及落盘，analyze 严禁执行，必须由 workspace_admin 执行。
5. 范围收窄机制：21 种 AUTO 级（todos, ownership 等）及 Tier-2（dead_code, hotspots, cycles）支持 options:{repo:"..."} 收窄；其余无范围属性的分析显式附带 scope_note 披露。

5. 代码修改、重构与全生命周期闭环验证

5.1 代码修改唯一法定入口与写入契约
已 track 仓库文件必须经由 Gortex 原生写工具修改，严禁调用宿主 replace_file_content、write_to_file，防图谱与磁盘失步。

核心契约与安全门禁：
1. dry_run 与 physical_evidence 互斥：预览传 dry_run:true，落地传 dry_run:false, options:{physical_evidence:true}。
2. 语法门禁与安全：默认开启 AST 校验，草稿写入传 options:{allow_parse_errors:true}；支持 guard:{base_sha:"..."} 防脏写；UTF-16 源文件写操作底层硬拦截拒绝（须外部转码 UTF-8）；scaffold/safe_delete 自动保持原文件权限模式。
3. 唯一法定修改模式（Batch-Only 铁律）：
   - 唯一默认标准入口：日常业务代码修改（无论 1 处还是 N 处、单文件还是跨文件）强制且仅能使用 edit(operation="batch") 单次全量提交！严禁以“仅改 1 处”为由绕过批处理；严禁通盘规划缺失导致的试探性轮询。动笔前必须通盘查清全部关联修改点，统一打包入 changes 列表单次落盘。
     edit(operation="batch", changes:[{"op":"edit_file","path":"view.vue","old_string":"旧1","new_string":"新1"},{"op":"edit_file","path":"view.vue","old_string":"旧2","new_string":"新2"},{"op":"move_file","source":"old.go","destination":"new.go"},{"op":"delete_file","path":"del.go"}], dry_run:false, options:{physical_evidence:true})
     * 支持 4 种 op：edit_file（支持 replace_all:true）、edit_symbol、move_file 与 delete_file。支持 transaction_id 幂等防重放。
   - 全量覆盖写入：全新文件创建或整文件全量重写使用 edit(operation="write", target:{file:"pkg/a.go"}, content="全量内容", dry_run:false, options:{physical_evidence:true})。
   - 符号重构与动作：refactor(operation="rename"|"move"|"delete"|"inline", target:{symbol:"id"}, dry_run:true)；整文件代码修复传 refactor(operation="fix_all", target:{file:"path"}, dry_run:false, options:{physical_evidence:true})。
   - 降级兜底禁令：edit(operation="file"|"symbol") 仅作为底层发生未知 schema 故障时的极端降级备用途径，日常业务修改严禁作为首选调用！
4. 幂等与重试规范：内置 3 秒指数退避重试。若写入因共享冲突报错，切勿盲目重复调用，优先 change(operation="receipt") 与 change(operation="detect") 核验状态。未重新阅读最新内容前严禁重写。

5.2 修改后全栈闭环验证
修改后严格按序闭环验证，严禁草率结案：
1. change(operation="verify", source:{changes:[{"symbol_id":"s1","new_signature":"sig1"},{"symbol_id":"s2","new_signature":"sig2"}]})：涉及公共签名修改前，单次聚合预检所有变动符号的下游契约。
2. change(operation="receipt")：编辑落地后首选核查写入确认凭据（mutation_status），确认变更确实入库。
3. change(operation="detect")：核验未提交改动符号范围。
4. change(operation="tests", target:{symbols:["id1","id2"]}) 与 change(operation="guards", target:{symbols:["id1","id2"]}): 必须将变动符号聚合传入，单次往返并发定位受波及单测并检查架构守护规则，严禁逐个符号发起验证。
5. change(operation="contract", source:{scope:"unstaged"})：预测变动风险（固定 ack=false；risk_level="high" 须由 remember(operation="risk_ack", arguments:{source:"symbols", symbols:"id"}) 显式确认）。
6. 真实测试执行：宿主运行测试命令（go test / npm test / cargo test / pytest）。配置 githook 可用 --hook-timeout=30 防挂死。

6. 虚拟图层、状态机与持久化记忆

6.1 overlay 虚拟图层体系
复杂推演严禁向磁盘写脏代码，强制在内存图层推演：
- 图层生命周期：overlay(operation="push", arguments:{path:"..", content:".."}) 内存模拟变更；register/switch/fork/drop/delete/keepalive。
- 合入与落盘：overlay(operation="merge") 固定 to_disk=false（内存合入）；物理落盘由 edit(operation="apply_overlay") 执行（显式传 to_disk=true）。
- 比对推演：change(operation="compare_branches"|"compare_overlay", target:{symbol:"id"})；change(operation="simulate", source:{steps:"[...]"}) 沙箱推演。

6.2 记忆系统与决策笔记
行为关键机制，须在对应触发点执行：
1. 会话初始与压缩后：启动或 Compaction 后首动执行 recall(operation="distill")，提前建立上下文。
2. 上下文装配后召回：在 context 后立即执行 recall(operation="surface", arguments:{task:"任务描述", symbol_ids:"id1,id2"})（固定 mark_accessed=false），召回规约约束。
3. 产生决策即时保存：否定方案或触及非显式约束时，调用 remember(operation="note", arguments:{file_path:"pkg/u.go", body:"理由", pinned:true})。正文提及符号 ID 自动关联图谱。
4. 固化持久规约：需团队继承的事实，调用 remember(operation="memory", arguments:{kind:"invariant", title:"标题", body:"内容", symbol_ids:"id", importance:5})。废弃旧规约传 arguments:{supersedes:"旧ID"}。支持 remember(operation="notebook", arguments:{path:"...", body:"..."}) 沉淀提交级日志。
5. 修改前复查：编辑高危符号前，调阅 recall(operation="notes"|"memories", arguments:{symbol_id:"id", text:"关键词"}) 历史预警。

6.3 代码审查与响应塑形
1. 审查单轮全量交付：审查强制调用 review(operation="run"|"pack", options:{scope:"unstaged"}) 全量生成报告，禁多轮索要 diff。PR 研判调用 pr(operation="triage"|"conflicts", arguments:{pr:123}) 单次判定。
2. 截断切片铁律：输出触发 _truncated_by_limit=true 时禁盲目加大 limit 重查，强制使用 response(operation="slice", arguments:{offset:N, limit:M}) 按需切片或 response(operation="grep", arguments:{pattern:".."}) 提取。

7. 冲突裁决优先级与分支视图（View）治理

7.1 路径机制与分支视图治理
1. 平台路径：图谱 ID 严格用正斜杠 /（如 "pkg/foo.go::Bar"）；物理路径底层自动规范化，兼容 / 与 \。严禁尖括号占位符。
2. 分支路由与 Worktree：
   - 自动 Overlay 禁重复 track：由 session/CWD 决定视图，linked worktree 均为自动 Overlay，严禁重复 track 或重启 daemon。
   - 拓扑语义：选定 overlay + Git 家族精确指定单一 primary；同名逻辑标识以 overlay 为准，tombstone 遮盖基线。
   - 降级只读：Freshness exact:false 时 fallback 视图一律只读！强一致传 require_exact:true，等待文件系统传 require_fresh:true。
   - 静态快照不可变：Inactive ref/commit 视图为不可变快照，不支持 LSP 与编辑。
   - 视图诊断：路由异常调用 workspace(operation="checkouts") 或 Core: explain_view(path="path/to/file") 诊断 Generation。

7.2 自顶向下裁决优先级
当规则、源码实现与运行时表现出现歧义时，严格按以下层级自顶向下裁决：
1. 当前工具返回真实响应（error、completion、view、guard、effect 信息）。
2. 当前运行时 capabilities 与 Schema 规范（request_shape、fixed_arguments、available）。
3. 真实文件系统与代码仓库状态（workspace、index、repository 物理状态）。
4. 当前 Gortex 运行时源码实现。
5. 本规范文档。

多仓库全图谱终极铁律：已 track 仓库统一托管于全局图谱。严禁未经穿透检索断定未索引并退回原生工具。全流程由 Gortex 原生 MCP 闭环，严禁读写割裂与调用宿主工具。严禁跨客户端规则漂移，以当前环境暴露的工具集为准。仅当 query_project 与路径均证实未 track 且用户明确要求时方可报告未 track。