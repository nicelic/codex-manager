Gortex 使用规范

> 本规范面向使用 Gortex MCP 与 Agent 宿主适配器（Google Antigravity、Claude Code、Codex、Cursor 等）的代码 Agent。以 Gortex 0.64.2 运行时源码为权威依据。

1. 最高原则和使用边界

Gortex 是对其 track 仓库进行代码定位、源码阅读、关系分析、数据流追踪、影响评估、编辑、重构、验证、审查、项目管理和持久化记忆的权威工具。

- **优先原生 MCP 句柄与 Antigravity 宿主约束**：凡已 track 仓库，100% 优先通过 Gortex 原生 MCP 句柄执行。**严禁**因惰性调用的包装摩擦或单次未命中退回 Antigravity 原生 `find_by_name`、`grep_search`、`view_file`、PowerShell 或 shell。严禁擅自调用 CLI 或启停/重启 daemon；若 MCP 通道不可达，直接报告 Gortex MCP integration failure 并停止操作。
- **0.64.2 全局技能只读豁免**：`read_file`（或 `read.file`）默认受仓库根目录限制，但在 0.64.2 运行时中，特别放行对全局预装技能（Curated Skills）的只读读取。Agent 可直接读取 `~/.agents/skills/*/SKILL.md`、`~/.config/opencode/skills/*/SKILL.md`、`~/.copilot/skills/*/SKILL.md`、`~/.claude/skills/*/SKILL.md`，不会触发越界拦截。
- **严禁凭记忆捏造**：严禁发明 tool、operation、参数、字段、命令或关系。严禁伪造未返回的 symbol、关系、view、索引状态或安全结论。
- **关键实现体完整阅读与 Token 瘦身**：摘要和搜索结果不能替代关键实现阅读；行为关键代码不要压缩 body。数据库迁移、并发/锁、权限安全、文件写入、网络调用、状态机必须读完整源码。非核心长代码巡检时，可在 `read_file`、`get_symbol_source`、`get_editing_context` 中启用 `compress_bodies: true`（函数体存根化，削减 60%~70% Token，支持 `keep="f1,f2"` 保留指定函数）。
- **遵守写契约**：所有写操作遵守当前 schema、effect、fixed_arguments、guard、view、overlay 和 partial failure 结果。

1.1 协议名与双模感知（Core 扁平离散 vs Facade-v1 门面）

根据 MCP 连接预设不同，Gortex 暴露给当前会话的工具形态存在两种模式，**必须以当前 session inventory 为准自适应路由**：
1. **Core 扁平离散工具模式（Antigravity 默认预设）**：当前会话直接暴露 `read_file`, `get_symbol_source`, `get_callers`, `search_symbols`, `edit_file`, `batch_edit`, `query_project`, `distill_session` 等离散工具。参数扁平传入，严格遵守入参 schema（详见第 3 节对照表）。
2. **Facade-v1 紧凑门面模式（21 个顶级工具）**：当前会话暴露 `read`, `explore`, `search`, `relations`, `trace`, `edit`, `change`, `refactor`, `workspace` 等 21 个门面。严格使用门面语法并遵守 `operation` 与 `target` 单一选择器规范。

- **多项目图谱一致性铁律**：Gortex 守护进程统一维护所有已 track 仓库的全局知识图谱。严禁因 `list_repos` 仅列出当前工作区、`get_active_project` 返回其他工程或单次检索为 0 就断定目标未被索引。必须通过第 3.4 节的自适应无缝切换机制在 Gortex 内精准执行。

2. 任务开启与定位流转（Localize Workflow）

新任务按目标选择首调用，不要把所有任务都强制使用 `explore.task`：
- 已知具体文件且只需读取/审查：`read(operation="file", target={file:"<path>"}, options={new_user_task:true})`，或扁平模式 `read_file(path="<path>")`。
- 需要定位文件、符号或关键证据：`explore(operation="localize", task="<完整问题>", options={new_user_task:true})`，或扁平模式 `explore(task="<完整问题>")`。
- 需要诊断、多步实现或修复：`explore(operation="task", task="<完整任务、错误和约束>", options={new_user_task:true})`。
- 恢复历史会话决策上下文：扁平模式 `distill_session()`（门面：`recall(operation="distill")`）；查阅既有规约：`surface_memories(task="...")`（门面：`recall(operation="surface", arguments={task:"..."})`）。
- `new_user_task=true` 仅用于新请求第一次调用，分页、重试或后续精读严禁携带。

可用 explore operation（9个）：`closure`, `context`, `localize`, `outline`, `plan`, `prefetch`, `suggest`, `task`, `wakeup`。

2.1 localize 形状与参数限制
门面模式 task 必须在顶层：`explore(operation="localize", task="<完整问题>", options={"new_user_task":true})`。
- **严禁**写成 `explore(operation="localize", options={task:"..."})`（报错 `explore.localize requires task`）。
- **严禁**在 `explore(operation="task")` 传 `localize: true`（报错 `explore.task does not accept localize=true`）。
- 扁平模式直接调用 `explore(task="<完整问题>")`，可选传入 `path` 约束子目录、`token_budget` 控制上下文开销。

2.2 completion 状态流转
localize 返回终结状态字段：`answer_ready`（证据充足直接回答）、`localized`（定位完成展开修改/测试）、`needs_exact_read`（仅精读指定精确对象，若候选错误可用 read.file 命名跳出）、`needs_refinement`（按 allowed_symbols 缩小范围）、`needs_recovery`（有限恢复，上限2次）；以及 `refinement_in_flight`, `exact_read_in_flight`, `recovery_in_flight`。附带 `required_action`、`instruction`、`allowed_symbols`、`allowed_operations`、`exact_symbol`。completion 只约束定位收敛，不代表代码已修改或测试已通过。

3. 双模调用对照字典与参数防错规范

3.1 双模核心对照表（彻底纠正入参字段名）

在 Antigravity 默认的 Core 扁平离散模式下，各个工具开启严格属性校验（`additionalProperties: false`）。**严禁将 Facade 门面容器混入扁平调用**！特别注意：**所有符号与图谱工具（`get_symbol`, `get_symbol_source`, `get_callers`, `find_usages`, `find_implementations`, `get_dependencies`, `get_dependents`, `get_call_chain`）扁平入参属性名必须为 `id`，严禁传 `symbol`**！

| 功能场景 | Core 扁平工具 (Mode A) | Facade-v1 门面 (Mode B) | 防错要点 |
| :--- | :--- | :--- | :--- |
| **全局任务定位** | `explore(task="...")` | `explore(operation="localize"|"task", task="...")` | 顶层单任务入口 |
| **上下文智能分析** | `smart_context(task="...")` | `explore(operation="context", task="...")` | 支持 fidelity="graded" |
| **大纲结构** | `get_repo_outline()` | `explore(operation="outline")` | 工程全局骨架 |
| **一站式架构快照** | `get_architecture()` | `analyze(operation="architecture")` | 语言配比/社区/入口/分层 |
| **符号搜索** | `search_symbols(query="...", kind="...")` | `search(operation="symbols", query="...")` | 固定 assist=off |
| **文本全文搜索** | `search_text(query="...", regexp=false)` | `search(operation="text", query="...")` | 支持正则与路径过滤 |
| **查找文件** | `find_files(query="...", glob="*.*")` | `search(operation="files", query="...")` | 通配查找文件 |
| **AST 语法搜索** | `search_ast(pattern="...", detector="...")` | `search(operation="ast", query="...")` | 预制 detector 反模式审计 |
| **非代码资产搜索** | `search_artifacts(query="...", kind="...")` | `search(operation="artifacts", query="...")` | 搜 schema/api/infra/doc |
| **多轴约束过滤** | `winnow_symbols(text_match="...", kind="...")` | `search(operation="winnow", query="...")` | 门面 query 映射 text_match |
| **读取文件** | `read_file(path="...", offset=1, limit=100)` | `read(operation="file", target={file:"..."})` | 放行全局 skill 绝对路径 |
| **读取非代码资产** | `get_artifact(id="..." \| path="...")` | `read(operation="artifact", target={artifact:"..."})` | 读取完整设计规范与文档 |
| **读取符号元数据** | `get_symbol(id="<id>")` | `read(operation="source", target={symbol:"<id>"})` | 传规范 Node ID |
| **读取符号源码** | `get_symbol_source(id="<id>", context_lines=3)` | `read(operation="source", target={symbol:"<id>"})` | 完整函数/结构体实现 |
| **批量读取符号** | `batch_symbols(symbols=["id1", "id2"])` | `read(operation="symbols", target={symbols:[...]})` | 批量获取源码 |
| **文件符号概览** | `get_file_summary(file="<path>")` | `read(operation="summary", target={file:"<path>"})` | 单文件摘要 |
| **编辑前拓扑必调** | `get_editing_context(file="<path>")` | `read(operation="editing_context", target={file:"..."})`| 修改前必读依赖关系 |
| **反向调用者** | `get_callers(id="<id>", depth=2)` | `relations(operation="callers", target={symbol:"<id>"})`| 查引用调用者 |
| **符号引用点** | `find_usages(id="<id>", context="call")` | `relations(operation="usages", target={symbol:"<id>"})` | 支持 group_by="file" |
| **接口实现查找** | `find_implementations(id="<id>")` | `relations(operation="implementations", target={symbol:"..."})` | 查接口实现 |
| **前向依赖** | `get_dependencies(id="<id>")` | `relations(operation="dependencies", target={symbol:"<id>"})` | 依赖项列表 |
| **反向被依赖** | `get_dependents(id="<id>")` | `relations(operation="dependents", target={symbol:"<id>"})` | 评估爆炸半径 |
| **深度调用链** | `get_call_chain(id="<id>", depth=4)` | `trace(operation="call_chain", target={symbol:"<id>"})` | 递归调用路径 |
| **修改文件(预览)** | `edit_file(path="...", old_string="...", new_string="...", dry_run=true)` | `edit(operation="file", target={file:"..."}, dry_run=true)` | dry_run=true 禁带凭证 |
| **修改文件(写入)** | `edit_file(path="...", old_string="...", new_string="...", dry_run=false)`| `edit(operation="file", target={file:"..."}, dry_run=false)`| 支持 base_sha 校验 |
| **修改符号(预览)** | `edit_symbol(id="...", old_source="...", new_source="...", dry_run=true)` | `edit(operation="symbol", target={symbol:"..."}, dry_run=true)` | 精确修改 AST 节点 |
| **修改符号(写入)** | `edit_symbol(id="...", old_source="...", new_source="...", dry_run=false)`| `edit(operation="symbol", target={symbol:"..."}, dry_run=false)`| 支持 base_sha 校验 |
| **写入全文件** | `write_file(path="...", content="...")` | `edit(operation="write", target={file:"..."}, content="...")` | 全量覆盖写入 |
| **批量事务编辑** | `batch_edit(edits=[...], dry_run=true)` | `edit(operation="batch", changes=[...], dry_run=true)` | 扁平入参名为 edits |
| **安全重命名** | `rename_symbol(id="...", new_name="...", dry_run=true)` | `refactor(operation="rename", target={symbol:"..."}, dry_run=true)`| 全图引用联动重构 |
| **安全删除符号** | `safe_delete_symbol(id="...", dry_run=true)` | `refactor(operation="delete", target={symbol:"..."}, dry_run=true)`| 支持 propagate=true |
| **守护规则检查** | `check_guards(ids="id1,id2")` | `change(operation="guards", target={symbols:[...]})` | 逗号分隔 ids 字符串 |
| **单测波及定位** | `get_test_targets(ids="id1,id2")` | `change(operation="tests", target={symbols:[...]})` | 逗号分隔 ids 字符串 |
| **签名契约校验** | `verify_change(changes='[{"symbol_id":"...","new_signature":"..."}]')` | `change(operation="verify", source={changes:[...]})` | changes 数组/字符串 |
| **变更契约评估** | `change_contract(workspace_edit="..." \| diff="...")` | `change(operation="contract", source={...})` | 预测风险与停止条件 |
| **LSP 编辑模拟** | `preview_edit(workspace_edit="...")` | `change(operation="preview", source={workspace_edit:"..."})` | 专用于 WorkspaceEdit |
| **修改状态对账** | `mutation_status(receipt="..." \| mutation_id="...")` | `change(operation="receipt")` | 超时后核实真实落盘状态 |
| **链式编辑模拟** | `simulate_chain(steps="[...]", keep=false)` | `change(operation="simulate")` | 变更链路沙箱模拟 |
| **检测未提交改动**| `detect_changes()` | `change(operation="detect")` | 工作区脏变动检测 |
| **Diff 上下文** | `diff_context(scope="unstaged")` | `review(operation="diff_context", source={scope:"unstaged"})`| review 域变更上下文 |
| **代码综合审查** | `review(scope="unstaged")` | `review(operation="run", source={scope:"unstaged"})` | 图增强代码审查 |
| **存储架构记忆** | `store_memory(kind="invariant", title="...", body="...")` | `remember(operation="memory", arguments={...})` | 持久化规约记忆 |
| **添加代码笔记** | `save_note(file="...", body="...", tags=[...])` | `remember(operation="note", arguments={...})` | 局部决策笔记 |
| **召回规约记忆** | `surface_memories(task="...")` | `recall(operation="surface", arguments={task:"..."})` | 召回任务相关记忆 |
| **提炼会话摘要** | `distill_session()` | `recall(operation="distill")` | 恢复历史决策与摘要 |
| **按文件查笔记** | `query_notes(file="..." \| symbol_id="...")` | `recall(operation="notes", arguments={...})` | 查看符号/文件决策历史 |
| **项目免切穿透查询**| `query_project(project="...", query="...")` | `workspace(operation="project", project="...", query="...")` | 官方跨库穿透检索 |
| **切换活跃项目** | `set_active_project(project="...")` | `workspace_admin(operation="set_active_project", arguments={...})`| 动态热切换主项目 |
| **当前活跃工程** | `get_active_project()` | `workspace(operation="active_project")` | 获取 bound 与活跃工程 |
| **已索引仓库列表**| `list_repos()` | `workspace(operation="repos")` | 查看已 track 仓库 |
| **工作区元数据** | `workspace_info()` | `workspace(operation="info")` | 工作区路径与图谱全景 |
| **知识图谱统计** | `graph_stats()` | `workspace(operation="graph")` | 节点、边与容量统计 |
| **索引健康检查** | `index_health()` | `workspace(operation="index")` | 检查索引与守护进程健康度 |

3.2 参数容器与 arguments 陷阱（极其重要）

- **冷门面**（`publish_review`, `pr`, `recall`, `remember`, `workspace`, `workspace_admin`, `overlay`, `response`）及 `session` 在 InputSchema 中显式声明了 `arguments` 属性，其操作参数必须封装在 `arguments: {...}` 中。
- **热门面**（`explore`, `search`, `read`, `relations`, `trace`, `analyze`, `ask`, `change`, `review`, `edit`, `refactor`）未声明 arguments 属性！**严禁**在外层包裹 `arguments: {...}`，否则触发硬报错：`arguments is an unexpected top-level key ... arguments is the JSON-RPC envelope, not a parameter`。参数必须直接放顶层或对应容器（`target` / `options` / `guard` / `source` / `context`）。

3.3 固定参数（fixed_arguments）全景

以下参数由运行时强制固化，调用者传参也会被系统安全覆盖：
- `search.symbols`: 固定 `assist=off`（保证本地确定性搜索）。
- `analyze.co_change`: 固定 `refresh=false`（读取预热缓存，不启动异步 git 挖掘）。
- `change.contract`: 固定 `ack=false`（保持只读咨询状态；持久化确认必须走 `remember.risk_ack`）。
- `change.simulate`: 固定 `keep=false`（只读模拟，不持久化到 overlay）。
- `edit.wiki`: 固定 `enhance=false`（禁止本地写边界私自调用大模型）。
- `edit.apply_overlay`: 固定 `to_disk=true`（合并 overlay 并真实写入磁盘）。
- `recall.surface`: 固定 `mark_accessed=false`（纯读模式，不污染频次访问计数器）。
- `overlay.simulate`: 固定 `keep=true`（模拟并持久化为 overlay 虚拟图层）。
- `overlay.merge`: 固定 `to_disk=false`（仅合并到 overlay 会话缓冲，不写磁盘）。
- `remember.risk_ack`: 固定 `ack=true`（确认已知破坏性改动）。
- `workspace_admin.{blame,coverage,sql_rebuild,temporal_verify}`: 固定 `kind=<name>`。

3.4 多项目自适应与无感无缝切换机制（核心合一规范）

在单守护进程多仓库架构下，Gortex Daemon 在后台统一维护所有已 track 项目的完整图谱。客户端（如 Google Antigravity）常驻单一 MCP 会话长连接，可能受启动环境固化在初始工作区（`bound: true`）。为达成“用户零提示词口令、Agent 自主感知、底层无缝流转”的极致体验，Agent 必须执行以下双轨无感切换规范：

1. **自动目标对齐（零口令全自动感知）**：
   - **环境自动感知**：每次交互，Agent 必须直接从宿主注入的上下文（如 Antigravity 的 `<user_information>` 中的 `active workspaces` 根路径）提取当前活动工程名。在新项目中发起提问时，绝不需要用户额外说明“切换项目”或“我现在在项目 D”，Agent 自动以当前工作区工程为第一核心展开。
   - **跨库引用自动识别**：在工程 A 的分析中，若用户明确提问关联工程 B，或代码中出现对已 track 库 B 的 import/依赖调用，Agent 自动感知项目 B 并纳入分析范围。

2. **双轨自适应执行机制（击穿 bound 隔离与无感切域）**：
   - **轨道 A：会话级静默热切换（Session Silent Hot-Switch，主场聚焦模式）**：
     - **适用场景**：用户直接提问新项目（如在项目 C 对话后直接提问项目 D）、或需要对目标工程执行深度调用拓扑分析（`explore`、`get_callers`、`get_call_chain`、`find_usages`）、多文件连续修改（`edit_file`、`batch_edit`）、变更守护与测试闭环（`check_guards`、`detect_changes`、`get_test_targets`）。
     - **执行指令**：Agent 后台首个动作静默调用：
       - Core 扁平模式：`set_active_project(project="<目标项目>")`
       - Facade 门面模式：`workspace_admin(operation="set_active_project", arguments={project:"<目标项目>"})`
     - **无感保障**：该调用为 Daemon 内部内存指针重定向（耗时通常 <5ms），在后台静默完成，严禁暂停打扰用户或向用户请求确认。切换后，目标工程的全局图谱、AST 语法树、变更守护全面解锁，彻底规避 `symbol not found`。
   - **轨道 B：跨项目单次轻量穿透（Cross-Project One-Shot Lookup，免切交叉模式）**：
     - **适用场景**：在主工程 A 分析中，仅需临时查阅公共库 B 的接口定义、配置文件、或跨库文本比对，无需打乱工程 A 当前会话环境。
     - **执行指令**：
       - **跨库符号检索**：Core 模式调用 `query_project(project="<目标>", query="...")`；门面模式调用 `workspace(operation="project", project="<目标>", query="...")`。
       - **跨库物理文件阅读**：`read_file(path="<Repo前缀>/path/to/file")`。
       - **跨库全文与文件检索**：`search_text(query="...", repo="<Repo前缀>")`；查找文件使用 `find_files(query="<Repo前缀>/...")`。
     - **零状态震荡**：穿透接口完全只读且不改动会话作用域，主副项目交叉分析零串扰。

3. **穿透精读与分析（严禁混淆 Path 与 Symbol ID）**：
   - **文件路径（Path）**：`read_file`、`get_editing_context`、`edit_file`、`write_file` 涉及文件系统的入参，跨库时**必须显式补齐仓库前缀**，如 `read_file(path="<Repo前缀>/path/to/file")`。
   - **符号标识（Node ID）**：`get_symbol`、`get_symbol_source`、`get_callers`、`find_usages`、`get_call_chain`、`edit_symbol` 必须直接使用 `query_project` 或搜索返回的真实全局 `id`，**严禁**在已有全局 ID 前二次拼接项目名！若调用提示 `symbol not found`，即代表该符号超出当前会话 bound 范围，必须立即走【轨道 A】静默切换主工程。

4. **严禁擅自退回原生工具**：
   严禁以 `list_repos` 仅显示当前单库、`search_text` 裸搜无结果或未收到切换指令为由退回宿主原生工具（`find_by_name`、`grep_search`、`view_file` 或终端 shell）。只要仓库已被 Daemon track，必须在 Gortex 内部通过“轨道 A 静默切域”或“轨道 B 穿透调用”闭环解决。

4. 检索、阅读与拓扑分析

4.1 search 7 大操作与高级约束过滤
可用操作：`artifacts`, `ast`, `completion`, `files`, `symbols`, `text`, `winnow`。
- `symbols`, `text`, `completion` 必须提供非空 `query`，否则报错 `search.<op> requires query`。
- `ast` 的 `query` 自动映射至 `pattern`。
- **`winnow`（多轴结构化约束检索）**：结合 BM25 文本与图属性精确筛选候选集。支持字段：`text_match`（门面传 `query`）、`kind`（function, method, struct）、`language`、`path_prefix`、`community`、`min_fan_in`、`min_fan_out`、`min_churn`、`is_test`、`limit`（默认 20）。

4.2 read 7 大操作与单一选择器规则
可用操作：`artifact`, `editing_context`, `file`, `history`, `source`, `summary`, `symbols`。
- **Target 选择器单一性铁律**：`target` 必须为对象且**有且仅能有一个键**（`file`, `symbol`, `symbols`, `query`, `artifact`, `repo` 选 1 个）。传空 `{}`、多键 `{file:"...", symbol:"..."}` 均报错 `target must contain exactly one selector`；传未知键报错 `unknown target selector`；单数 `symbol` 传数组报错；批量查询必须用 `symbols: [...]`。
- `read.file` 支持 `options={offset:1, limit:100}` 或 `context={start_line:1, end_line:100}`。
- 在 `git_ref` 或 `commit` 虚拟视图下，严禁请求 `physical_evidence: true`。

4.3 relations (11个) 与 trace (7个)
- **relations（11个）**：`callers`, `cluster`, `declaration`, `dependencies`, `dependents`, `hierarchy`, `implementations`, `import_path`, `overrides`, `references`, `usages`。调用：`relations(operation="callers", target={symbol:"..."})`。
- **trace（7个）**：`call_chain`, `cfg`, `flow`, `graph`, `path`, `taint`, `walk`。调用：`trace(operation="call_chain", target={symbol:"..."})`。`flow`, `path`, `taint` 必须同时提供源 `target` 与目标 `to`，遵守单一选择器规范。

4.4 analyze 只读统一分析与核心 Kind 全景
`analyze` 内置 78 种分析 kind，核心速查：
- **架构拓扑**：`architecture`（分层与依赖边界）、`cycles`（循环依赖）、`components`/`clusters`（连通分量与聚合簇）。
- **质量债务**：`dead_code`（死代码）、`untested`（未测符号）、`coverage_gaps`（覆盖盲区）、`hotspots`（高频热点）、`clones`（重复代码）、`churn`（修改抖动）、`todos`（待办标记）。
- **并发与语言**：`race_writes`（多协程竞态）、`channel_ops`/`unclosed_channels`/`goroutine_spawns`（Go协程/管道泄漏）、`cgo_users`（Cgo边界调用）。
- **安全合规**：`sast`/`hygiene`（安全隐患与代码坏味道）、`unsafe_patterns`（高危 API）、`routes`（已注册路由）。
- **变动类强制阻断**：`blame`, `coverage`, `sql_rebuild`, `temporal_verify` 会持久化图或调外部模型，在 `analyze` 下会被硬拦截，必须通过 `workspace_admin(operation="<kind>")` 执行。

5. 修改、重构与验证

5.1 修改前评估：change 19 大操作
可用操作：`api_impact`, `code_actions`, `compare_branches`, `compare_overlay`, `contract`, `detect`, `diagnostics`, `edit_plan`, `guards`, `impact`, `overlay_branches`, `overlay_state`, `pattern`, `preview`, `ranges`, `receipt`, `simulate`, `tests`, `verify`。
- 修改前建议通过 `change(operation="impact", target={symbol:"<id>"})`（扁平模式 `explain_change_impact`）评估爆炸半径。
- 修改函数签名或公共接口前，必须通过 `change(operation="verify", source={changes:[{symbol_id:"<id>", new_signature:"<sig>"}]})`（扁平模式 `verify_change`）校验破坏性。
- `remember(operation="risk_ack")` 仅在存在待确认的变动符号时调用。

5.2 edit (10个) 与 refactor (6个) 两阶段修改铁律
可用 edit 操作：`apply_overlay`, `batch`, `docs`, `export_graph`, `file`, `scaffold`, `skill`, `symbol`, `wiki`, `write`。
可用 refactor 操作：`apply_code_action`, `delete`, `fix_all`, `inline`, `move`, `rename`。

【两阶段修改与参数冲突核心防线】
1. **`dry_run: true` 与 `physical_evidence: true` 严格互斥！** 源码断言：`physical_evidence requires a real write; dry_run leaves no disk bytes to attest`。预览必须 `dry_run: true`（禁带 `physical_evidence`）；正式写入必须 `dry_run: false` 才可开启 `physical_evidence: true`。
2. **0.64.2 语法门禁（Parse Gate）与防并发脏写**：默认开启语法校验，若改动导致新增 Tree-sitter 语法错误将被硬拦截；写入草稿片段必须显式传入 `allow_parse_errors: true`。传入 `base_sha` 可校验磁盘版本，防范并发幽灵覆盖。
3. **`expected_occurrences` 仅用于文件编辑**：仅在 `edit_file` 生效，数量不符拒绝写入；`edit_symbol` 基于 AST 节点，不接受此字段。
4. **前后内容相同拒绝**：`old_string == new_string` 或 `old_source == new_source` 会被硬拒绝。
5. **语言支持边界**：`refactor.move`（`move_symbol`）和 `refactor.inline`（`inline_symbol`）目前仅支持 Go 源码。
6. **0.64.2 edit.skill (generate_skill) 安全硬约束**：`skill_name` 必须为单路径组件（严格限制 `[a-zA-Z0-9-_.]`），严禁包含路径分隔符、`..` 或卷名；Frontmatter 标量自动转义 `\r`, `\n`, `\t`。

【标准化调用代码模板】
- **普通文本/文件修改**：
  - 预览：`edit_file(path="<file>", old_string="<old>", new_string="<new>", dry_run=true, expected_occurrences=1)`
    *门面：`edit(operation="file", target={file:"<file>"}, match="<old>", replacement="<new>", dry_run=true, guard={expected_occurrences:1})`*
  - 写入：`edit_file(path="<file>", old_string="<old>", new_string="<new>", dry_run=false, physical_evidence=true)`
    *门面：`edit(operation="file", target={file:"<file>"}, match="<old>", replacement="<new>", dry_run=false, options={physical_evidence:true})`*
- **精准符号修改**：
  - 预览：`edit_symbol(id="<id>", old_source="<old>", new_source="<new>", dry_run=true)`
  - 写入：`edit_symbol(id="<id>", old_source="<old>", new_source="<new>", dry_run=false, physical_evidence=true)`
- **事务型批量修改（原子提交）**：
  `batch_edit(dry_run=true, edits=[{"op":"edit_file","path":"<f1>","old_string":"<o>","new_string":"<n>"},{"op":"edit_symbol","id":"<id>","old_source":"<o>","new_source":"<n>"},{"op":"move_file","source":"<s>","destination":"<d>","expected_sha256":"<sha>"},{"op":"delete_file","path":"<p>","expected_sha256":"<sha>"}])`
  *门面：`edit(operation="batch", dry_run=true, changes=[...])`。单快照执行，任一失败整体回滚。*
- **安全符号重命名与删除**：
  - 重命名预览：`rename_symbol(id="<id>", new_name="<new_name>", dry_run=true)`
  - 安全删除：`safe_delete_symbol(id="<id>", dry_run=true, propagate=true)`（门面：`refactor(operation="delete", target={symbol:"<id>"}, dry_run=true)`）

5.3 修改后闭环验证
代码修改后必须执行闭环校验链路：
1. `detect_changes()`（门面：`change(operation="detect")`）：获取未提交变动符号集合。
2. `get_test_targets(ids="id1,id2")`（门面：`change(operation="tests", target={symbols:[...]})`）：定位受波及单测目标列表（不代表测试已运行）。
3. `check_guards(ids="id1,id2")`（门面：`change(operation="guards", target={symbols:[...]})`）：评估防护规则与越界依赖。
4. **真实测试执行**：在宿主环境实际运行测试构建命令（`go test`, `npm test`, `pytest` 等），捕获真实输出并汇报。

6. 状态机、隔离层与持久化记忆

6.1 overlay（10个）虚拟图层控制
操作列表：`delete`, `drop`, `drop_branch`, `fork`, `keepalive`, `merge`, `push`, `register`, `simulate`, `switch`。
- 注册与推送：`overlay(operation="register")`；`overlay(operation="push", arguments={branch:"main"})`。
- 内存合并与续期：`overlay(operation="merge", arguments={branch:"main"})`（固定 `to_disk=false`）；`overlay(operation="keepalive")`。
- 图层持久化落盘：必须调用 `edit(operation="apply_overlay")`（固定 `to_disk=true`）。

6.2 recall (8个) 与 remember (8个) 记忆系统
- **recall（只读检索）**：`distill`, `memories`, `notebook_find`, `notebook_list`, `notebook_show`, `notes`, `onboarding`, `surface`。
  - 恢复会话记忆与决策摘要：`distill_session()`（门面：`recall(operation="distill")`）。
  - 召回架构决策记忆：`surface_memories(task="...")`（门面：`recall(operation="surface", arguments={task:"..."})`，固定 `mark_accessed=false`）。
  - 按文件/符号查笔记：`query_notes(file="..." \| symbol_id="...")`（门面：`recall(operation="notes", arguments={file:"..."})`）。
  - 查询跨会话持久规约：`query_memories(query="...")`（门面：`recall(operation="memories", arguments={...})`）。
- **remember（本地持久化写入）**：`edit_memory`, `memory`, `note`, `notebook`, `notebook_used`, `rename_memory`, `risk_ack`, `suppress_finding`。
  - 持久化关键不变量：`store_memory(kind="invariant", title="...", body="...")`（门面：`remember(operation="memory", arguments={kind:"...", title:"...", body:"..."})`）。
  - 记录代码决策笔记：`save_note(file="...", body="...", tags=["decision"])`；确认契约风险：`remember(operation="risk_ack")`（固定 `ack=true`）。

6.3 session（8个）会话控制与事件订阅
操作列表：`agents`, `cursor`, `planning_mode`, `proxy_disable`, `proxy_enable`, `subscribe`, `unsubscribe`, `workflow`。
- 控制协同 Agent 状态机：`session(operation="agents", arguments={action:"list|register|heartbeat|lock|unlock|unregister"})`。
- 开启/关闭规划模式：`session(operation="planning_mode", arguments={enabled:true})`。
- 订阅后台事件通知：`session(operation="subscribe", channel="<channel>", arguments={min_severity:1})`。
- **合法 Channel 仅限 5 个**：`daemon_health`, `diagnostics`, `graph_invalidated`, `stale_refs`, `workspace_readiness`。

6.4 review (7个), pr (6个), response (5个)
- **review 审查**：`review(scope="unstaged")`（门面：`review(operation="run", source={scope:"unstaged"})`）；审查 diff 上下文：`diff_context(scope="unstaged")`（门面：`review(operation="diff_context", source={diff:"..."})`）。
- **pr 审查拉取**：`pr(operation="list")`，`pr(operation="impact", arguments={pr:123})`，`pr(operation="conflicts")`。
- **publish_review 外部发布**：`publish_review(operation="post", arguments={pr:123, body:"...", confirm_public:false})`。
- **response 缓冲区切片**：超大响应裁剪：`response(operation="slice", arguments={start:1, end:50})`；`response(operation="grep", arguments={pattern:"error"})`；`response(operation="export_context", arguments={task:"..."})`。

7. 通用响应控制与冲突裁决

7.1 通用 Output 响应塑形参数
门面与查询类扁平工具支持统一 `output` 控制对象：`output.max_bytes`（限制最大字节）、`output.limit`（限制最大条数）、`output.format`（`json`、`gcx`、`toon`）、`output.cursor`（增量分页游标）。

7.2 冲突裁决优先级
自顶向下裁决：
1. **当前工具返回的真实响应**（error、completion、view、guard、effect 信息）。
2. **当前运行时 capabilities / Schema 规范**（request_shape、fixed_arguments、available）。
3. **真实文件系统与代码仓库状态**（workspace、index、repository 物理状态）。
4. **当前 Gortex 0.64.2 运行时源码实现**。
5. **本规范文档**。

**多仓库全图谱最终铁律**：已 track 仓库全部统一托管于全局图谱。严禁在未经跨项目穿透检索或静默切域的情况下断定“未索引”并退回原生工具。只有在 `query_project` 与带前缀路径均证实未 track，且用户明确要求本地文件检查时，方可报告未 track 状态。
