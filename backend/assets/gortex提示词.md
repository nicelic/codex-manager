【语法说明】命令示例中的 "|" 纯属文档排版的候选间隔说明，绝非命令参数值！实际执行时严禁原样照抄 "|"，必须且仅能选择传入单一合法值。带 "|" 的合并项，各操作专属必填字段不同，调用时须按具体 operation 契约填充。

1. 最高原则和使用边界

Gortex 是对其 track 仓库全生命周期代码定位、精读、分析、编辑、重构、验证与记忆的权威工具。

- 原生 MCP 绝对闭环：已 track 仓库的一切操作必须且仅能通过 Gortex 原生 MCP 闭环执行。
- 严禁读写割裂与降级：严禁阅读用 Gortex、修改退回宿主工具；严禁调用宿主工具（replace_file_content、write_to_file、grep_search、find_by_name 等）。报错或无法满足时须停用向用户说明，严禁退回宿主工具兜底。未 track 项目不得擅自图谱化。
- 全局技能只读豁免：read 放行全局技能只读（~/.agents/skills/、~/.claude/skills/ 及 Pi 包生态）。
- 严禁凭记忆捏造：严禁伪造未返回的 symbol、关系、view、索引状态或安全结论。
- 关键实现精读与 Token 瘦身：摘要/搜索不能替代关键实现阅读；行为关键代码必须精读完整源码。长代码巡检使用 compress_bodies=true（存根化省 60%~70% Token，keep="f1,f2" 保留关键源码），配合 format="gcx" 达到最佳瘦身。UTF-16 源文件 read 自动解码呈现，写工具硬拦截拒绝写入。
- 唯一法定写契约：edit、refactor（Core 模式: edit_file、batch_edit、write_file、edit_symbol）是唯一法定落地途径。写操作严格遵守当前 schema、guard、view、overlay 与 partial failure，严禁绕过状态机与图谱同步。
- 平台工具例外豁免：若用户明确指令“使用原生工具”时方可使用平台工具，避免频繁切换导致图谱失步。

1.1 核心工具集协议规范

支持双运行范式：默认客户端接入为「Facade-v1 紧凑门面模式」（21 个面向效果的领域工具）；配置 GORTEX_TOOLS=core/full 时切换为「Core 扁平离散模式」（34 个核心热工具，其余归入 Deferred Catalog）。协议规范：
1. ToolName 契约：
   - 默认 Facade-v1：命令名为 21 个领域动词（explore, search, read, relations, trace, analyze, ask, change, edit, refactor, review, publish_review, pr, recall, remember, workspace, workspace_admin, overlay, session, response, capabilities）。调用旧版扁平工具会被底层拦截；通过 operation 参数派发。
   - Core 扁平模式：命令名纯小写下划线标识符（如 read_file, edit_file, get_callers），严禁虚构。
2. 容器分层契约（Container Boundaries）：
   - 通用门面工具扩展参数归入 options（limit, path, depth, glob）、source（changes, steps, scope）、target（file, symbol, symbols, query）、to（symbol, query）、context（context_lines, compress_bodies）、output（format, max_bytes）；严禁随意平铺至根层级触发 additionalProperties: false 拒绝。
   - 冷门面强制 arguments 容器：remember, recall, workspace, workspace_admin, overlay, pr, publish_review, session 的业务参数必须且仅能封装在 arguments: { ... } 对象内（顶层仅接收 operation, arguments, output, view 及 session 的 channel）。
3. 真实参数值：符号 id 必为 "相对路径::符号名"（如 "pkg/foo.go::Bar"）或全局唯一短名，正斜杠分隔，严禁尖括号占位；文件 path 必为具体工程相对或绝对路径，严禁尖括号占位。
4. 固定参数（fixed_arguments）：search.symbols 服务端固定 assist="off"（无需传参）；change.contract 固定 ack=false（确认风险走 remember(risk_ack)）；simulate_chain / change.simulate 固定 keep=false；surface_memories / recall.surface 固定 mark_accessed=false；overlay_merge 默认 to_disk=false（落盘走 edit.apply_overlay 或显式传 to_disk=true）。
5. 统一 Output 控制：max_bytes（限字节）、limit/cursor（分页）、format（推荐 "gcx" 专有编码省 27% Token）、fields（稀疏投影）、scope（如 "unstaged"）。截断工具统一支持 _truncated_by_limit=true、count_is_exact:false 及 truncation_note 披露。

2. 任务开启与探索流转

流转准则与核心工具：
1. 首读契约：用户指定文件阅读/审查时，首个动作直接调用 read(operation="file", target:{file:"具体路径"}, options:{new_user_task:true})（Core: read_file）；严禁触发 localize 定位流程。
2. 代码定位与探索状态机：
   - 位置未知："where is X" 时调用 explore(operation="localize", task="...")。遵循 completion.required_action 状态机：
     * answer_ready：结果完备，直接从 completion.final_response 结案并停用工具。
     * needs_exact_read：补齐 read(operation="source", target:{symbol:"..."}) 精读核心实现体。
     * needs_more_context：缩小 task 范围或传入 path 补充线索。
   - 诊断排障或代码变更：调用 explore(operation="task", task="...")。探索后最多仅对 1 个未决符号追问一次（search, read, relations, trace），随后推进到 change(impact) -> edit -> verify 阶段。严禁退回原生 Grep/Glob。
3. 智能上下文装配：优先 explore(operation="context", task="任务描述", options:{entry_point:"入口符号", fidelity:"graded", token_budget:8000})（Core: smart_context）装配最小完备上下文与多文件编辑规划。
4. 架构速览辅助：explore(operation="outline") 获取主干入口与骨架；explore(operation="closure", options:{symbols:"id1,id2"}) 拓扑闭包；explore(operation="plan", task="...") 推荐开局序列；explore(operation="wakeup") 生成 500 Token 架构摘要。

2.1 检索、阅读与图谱协同
- search(operation="text", query="...", options:{limit:100, path:"..."})（Core: search_text）：Trigram 全文搜索，带 symbol_id/name。path 前置过滤；若 _truncated_by_limit=true，count 为下界 floor，可追加 path 收窄或扩大 limit。
- search(operation="symbols", query="...", options:{kind:"function", flavor:"struct"})（Core: search_symbols）：BM25 驼峰检索 AST 定义，服务端固定 assist="off"。
- search(operation="ast", query="模式表达式", options:{detector:"检测器名", language:"go"})（Core: search_ast）：语法级检索，支持 15+ 缺陷安全检测器及 Tree-sitter S 表达式。
- search(operation="files", query="...", options:{glob:"*.*"})（Core: find_files）：按路径前缀或 Glob 通配查找物理文件。
- 关系追踪（relations 与 trace；Core: get_callers, get_call_chain, find_usages, find_implementations）：沿 AST 追踪调用链、实现与引用点。原生支持 Python 第一方绝对导入（杜绝 dep:: 存根断裂）及 TS/JS 别名、Go 模块导入。

3. 原生独立核心工具调用全景字典

3.1 核心工具调用速查（Facade-v1 21 领域门面；Core 模式映射：read.file->read_file, edit.file->edit_file, search.symbols->search_symbols 等）

严守属性校验，入参严格匹配容器与属性名（注："|" 仅为备选说明）：
1. explore(operation="localize"|"task", task="...", options:{token_budget:1600})
2. explore(operation="context", task="...", options:{entry_point:"...", token_budget:8000})
3. explore(operation="outline")
4. analyze(kind="architecture")（鉴别参必须为 kind，支持 kind="help" 导出字典）
5. search(operation="symbols", query="...", options:{kind:"...", flavor:"..."})
6. search(operation="text", query="...", options:{limit:100})
7. search(operation="ast", query="模式", options:{detector:"检测器", language:"go"})
8. search(operation="files", query="...", options:{glob:"*.*"})
9. search(operation="winnow", query="...", options:{limit:50, min_fan_in:2})
10. read(operation="file", target:{file:"path"}, options:{offset:1, limit:100}, context:{compress_bodies:false})
11. read(operation="source", target:{symbol:"pkg/foo.go::Bar"}, context:{context_lines:0})
12. read(operation="symbols", target:{symbols:["id1","id2"]})
13. read(operation="summary"|"editing_context", target:{file:"path"}, options:{keep:"fn1"})
14. relations(operation="callers"|"dependencies"|"dependents", target:{symbol:"id"}, options:{depth:2})
15. relations(operation="usages", target:{symbol:"id"}, options:{context:"call"})
16. relations(operation="implementations"|"hierarchy"|"overrides", target:{symbol:"id"})
17. relations(operation="declaration", target:{query:"pos"})
18. trace(operation="call_chain", target:{symbol:"id"}, options:{depth:4})
19. trace(operation="cfg", target:{symbol:"id"})
20. trace(operation="path"|"flow", target:{symbol:"src"}, to:{symbol:"dst"}, options:{max_depth:8})
21. trace(operation="taint", target:{query:"src"}, to:{query:"sink"})
22. edit(operation="file"|"symbol", target:{file:"path", symbol:"id"}, match="旧", replacement="新", dry_run:false)
23. edit(operation="write", target:{file:"path"}, content="全量文本", dry_run:false)
24. edit(operation="batch", changes:[{"op":"edit_file","path":"..","old_string":"..","new_string":".."}], dry_run:true)
25. edit(operation="apply_overlay")
26. refactor(operation="rename", target:{symbol:"id"}, new_name="NewName", dry_run:true)
27. refactor(operation="move", target:{symbol:"id"}, destination="path/dst.go", dry_run:true)
28. refactor(operation="delete"|"inline", target:{symbol:"id"}, dry_run:true)
29. change(operation="impact", target:{symbol:"id"})
30. change(operation="verify", source:{changes:[{"symbol_id":"sym_id","new_signature":"sig"}]})
31. change(operation="detect")
32. change(operation="tests"|"guards", target:{symbols:["id1","id2"]})
33. change(operation="contract", source:{scope:"unstaged"})
34. change(operation="simulate", source:{steps:"[...]"})
35. change(operation="diagnostics", source:{file:"path"})
36. change(operation="compare_branches", target:{symbol:"id"}, options:{a:"b1", b:"b2", kind:"get_callers"})
37. change(operation="compare_overlay", target:{symbol:"id"}, options:{kind:"get_callers"})
38. review(operation="diff_context"|"run"|"pr_context", options:{scope:"unstaged"})
39. publish_review(operation="post", arguments:{pr:123, body:"..."})
40. pr(operation="list"|"impact"|"risk"|"reviewers", arguments:{pr:123})
41. recall(operation="surface", arguments:{task:"任务描述", symbol_ids:"id1,id2"})
42. recall(operation="distill")
43. recall(operation="notes"|"memories", arguments:{symbol_id:"id", text:"关键词"})
44. remember(operation="note", arguments:{file_path:"path", body:"理由", pinned:true})
45. remember(operation="memory", arguments:{kind:"invariant", title:"标题", body:"内容", symbol_ids:"id", importance:5})
46. remember(operation="risk_ack", arguments:{source:"symbols", symbols:"id"})
47. workspace(operation="project", arguments:{project:"target", query:"query"})
48. workspace(operation="repos"|"graph"|"index"|"checkouts")
49. workspace_admin(operation="track", arguments:{path:"abs/path"})
50. workspace_admin(operation="untrack"|"blame"|"coverage"|"sql_rebuild", arguments:{path:"..."})
51. overlay(operation="register"|"switch"|"fork"|"drop"|"delete")
52. overlay(operation="push", arguments:{path:"path", content:"代码"})
53. session(operation="planning_mode"|"cursor")
54. session(operation="subscribe"|"unsubscribe", channel="diagnostics")
55. capabilities(domain="read", operation="file", detail="schema")

3.2 多项目联合工作区与检索自适应规范

单守护进程多仓库接入统一联合知识图谱（默认 workspace: default）：
1. 单项目自动箝位：search、explore 默认锁定主场仓库。相对路径由底层自动箝位至当前工作区，杜绝跨库与外围 worktree 歧义拦截。
2. 跨项目穿透机制：跨库文件读写直接传绝对路径；跨库符号检索调用 workspace(operation="project", arguments:{project:"target", query:"..."})（Core: query_project），全图搜索传 repo="*"；关系工具全域贯通，Python 绝对导入自动标记 CrossRepo 跨仓边。
3. 规范铁律：set_active_project 仅接受逻辑 Project 标识，严禁传路径。已 track 项目一切操作必须在 Gortex 原生工具链内闭环。

4. 源码精读、关系分析与架构全景深度指南

4.1 源码阅读契约与 Token 优化策略
1. read(operation="file", target:{file:"path"}, options:{offset:1, limit:100})（Core: read_file）：首读支持 options:{new_user_task:true}。扁平模式仅传 path, offset, limit，禁传 line_range。长文件巡检必开 compress_bodies=true（存根化省 60%~70% Token），配合 keep="fn1,fn2" 保留关键源码。
2. read(operation="source", target:{symbol:"pkg/file.go::Symbol"}, context:{context_lines:0})（Core: get_symbol_source）：精读实现体，传 context_lines 扩展外围行数。
3. read(operation="summary", target:{file:"path"})（Core: get_file_summary）：快速获取顶级符号大纲、类型分布与导入依赖。
4. read(operation="editing_context", target:{file:"path"}, options:{keep:"fn1"})（Core: get_editing_context）：修改前装配文件拓扑、相邻符号、引用分布与波及范围。
5. ETag 缓存复查：read、explore(context) 均返回 etag 并支持 options:{if_none_match:"etag"}。若未变动返回 not_modified（~0 Token 开销）。

4.2 拓扑关系与深度数据流追踪
1. relations(operation="callers", target:{symbol:"id"}, options:{depth:2})（Core: get_callers）：反向递归查询调用者，获取位置、深度与语法上下文。
2. trace(operation="call_chain", target:{symbol:"id"}, options:{depth:4})（Core: get_call_chain）：递归追踪深度调用链路。
3. relations(operation="usages", target:{symbol:"id"}, options:{context:"call"})（Core: find_usages）：定位真实使用点（filter: call, read, write, import）。
4. relations(operation="implementations"|"overrides", target:{symbol:"id"})：定位接口实现类与基类虚函数重写。
5. relations(operation="dependencies"|"dependents", target:{symbol:"id"})：前向与反向依赖评估，计算重构影响面。
6. trace(operation="cfg", target:{symbol:"id"}) 与 trace(operation="path", target:{symbol:"src"}, to:{symbol:"dst"})：深入函数控制流（CFG），或计算两节点间最短依赖/调用路径。
7. Python 绝对导入支持：支持 import a.b as m 与 from a.b import f，穿透点分命名空间精准落地 AST 节点，彻底杜绝虚假 dep:: 存根断裂。

4.3 analyze 统一分析全景（78 种 kind 与 Admin 安全治理）
analyze 必传 kind="..."（kind="help" 导字典），只读分析：
1. 拓扑与边界：cycles（成环）、would_create_cycle、clusters、suggest_boundaries、hotspots（热点）、components、pagerank。
2. 健康与技术债：ownership（责任人，遇 absent/partial 运行 gortex enrich blame）、dead_code（Pydantic 验证器免误报）、coverage_gaps、doc_staleness、todos、health_score。
3. 并发与安全：race_writes、channel_ops、goroutine_spawns、sast（190+ 跨语言安全规则）、hygiene、unsafe_patterns、routes、models。
4. 禁令与写治理：blame, coverage, sql_rebuild, temporal_verify 涉及落盘，analyze 严禁执行，必须由 workspace_admin 执行。
5. 范围收窄：todos, dead_code, hotspots 等支持 options:{repo:"..."} 收窄；不支持的返回 scope_note。

5. 代码修改、重构与全生命周期闭环验证

5.1 代码修改唯一法定入口与写入契约
已 track 仓库文件必须经由 Gortex 原生写工具修改，严禁调用宿主 replace_file_content、write_to_file，防图谱与磁盘失步。

核心契约与安全门禁：
1. dry_run 与 physical_evidence 互斥：预览传 dry_run:true，落地传 dry_run:false, options:{physical_evidence:true}。
2. 语法门禁与安全：默认开启 AST 校验，草稿写入传 options:{allow_parse_errors:true}；支持 guard:{base_sha:"..."} 防脏写；UTF-16 源文件写操作底层硬拦截拒绝（须外部转码 UTF-8）；scaffold/safe_delete 自动保持原文件权限模式。
3. 标准化调用模板：
   - 文件局部替换：edit(operation="file", target:{file:"pkg/a.go"}, match="旧", replacement="新", dry_run:false)（Core: edit_file，传 expected_occurrences=1 防误覆盖）。
   - 符号精确修改：edit(operation="symbol", target:{symbol:"pkg/a.go::Bar"}, match="旧", replacement="新", dry_run:false)（Core: edit_symbol）。
   - 全量覆盖写入：edit(operation="write", target:{file:"pkg/a.go"}, content="全量内容", dry_run:false)（Core: write_file）。
   - 批量事务修改（edit(batch) / batch_edit）：原子执行多文件/符号混合变更，单项失败整批回滚。支持 4 种 op：edit_file（支持 replace_all:true）、edit_symbol、move_file、delete_file。
     edit(operation="batch", changes:[{"op":"edit_file","path":"pkg/a.go","old_string":"旧","new_string":"新"},{"op":"move_file","source":"pkg/old.go","destination":"pkg/new.go"},{"op":"delete_file","path":"pkg/del.go"}], dry_run:true)
   - 符号重构：refactor(operation="rename", target:{symbol:"id"}, new_name="NewName", dry_run:true)；refactor(operation="move", target:{symbol:"id"}, destination="pkg/b.go", dry_run:true)；refactor(operation="delete"|"inline", target:{symbol:"id"}, dry_run:true)。
4. 幂等与重试规范：内置 3 秒指数退避重试。若写入因共享冲突报错，切勿盲目重复调用，优先 change(operation="detect") 核验状态。未重新阅读最新内容前严禁重写。

5.2 修改后全栈闭环验证
修改后严格按序闭环验证，严禁草率结案：
1. change(operation="detect")（Core: detect_changes()）：核验未提交改动符号范围。
2. change(operation="tests", target:{symbols:["id1","id2"]})（Core: get_test_targets）：定位受波及单测。
3. change(operation="guards", target:{symbols:["id1","id2"]})（Core: check_guards）：评估是否触犯架构守护规则。
4. change(operation="verify", source:{changes:[{"symbol_id":"sym_id","new_signature":"新签名"}]})（Core: verify_change）：校验函数签名契约。
5. change(operation="contract", source:{scope:"unstaged"})（Core: change_contract）：预测变动风险（固定 ack=false；risk_level="high" 须由 remember(operation="risk_ack", arguments:{source:"symbols", symbols:"id"}) 显式确认）。
6. 真实测试执行：宿主运行测试命令（go test / npm test / cargo test / pytest）。配置 githook 可用 --hook-timeout=30 防挂死。

6. 虚拟图层、状态机与持久化记忆

6.1 overlay 虚拟图层体系
在不污染物理磁盘前提下推演与对比方案：
- 图层管理：overlay(operation="register"|"switch"|"fork"|"drop"|"delete")。
- 图层推送：overlay(operation="push", arguments:{path:"文件路径", content:"修改代码"})。
- 图层合入与落盘：overlay(operation="merge") 默认 to_disk=false（仅合入主图谱内存）；持久化落盘必须由 edit(operation="apply_overlay") 执行（Core: overlay_merge(to_disk=true)）。
- 比对与推演：change(operation="compare_branches", target:{symbol:"id"}, options:{a:"b1", b:"b2", kind:"get_callers"}) 比对分支依赖差异；change(operation="compare_overlay", target:{symbol:"id"}, options:{kind:"get_callers"}) 比对内存修改与基线差异；change(operation="simulate", source:{steps:"[...]"}) 沙箱推演连续修改影响（固定 keep=false，保留推演图层传 overlay(operation="simulate")）。

6.2 记忆系统与决策笔记
行为关键机制，须在对应触发点执行：
1. 会话初始与压缩后：启动或 Compaction 后首动执行 recall(operation="distill")（Core: distill_session()），提前建立上下文。
2. 上下文装配后召回：在 context 后立即执行 recall(operation="surface", arguments:{task:"任务描述", symbol_ids:"id1,id2"})（Core: surface_memories，固定 mark_accessed=false），召回规约约束。
3. 产生决策即时保存：否定方案或触及非显式约束时，调用 remember(operation="note", arguments:{file_path:"pkg/u.go", body:"理由", pinned:true})（Core: save_note）。正文提及符号 ID 自动关联图谱。
4. 固化持久规约：需团队继承的事实，调用 remember(operation="memory", arguments:{kind:"invariant", title:"标题", body:"内容", symbol_ids:"id", importance:5})（Core: store_memory）。废弃旧规约传 arguments:{supersedes:"旧ID"}。
5. 修改前复查：编辑高危符号前，调阅 recall(operation="notes"|"memories", arguments:{symbol_id:"id", text:"关键词"}) 历史预警。

6.3 代码审查与响应塑形
1. 代码审查工具集：review(operation="run", options:{scope:"unstaged"})（审查引擎）、review(operation="diff_context", options:{scope:"unstaged"})（diff 拓扑）、review(operation="pr_context")（PR 审查拓扑）、publish_review(operation="post", arguments:{pr:123, body:"..."})（提交评审报告）。
2. 响应塑形规范：全查询/分析工具支持 max_bytes、limit/cursor、format（推荐 "gcx"）、fields 稀疏列投影。

7. 冲突裁决优先级与分支视图（View）治理

7.1 路径机制与分支视图治理
1. 平台路径：图谱 ID 严格用正斜杠 /（如 "pkg/foo.go::Bar"）；物理路径底层自动规范化，兼容 / 与 \。严禁尖括号占位符。
2. 分支路由与 Worktree：
   - 自动 Overlay 禁重复 track：由 session/CWD 决定视图，linked worktree 均为自动 Overlay，严禁重复 track 或重启 daemon。
   - 拓扑语义：选定 overlay + Git 家族精确指定单一 primary；同名逻辑标识以 overlay 为准，tombstone 遮盖基线。
   - 降级只读：Freshness exact:false 时 fallback 视图一律只读！强一致传 require_exact:true，等待文件系统传 require_fresh:true。
   - 静态快照不可变：Inactive ref/commit 视图为不可变快照，不支持 LSP 与编辑。
   - 视图诊断：路由异常调用 workspace(operation="checkouts") 或 explain_view(path="path/to/file") 诊断 Generation。

7.2 自顶向下裁决优先级
当规则、源码实现与运行时表现出现歧义时，严格按以下层级自顶向下裁决：
1. 当前工具返回真实响应（error、completion、view、guard、effect 信息）。
2. 当前运行时 capabilities 与 Schema 规范（request_shape、fixed_arguments、available）。
3. 真实文件系统与代码仓库状态（workspace、index、repository 物理状态）。
4. 当前 Gortex 运行时源码实现。
5. 本规范文档。

多仓库全图谱终极铁律：已 track 仓库统一托管于全局图谱。严禁未经穿透检索断定未索引并退回原生工具。全流程由 Gortex 原生 MCP 闭环，严禁读写割裂与调用宿主工具。严禁跨客户端规则漂移，以当前环境暴露的工具集为准。仅当 query_project 与路径均证实未 track 且用户明确要求时方可报告未 track。