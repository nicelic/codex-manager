Gortex 使用规范

> 本规范面向使用 Gortex MCP、Gortex CLI 和 Agent host adapter（Google Antigravity、Claude Code、Codex、Cursor 等）的代码 Agent。以 Gortex 运行时源码为权威依据。

1. 最高原则和使用边界

Gortex 是对其 track 仓库进行代码定位、源码阅读、关系分析、数据流追踪、影响评估、编辑、重构、验证、审查、项目管理和持久化记忆的权威工具。

- 对 indexed repository，优先使用当前 host 提供的 Gortex 原生 MCP handle。
- **Antigravity 宿主约束**：在 Google Antigravity 中，Gortex 通常作为 Lazy MCP 挂载。**严禁**因为惰性调用的包装摩擦或单次检索未直接命中而退回使用 Antigravity 原生的 `find_by_name`、`grep_search`、`view_file`、PowerShell 或 shell 替代 Gortex 的索引搜索、符号搜索、关系、影响、编辑、重构、guard 或 contract。凡已 track 的仓库，必须第一优先级通过 Gortex MCP 句柄执行。
- 不要凭记忆发明 tool、operation、参数、字段、preset、命令或关系。
- 不要伪造 Gortex 没有返回的文件、symbol、关系、view、索引状态、测试结果或安全结论。
- 摘要、搜索结果和关系图只能缩小范围，不能替代关键实现体阅读；行为关键代码不要压缩 body。
- 数据库迁移、重试/回退、并发/锁、权限/安全、文件写入、网络调用、事务和状态机必须读取完整实现。
- 所有写操作遵守当前 schema、effect、fixed_arguments、guard、view、overlay 和 partial failure 结果。

1.1 协议名与双模感知（Facade-v1 vs 55 Core 扁平）

协议名与宿主 callable tool 名因 MCP 预设不同而存在两种暴露形态，**必须以当前 session inventory 为准自适应路由**：
1. **Facade-v1 紧凑门面（21 个顶级工具）**：当当前会话暴露的是 `read`, `explore`, `search`, `relations`, `trace`, `edit` 等时，严格使用门面语法并遵守 `operation` 与 `target` 单一选择器规范。
2. **Core 扁平工具（55 个离散工具，如 Antigravity 默认预设）**：当当前会话暴露的是 `read_file`, `get_symbol`, `get_callers`, `search_symbols`, `edit_file` 等时，直接调用具体工具名，参数扁平传入（对应关系详见第 3.3 节）。

如果宿主已配置 Gortex MCP，但当前会话没有原生 handle：
1. 报告 Gortex MCP integration failure。
2. 停止当前 indexed-code 操作。
3. 不要手动启动 daemon。
4. 不要自动切到 gortex call、CLI、PowerShell 或 shell。

daemon 不可达时，不要假装图分析完成；用户明确要求诊断时才可使用 gortex version、gortex doctor --json、gortex status、gortex daemon status、gortex daemon logs。不要因为 daemon 故障自动 start、track、reindex、untrack。

- **多项目图谱一致性铁律**：Gortex 守护进程在后台统一维护所有已 track 仓库的完整全局图谱。严禁因 `list_repos` 仅列出单一仓库、`get_active_project` 返回其他工程或单次检索结果为 0，就主观断定目标工程未被索引！**严禁擅自退回宿主原生工具**。若 Gortex 错误信息包含宿主自身程序安装目录等 IDE 环境路径，此为工作区未对齐的伪报错，绝不代表目标未索引，必须通过第 3.2 节的双穿透机制在 Gortex 内精准执行。

2. 任务开始和 localize

开始 Gortex 任务先确认：

workspace(operation="info")
workspace(operation="active_project")
workspace(operation="repos")
workspace(operation="index")
capabilities()
*注：在 55 扁平工具模式下，使用 graph_stats 与 index_health 确认图谱与索引健康度；跨项目操作直接使用带项目前缀的路径（如 read_file(path="<目标项目>/path/to/file")）；若需调用 get_active_project / set_active_project 等高阶工具，可先通过 tools_search(query="project") 动态激活。*

新任务按目标选择首调用，不要把所有任务都强制用 explore.task：

read(operation="file", target={file:"<path>"}, options={new_user_task:true})
explore(operation="localize", task="<完整问题>", options={new_user_task:true})
explore(operation="task", task="<完整任务、错误和约束>", options={new_user_task:true})
recall(operation="distill")
*注：在 55 扁平工具模式下，对应调用 `read_file(path="...")` 或 `explore(task="...")`。*

- 已知文件且只是读取、总结、审查：read.file。
- 需要找文件、symbol、调用点或证据：explore.localize。
- 需要诊断、实现、修改或继续任务：explore.task。
- recall.distill 用于恢复上下文；recall.surface 按需查询既有决策/约束。
- new_user_task=true 只用于新请求第一次调用，不用于分页、重试或后续读取。

可用 explore operation（9个）：`closure`, `context`, `localize`, `outline`, `plan`, `prefetch`, `suggest`, `task`, `wakeup`。

2.1 localize 形状与参数限制

当前运行时要求 task 必须在顶层：

explore(
  operation="localize",
  task="<完整问题>",
  options={new_user_task:true}
)

- **严禁**写成 `explore(operation="localize", options={task:"..."})`（运行时会报错 `explore.localize requires task`）。
- **严禁**在 `explore(operation="task")` 中传入 `localize: true`，运行时会硬拒绝报错 `explore.task does not accept localize=true`。

2.2 completion 状态流转

localize 可能返回：
`needs_exact_read`, `needs_refinement`, `needs_recovery`, `localized`, `answer_ready`, `refinement_in_flight`, `exact_read_in_flight`, `recovery_in_flight`。

并带有 required_action、instruction、final_response、allowed_tool_calls、allowed_symbols、allowed_operations、exact_symbol 等字段。

- answer_ready：依据 final_response 和证据回答，停止导航。
- localized：定位结束，可以继续诊断、实现、修改、测试或回答。
- needs_exact_read：只读契约指定的精确对象（若候选错误，可用 `read.file` 命名文件跳出）。
- needs_refinement：按 allowed symbols/operations 缩小范围。
- needs_recovery：只执行工具返回的有限 recovery（recovery allowance 上限为 2，不能无限重试）。
- completion 只约束定位后续，不等于代码已修改或测试已运行。

3. workspace、view、Facade 与 55 工具映射

3.1 workspace/view

可用 workspace operation（只读，9个）：
`active_project`, `checkouts`, `graph`, `index`, `info`, `project`, `proxy`, `repos`, `scopes`。

可用 workspace_admin operation（控制写入，14个）：
`blame`, `coverage`, `delete_scope`, `enrich_churn`, `enrich_releases`, `feedback`, `index`, `reindex`, `save_scope`, `set_active_project`, `sql_rebuild`, `temporal_verify`, `track`, `untrack`。

通用 view：
view={kind:"auto"}
view={kind:"base", graph_id:"<graph-id>"}
view={kind:"worktree", checkout_id:"<checkout-id>"}
view={kind:"worktree", path:"<absolute-path>"}
view={kind:"git_ref", value:"refs/heads/<branch>"}
view={kind:"commit", value:"<full-lowercase-object-id>"}

- exact=false 是 fallback，永远只读。
- git_ref、commit 和 fallback view 不可写，**且不可请求 physical_evidence**。
- 只有 exact 且 coordinator-backed 的可写 worktree 才支持 mutation。
- 不要把 fallback、immutable view 或 explain/preview 结果写成已写入。

3.2 多项目自适应与动态感知机制（核心合一规范）

在单守护进程多仓库架构下，后台 Daemon 同时完整索引所有已 track 项目。MCP 客户端通常为宿主创建的单一常驻长连接，启动时可能被环境固化了初始绑定（`bound: true`）。为彻底解决项目切换问题，Agent 必须遵循以下合一感知与穿透规则：

1. **自动目标对齐（零口令感知，无需显式切换）**：
   - **独立项目提问场景**：每次对话，Agent 必须直接从宿主上下文（Active Workspace 根路径）自动提取当前项目名作为主目标。用户在新项目中发起提问时，绝不需要额外说明“切换项目”，Agent 自动以该工程为核心展开分析。
   - **跨项目交叉分析场景**：在项目 A 的分析中，若需要涉及项目 B（用户直接提问项目 B 的功能，或代码中存在跨库依赖），Agent 自动识别出项目 B，无需显式执行切换会话或切换 active_project，直接并行接入。

2. **双穿透合一规则（突破 bound: true 会话隔离）**：
   当当前会话与目标项目不一致（或 `bound: true` 锁定在另一项目）时，严禁使用会受 bound 作用域过滤的常规裸搜索，必须无条件采用 Gortex 官方双穿透机制：
   - **穿透搜索（全局生效）**：
     - Core 55 模式：一律使用 `query_project(project="<目标项目>", query="...")` 检索符号。此为官方免切穿透接口，直接击穿 bound 隔离并返回目标项目完整符号。
     - Facade 门面模式：一律使用 `workspace(operation="project", project="<目标项目>", query="...")`。
   - **穿透精读与分析（全局权威）**：
     - 所有阅读、拓扑与追踪工具（`read_file`, `get_symbol`, `get_symbol_source`, `get_editing_context`, `get_callers`, `find_usages`, `trace` 等），路径一律显式补齐目标项目前缀：
       `read_file(path="<目标项目>/path/to/file")`
       门面模式：`read(operation="file", target={file:"<目标项目>/path/to/file"})`
     - Gortex 运行时对带合法项目前缀的路径拥有全局权威解析能力，不受会话 bound 约束。
   - **无缝回切与零污染**：
     跨项目分析完毕后，无需执行任何“切回”动作，直接在主项目中继续使用主项目路径推进，主副项目零状态震荡、零串扰。

3. **严禁擅自退回原生工具**：
   严禁以 `list_repos` 仅显示初始库、`search_text` 裸搜无结果或未收到切换指令为由擅自退回宿主原生工具。只要用户已 track 相关项目，必须通过上述“穿透搜索 + 前缀精读”在 Gortex 中完成任务。

3.3 Facade surface 与参数容器规则

公共 facade-v1 名称（21个）：
`analyze`, `ask`, `capabilities`, `change`, `edit`, `explore`, `overlay`, `pr`, `publish_review`, `read`, `recall`, `refactor`, `relations`, `remember`, `response`, `review`, `search`, `session`, `trace`, `workspace`, `workspace_admin`。

capabilities 查询：
capabilities()
capabilities(domain="<tool>")
capabilities(domain="<tool>", operation="<operation>", detail="summary")
capabilities(domain="<tool>", operation="<operation>", detail="schema")

【参数容器与 arguments 陷阱（极其重要）】
- **冷门面**（`publish_review`, `pr`, `recall`, `remember`, `workspace`, `workspace_admin`, `overlay`, `response`）及 `session` 在 input_schema 中声明了 `arguments` 容器，其操作参数允许封装在 `arguments: {...}` 中。
- **热门面**（`explore`, `search`, `read`, `relations`, `trace`, `analyze`, `ask`, `change`, `review`, `edit`, `refactor`）未声明 arguments 属性！**严禁**在外层包裹 `arguments: {...}`，否则触发硬错误：`arguments is an unexpected top-level key ... arguments is the JSON-RPC envelope, not a parameter`。参数必须直接放顶层或对应容器（target/options/guard/source/context）。

【固定参数（fixed_arguments）】
search.symbols: assist=off
analyze.co_change: refresh=false
change.contract: ack=false
change.simulate: keep=false
edit.wiki: enhance=false
edit.apply_overlay: to_disk=true
recall.surface: mark_accessed=false
overlay.simulate: keep=true
overlay.merge: to_disk=false
remember.risk_ack: ack=true
workspace_admin.{blame,coverage,sql_rebuild,temporal_verify}: kind=<name>

GORTEX_TOOL_ARG_GUARD=reject 时未知参数直接拒绝；默认模式下未知参数附加 `_ignored_options`。

3.4 55 Core 扁平工具与 21 Facade 门面对照字典

当当前 session inventory 显示为 55 个扁平工具（Antigravity 默认状态）时，直接按下列左侧函数名平铺传参；当为 Facade 模式时按右侧门面语法调用：

| 55 扁平工具 (Mode A) | 对应 21 Facade 门面 (Mode B) | 参数平铺说明 (Mode A 入参) |
| :--- | :--- | :--- |
| `explore` | `explore(operation="localize"|"task")` | `task="<query>"` (首选全局定位) |
| `smart_context` | `explore(operation="context")` | `task="<query>"` (上下文分析) |
| `get_repo_outline` | `explore(operation="outline")` | `{}` (仓库大纲全景) |
| `search_symbols` | `search(operation="symbols")` | `query="...", kind="..."` (符号搜索) |
| `search_text` | `search(operation="text")` | `query="...", regexp=false` (代码全文) |
| `find_files` | `search(operation="files")` | `query="...", glob="..."` (文件名查找) |
| `read_file` | `read(operation="file")` | `path="...", offset=1, limit=100` (读文件) |
| `get_symbol` | `read(operation="source"|"summary")` | `symbol="<id>"` (符号定位与签名) |
| `get_symbol_source` | `read(operation="source")` | `symbol="<id>"` (符号完整源码) |
| `get_file_summary` | `read(operation="summary")` | `file="<path>"` (文件符号概览) |
| `get_editing_context` | `read(operation="editing_context")` | `file="<path>"` (编辑前拓扑必调) |
| `get_callers` | `relations(operation="callers")` | `symbol="<id>"` (反向调用者) |
| `find_usages` | `relations(operation="usages")` | `symbol="<id>", context="..."` (引用点) |
| `find_implementations`| `relations(operation="implementations")` | `symbol="<id>"` (接口实现) |
| `get_dependencies` | `relations(operation="dependencies")` | `symbol="<id>"` (前向依赖) |
| `get_dependents` | `relations(operation="dependents")` | `symbol="<id>"` (反向被依赖/爆炸半径)|
| `get_call_chain` | `trace(operation="call_chain")` | `symbol="<id>"` (深度调用链) |
| `simulate_chain` | `trace(operation="flow"|"cfg")` | `symbol="<id>"` (调用模拟) |
| `preview_edit` | `edit(..., dry_run=true)` | `path="...", old_string="...", new_string="..."` |
| `edit_file` | `edit(operation="file")` | `path="...", match="...", replacement="...", dry_run=false` |
| `edit_symbol` | `edit(operation="symbol")` | `id="...", old_source="...", new_source="...", dry_run=false` |
| `write_file` | `edit(operation="write")` | `path="...", content="..."` |
| `batch_edit` | `edit(operation="batch")` | `changes=[...]` (批量事务修改) |
| `rename_symbol` | `refactor(operation="rename")` | `id="...", new_name="..."` |
| `check_guards` | `change(operation="guards")` | `symbols=["<id>"]` (守护与破坏检查) |
| `get_test_targets` | `change(operation="tests")` | `symbols=["<id>"]` (获取波及单测目标) |
| `verify_change` | `change(operation="verify")` | `symbol_id="...", new_signature="..."` |
| `detect_changes` | `change(operation="detect")` | `{}` (检测工作区未提交变动) |
| `diff_context` | `change(operation="preview")` | `{}` (审查 diff 上下文) |
| `review` | `review(operation="run")` | `scope="unstaged"` (代码综合审查) |
| `compare_branches` | `change(operation="compare_branches")` | `base="...", head="..."` |
| `store_memory` | `remember(operation="memory")` | `kind="invariant", title="...", body="..."` |
| `save_note` | `remember(operation="note")` | `file="...", body="...", tags=[...]` |
| `surface_memories` | `recall(operation="surface")` | `task="<query>"` (召回项目规约记忆) |
| `overlay_*` (12个) | `overlay(operation="*")` | 虚拟分支/编辑隔离控制 |
| `proxy_*` (3个) | `session(operation="proxy_*")` | 外部代理控制 |
| `tool_profile` | `capabilities()` | 查看当前激活的工具配置与预设 |
| `tools_search` | `capabilities(detail="schema")` | 在线搜索并热加载 120+ 延迟工具 |
| `graph_stats` | `workspace(operation="graph")` | 知识图谱节点边统计 |
| `index_health` | `workspace(operation="index")` | 索引与守护进程健康检查 |
| `set_active_project` | `workspace_admin(operation="set_active_project")` | `project="<id>"` (动态热切换活跃项目) |
| `get_active_project` | `workspace(operation="active_project")` | `{}` (获取当前活跃工作区与项目) |
| `query_project` | `workspace(operation="project")` | `project="<id>", query="..."` (跨项目单次免切查询) |
| `list_repos` | `workspace(operation="repos")` | `{}` (查看已索引仓库列表) |
| `workspace_info` | `workspace(operation="info")` | `{}` (工作区全量元数据) |

4. 读取、搜索、关系和分析

4.1 search operation（7个）

可用操作：`artifacts`, `ast`, `completion`, `files`, `symbols`, `text`, `winnow`。

search(operation="files", query="...")
search(operation="symbols", query="...")
search(operation="text", query="...")
search(operation="ast", query="<pattern>")
search(operation="winnow", query="...")
- symbols, text, completion 必须显式提供非空 `query`，否则报错 `search.<op> requires query`。
- symbols 固定 assist=off（保证本地确定性搜索）。
- ast 的 query 映射至 pattern；winnow 的 query 映射至 text_match。

4.2 read operation（7个）

可用操作：`artifact`, `editing_context`, `file`, `history`, `source`, `summary`, `symbols`。

read(operation="file", target={file:"..."}, offset=1, limit=100)
read(operation="editing_context", target={file:"..."})
read(operation="source", target={symbol:"..."})
read(operation="symbols", target={symbols:["id1","id2"]})
read(operation="history", target={symbol:"..."})
read(operation="summary", target={file:"..."})
read(operation="artifact", target={artifact:"..."})

【target 选择器单一性规则（高危校验）】
- target 必须为对象且**有且仅能有一个选择器**：只能从 `file`, `symbol`, `symbols`, `query`, `artifact`, `repo` 中选 1 个。
- 传空 `{}` 或多键 `{file:"...", symbol:"..."}` 均会报错 `target must contain exactly one selector`。
- 传未知键（如 `{path:"..."}`）会报错 `unknown target selector "path"`。
- 单数 `symbol` 传入数组会报错 `singular selector requires exactly one ID`；批量查询必须用 `symbols`。
- `read.symbols` 的 `target.symbols` 必须是非空数组，传空会报错 `read.symbols requires a non-empty symbol ID array`。
- `read.file` 支持 `offset`/`limit` 或 `start_line`/`end_line`。
- 在 `git_ref` 或 `commit` 虚拟视图下，严禁请求 `physical_evidence: true`（会报错提示非磁盘文件）。

4.3 relations & trace operation

relations（11个）：`callers`, `cluster`, `declaration`, `dependencies`, `dependents`, `hierarchy`, `implementations`, `import_path`, `overrides`, `references`, `usages`。
relations(operation="callers", target={symbol:"..."})
relations(operation="declaration", target={query:"use_site"})
relations(operation="dependencies", target={symbol:"..."})
relations(operation="implementations", target={symbol:"..."})
relations(operation="usages", target={symbol:"..."})

trace（7个）：`call_chain`, `cfg`, `flow`, `graph`, `path`, `taint`, `walk`。
trace(operation="call_chain", target={symbol:"..."})
trace(operation="flow", target={symbol:"<src>"}, to={symbol:"<sink>"}, options={max_depth:6})
trace(operation="path", target={symbol:"<src>"}, to={symbol:"<sink>"})
trace(operation="taint", target={query:"<src_pat>"}, to={query:"<sink_pat>"})
- `flow`、`path`、`taint` 同时需要 `target`（源）与 `to`（宿），且均遵守单一选择器规范。

4.4 analyze operation

`analyze` 是只读统一分析门面，内置 77 种分析 kind（如 `architecture`, `cycles`, `dead_code`, `health`, `impact`, `sast`, `coverage_gaps`, `race_writes`, `untested`, `clones`, `churn` 等）。
- **变动类阻断**：`blame`, `coverage`, `sql_rebuild`, `temporal_verify` 会持久化变更图数据或调用外部模型，在 `analyze` 下会被直接拦截拒执，必须通过 `workspace_admin(operation="<kind>")` 执行。
- 默认无 kind 调用时等同于 `analyze(operation="help")`。

5. 修改、重构和验证

5.1 修改前评估

change(operation="impact", target={symbol:"<id>"})
change(operation="edit_plan", target={symbols:["<id1>","<id2>"]})
change(operation="guards", target={symbols:["<id1>"]})
change(operation="tests", target={symbols:["<id1>"]})
change(operation="contract", target={symbol:"<id>"})
change(operation="verify", source={changes:[{symbol_id:"<id>", new_signature:"<sig>"}]})

- `change.contract` 必须提供 `target.symbol`/`target.symbols` 或显式 non-symbol source。
- `remember(operation="risk_ack")` 必须在当前确实存在待确认的变动符号时调用，无变动时会报错 `change_contract ack: no changed symbols to acknowledge`。

change operation（19个）：
`api_impact`, `code_actions`, `compare_branches`, `compare_overlay`, `contract`, `detect`, `diagnostics`, `edit_plan`, `guards`, `impact`, `overlay_branches`, `overlay_state`, `pattern`, `preview`, `ranges`, `receipt`, `simulate`, `tests`, `verify`。

5.2 edit 与 refactor（核心防错规范）

edit operation（10个）：`apply_overlay`, `batch`, `docs`, `export_graph`, `file`, `scaffold`, `skill`, `symbol`, `wiki`, `write`。
refactor operation（6个）：`apply_code_action`, `delete`, `fix_all`, `inline`, `move`, `rename`。

【【重要铁律】两阶段修改规范与参数冲突排错

1. **`dry_run: true` 与 `physical_evidence: true` 严格互斥！**
   - 源码 `mutation_evidence.go:17` 硬编码拒绝此组合：`physical_evidence requires a real write; dry_run leaves no disk bytes to attest`。
   - **预览阶段**必须使用 `dry_run: true`（或 55 模式下的 `preview_edit`），严禁传入 `physical_evidence`。
   - **真实写入阶段**必须使用 `dry_run: false`，才可开启 `options={physical_evidence:true}`。
2. **`digest` 强依赖 `physical_evidence`**：
   - 传了 `digest` 必须同时有 `physical_evidence: true`，且当前仅支持 `sha256`。
3. **`guard.expected_occurrences` 仅用于 `edit_file`**：
   - 当实际匹配数量与 `expected_occurrences` 不一致时，源码会拒绝修改并报警。
   - `edit_symbol` 基于 AST 节点 ID，不接受 `expected_occurrences`。
4. **前后内容一致拒绝**：
   - `old_string == new_string` 或 `old_source == new_source` 会被硬拒绝。
5. **语言支持边界**：
   - `refactor.move`（`move_symbol`）和 `refactor.inline`（`inline_symbol`）在当前运行时仅支持 Go 语言文件。
6. **Secret 配置保护**：
   - 对包含密码或 token 的敏感配置（如 `.env`）请求 physical_evidence 会被拦截，除非使用 `allow_secrets: true`。

【正确调用形状示例】

【形状 A：修改普通文件（edit.file / edit_file）】
// 阶段 1：预览 Diff（dry-run，严禁带 physical_evidence）
edit(operation="file", target={file:"<file>"}, match="<old>", replacement="<new>", dry_run=true, guard={expected_occurrences:1})
// 或 55 扁平模式：preview_edit(path="<file>", old_string="<old>", new_string="<new>")

// 阶段 2：正式写入（dry_run=false，开启磁盘哈希凭证）
edit(operation="file", target={file:"<file>"}, match="<old>", replacement="<new>", dry_run=false, options={physical_evidence:true, mutation_id:"<id>"})
// 或 55 扁平模式：edit_file(path="<file>", match="<old>", replacement="<new>", dry_run=false)

【形状 B：精确修改符号（edit.symbol / edit_symbol）】
// 阶段 1：预览 Diff
edit(operation="symbol", target={symbol:"<id>"}, match="<old>", replacement="<new>", dry_run=true)

// 阶段 2：正式写入
edit(operation="symbol", target={symbol:"<id>"}, match="<old>", replacement="<new>", dry_run=false, options={physical_evidence:true})
// 或 55 扁平模式：edit_symbol(id="<id>", old_source="<old>", new_source="<new>", dry_run=false)

【形状 C：事务型批量修改（edit.batch / batch_edit）】
edit(
  operation="batch",
  dry_run=true,
  changes=[
    {op:"edit_file", path:"<f1>", old_string:"<old>", new_string:"<new>"},
    {op:"edit_symbol", id:"<id>", old_source:"<old>", new_source:"<new>"},
    {op:"move_file", source:"<src>", destination:"<dst>", expected_sha256:"<sha>"},
    {op:"delete_file", path:"<path>", expected_sha256:"<sha>"}
  ]
)
- `changes` 数组中每项的必填字段缺失时会立刻报错；`move_file` 和 `delete_file` 强烈建议带 `expected_sha256`。

【safe-delete 规则】
安全删除符号默认 dry_run=true，存在引用时拒绝；支持通过 `--apply`、`--cascade`（preview/apply）、`--propagate`、`--force` 控制级联与强制执行。

5.3 修改后验证

change(operation="detect")
change(operation="tests", target={symbols:["<affected ids>"]})
change(operation="guards", target={symbols:["<affected ids>"]})
change(operation="contract", target={symbols:["<affected ids>"]})
*注：在 55 模式下，对应调用 `detect_changes()`、`get_test_targets(symbols=[...])`、`check_guards(symbols=[...])`。*
- `change.tests` / `get_test_targets` 仅返回建议运行的单测目标列表，**并不代表单测已运行**；必须实际执行构建和测试命令（如 `go test`, `npm test` 等）并汇报真实测试输出。

6. overlay、memory、session 和 review

6.1 overlay（10个）

操作列表：`delete`, `drop`, `drop_branch`, `fork`, `keepalive`, `merge`, `push`, `register`, `simulate`, `switch`。
overlay(operation="register")
overlay(operation="push", arguments={branch:"main"})
overlay(operation="merge", arguments={branch:"main"})
overlay(operation="keepalive")
- `overlay.merge` 固定 to_disk=false（合并至 overlay 缓冲）；若要合并写盘，使用 `edit(operation="apply_overlay")`（固定 to_disk=true）。

6.2 recall（8个）& remember（8个）

recall（只读）：`distill`, `memories`, `notebook_find`, `notebook_list`, `notebook_show`, `notes`, `onboarding`, `surface`。
recall(operation="distill")
recall(operation="notes", arguments={file:"..."})
recall(operation="memories", arguments={kind:"invariant"})
recall(operation="surface", arguments={task:"..."})
*注：在 55 模式下调用 `surface_memories(task="...")`。*

remember（本地写入）：`edit_memory`, `memory`, `note`, `notebook`, `notebook_used`, `rename_memory`, `risk_ack`, `suppress_finding`。
remember(operation="note", arguments={body:"...", file:"...", tags:["decision"]})
remember(operation="memory", arguments={kind:"invariant", title:"...", body:"..."})
remember(operation="risk_ack")
*注：在 55 模式下调用 `store_memory(...)` 或 `save_note(...)`。*

6.3 session（控制会话）

操作列表：`agents`, `cursor`, `planning_mode`, `proxy_disable`, `proxy_enable`, `subscribe`, `unsubscribe`, `workflow`。
session(operation="agents", arguments={action:"list|register|heartbeat|lock|unlock|unregister"})
session(operation="planning_mode", arguments={enabled:true})
session(operation="subscribe", channel="diagnostics", arguments={min_severity:1})
session(operation="unsubscribe", channel="diagnostics")
- `subscribe` / `unsubscribe` **必须提供 channel 字段**，合法 channel 仅限：
  `daemon_health`, `diagnostics`, `graph_invalidated`, `stale_refs`, `workspace_readiness`。

6.4 review（7个）& pr（6个）& response（5个）

review(operation="run", source={scope:"unstaged"})
review(operation="critique", source={diff:"..."})
review(operation="diff_context", source={diff:"..."})
publish_review(operation="post", arguments={pr:123, body:"...", confirm_public:false})

pr(operation="list")
pr(operation="conflicts")
pr(operation="impact", arguments={pr:123})
pr(operation="triage", arguments={use_llm:false})

response(operation="export_context", arguments={task:"..."})
response(operation="grep", arguments={pattern:"..."})
response(operation="slice", arguments={start:1, end:50})

7. CLI 边界与核心命令

原生 MCP 可用时第一优先级调用 MCP。CLI 仅用于用户明确要求、只读诊断、真实单测/构建、未索引目录检查，或宿主无原生 Gortex 且用户明确允许。

7.1 Daemon 与服务控制
  gortex daemon start [--detach] [--tools <preset>]
  gortex daemon stop | restart | reload
  gortex daemon status [--watch]
  gortex daemon logs [--tail 50]

7.2 仓库与工作区管理
  gortex repos [--json]
  gortex repos families [--family <prefix>]
  gortex repos reconcile [prefix]
  gortex track <path> [--wait] [--as-worktree] [--name <prefix>]
  gortex untrack <path> [--confirm]
  gortex workspace list [--json]
  gortex workspace set <repo> <workspace> [project] [--global]

7.3 变动波及与测试目标检查
  gortex affected [files...] [--stdin] [--quiet] [--json]
  git diff --name-only | gortex affected --stdin --quiet
  （退出码：0=有测试受影响，3=无受影响测试可跳过CI，1=执行出错）

7.4 诊断、修复与状态
  gortex version [--short]
  gortex doctor [--days 7] [--redact] [--json] [--all]
  gortex status
  gortex tools list [--format json] [--preset compact]

8. 宿主适配与冲突裁决

适配诊断与环境核查：
gortex version
gortex doctor --days 7 --json
gortex status
gortex tools list --format json

核查 Gortex 绝对路径、MCP 配置文件、MCP command/args、hooks 信任和同一组后台环境变量。

冲突裁决优先级：
1. 当前工具返回的 error、completion、view、guard、effect。
2. 当前 capabilities / tools/list 的 schema、request_shape、fixed_arguments、available。
3. workspace/index/repository/checkout 真实状态。
4. 当前 Gortex 运行时源码、CLI --help、gortex tools describe。
5. 本文。

**多仓库全图谱铁律**：
所有已 track 仓库均处于 Gortex 的统一全量知识图谱中。严禁在未经跨项目穿透检索的情况下擅自断定“仓库未索引”并退回宿主原生工具。只有在 `query_project` 与带前缀路径均明确证实仓库未被守护进程 track，且用户明确要求本地文件检查时，方可使用本地原生命令或文件工具。