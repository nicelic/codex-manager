Gortex 使用规范

> 本规范面向使用 Gortex MCP 与 Agent 宿主适配器（Google Antigravity、Claude Code、Codex、Cursor 等）的代码 Agent。

1. 最高原则和使用边界

Gortex 是对其 track 仓库进行代码定位、源码阅读、关系分析、数据流追踪、影响评估、编辑、重构、验证、审查、项目管理和持久化记忆的权威工具。

- **优先原生 MCP 句柄约束**：已 track 仓库，必先通过 Gortex 原生 MCP 句柄执行，若无法使用不得擅自退回软件原生工具、命令执行分析，必须停止并进行通知。未track的项目\内容不得擅自进行track图谱化。
- **全局技能只读豁免**：`read_file` 放行全局技能只读（`~/.agents/skills/*/SKILL.md`、`~/.config/opencode/skills/*/SKILL.md`、`~/.copilot/skills/*/SKILL.md`、`~/.claude/skills/*/SKILL.md`）。
- **严禁凭记忆捏造**：严禁伪造未返回的 symbol、关系、view、索引状态或安全结论。
- **关键实现体完整阅读与 Token 瘦身**：摘要和搜索结果不能替代关键实现阅读；行为关键代码读完整源码。长代码巡检时，可在 `read_file`、`get_symbol_source`、`get_editing_context` 中使用 `compress_bodies: true`（存根化削减 60%~70% Token，支持 `keep="f1,f2"` 保留指定函数）。
- **遵守写契约**：所有写操作遵守当前 schema、effect、fixed_arguments、guard、view、overlay 和 partial failure 结果。

1.1 协议名与双模感知（Core 扁平离散 vs Facade-v1 门面）

根据 MCP 连接预设不同，Gortex 暴露两种工具形态，以当前 session inventory 自适应路由：
1. **Core 扁平离散工具模式（Antigravity 默认预设）**：直接暴露 `explore`, `smart_context`, `get_repo_outline`, `read_file`, `get_symbol_source`, `get_callers`, `search_symbols`, `search_text`, `search_ast`, `edit_file`, `batch_edit`, `query_project`, `contracts`, `audit_health` 等 65 个热核心工具。参数扁平传入。
   - **热核心集（Hot Eager Tools）**：常用基础工具开局立即可用（0.64.3 中 `search_ast`, `contracts`, `audit_health`, `get_churn_rate` 均已入热集）。
   - **延迟目录（Deferred Catalog）**：冷工具（如 `winnow_symbols`, `context_closure`, `plan_turn`, `safe_delete_symbol` 等）托管于冷目录，若环境提示未暴露，通过 `tools_search(query="...")` 唤醒或选用热集等价工具（如 `analyze(kind="...")`）。
2. **Facade-v1 紧凑门面模式（21 个顶级工具）**：暴露 `read`, `explore`, `search`, `relations`, `trace`, `edit`, `change`, `refactor`, `workspace` 等门面，使用 `operation` 与 `target` 单一选择器规范。

- **多项目图谱一致性铁律**：Gortex 统一维护所有已 track 仓库的全局知识图谱。必须通过第 3.4 节机制在 Gortex 内精准执行。

2. 任务开启与探索流转（Explore & Discovery Workflow）

- **任务初次分析流转准则**：
  1. **明确文件名时的首读契约（0.64.3 准则）**：用户明确指定文件阅读/审查/总结时，**首个动作直接调用 `read_file(path="<path>")`**（门面：`read(operation="file", target:{file:"<path>"}, options:{new_user_task:true})`），**勿触发 localize 定位**。
  2. **代码定位与任务探索**：位置未知或故障排查时，**优先使用 `explore(task="<完整问题>")`**（门面：`explore(operation="localize"|"task", task="...")`）。一键汇聚目标附近的符号、源码与调用链。遵循 `completion.required_action`：状态为 `answer_ready` 时直接从 `completion.final_response` 结案并停用工具；为 `needs_exact_read` 时补齐精读。
  3. **智能上下文装配**：**优先使用 `smart_context(task="<任务>")`**（门面：`explore(operation="context", task="...")`）。装配最小完备上下文与编辑规划，支持 `fidelity="graded"`, `token_budget`, `entry_point`。
  4. **架构大纲速览**：**优先使用 `get_repo_outline()`**（门面：`explore(operation="outline")`）。快速获取语言分布、Entry Points、Hotspots 与顶层大纲（扁平无 repo 参）。

2.1 `explore` 9 大子命令（operations）深度指南
根据 `internal/mcp/facade_registry.go` 源码，9 个子命令职责与参数：
1. **`localize`（终结性定位）**：门面 `explore(operation="localize", task="...")`（固定 `localize=true`）；扁平 `explore(task="...", path="...", token_budget=1600)`。强约束 terminality 状态机。
2. **`task`（诊断与任务分析）**：门面 `explore(operation="task", task="...")`；扁平 `explore(task="...")`。故障诊断与多步因果分析。
3. **`context`（智能上下文装配）**：门面 `explore(operation="context", task="...")`；扁平 `smart_context(task="...", entry_point="...", fidelity="graded", token_budget=8000)`。
4. **`outline`（工程骨架）**：门面 `explore(operation="outline")`；扁平 `get_repo_outline()`。
5. **`closure`（图依赖闭包）**：门面 `explore(operation="closure")`；扁平 `context_closure(symbols="id1,id2", files="f1,f2")`（参数为 symbols/files）。
6. **`plan`（开局路径路由）**：门面 `explore(operation="plan")`；扁平 `plan_turn(task="...")`。推荐开局优先调用的工具序列。
7. **`suggest`（冷启动查询词推荐）**：门面 `explore(operation="suggest")`；扁平 `suggest_queries()`。基于拓扑推荐 5-10 个探索词。
8. **`prefetch`（上下文预取）**：门面 `explore(operation="prefetch")`；扁平 `prefetch_context(task="...", recent_symbols="...")`。后台预热邻居节点。
9. **`wakeup`（架构全景摘要）**：门面 `explore(operation="wakeup")`；扁平 `gortex_wakeup()`。生成 ~500 Token 架构摘要 Markdown。

2.2 检索、阅读与图谱协同
各工具正交协同，按需自由组合：
- **`search_text(query="...", regexp=false, path="...", repo="...")`**：Trigram 索引全文/正则搜索。**命中附带 `symbol_id` 与 `symbol_name`**，可衔接图谱工具。
- **`search_symbols(query="...", kind="...", flavor="...", path="...")`**：BM25 驼峰分词检索函数、类、接口等 AST 定义。
- **`search_ast(pattern="..." | detector="...")`**：[热] 语法级代码检索。支持 15+ 缺陷/安全检测器及 Tree-sitter S 表达式匹配。
- **`find_files(query="...", glob="*.*")`**：按路径前缀或 Glob 通配查找工程物理文件。
- **`get_callers` / `get_call_chain` / `find_usages` / `find_implementations`**：沿 AST 拓扑追踪调用链、实现与精准引用点。
- **`read_file` / `get_symbol_source`**：阅读文件或具体函数源码。长文件巡检建议配合 `offset/limit` 或 `compress_bodies`。

3. 双模调用对照字典与参数防错规范

3.1 双模核心对照表（基于 Gortex 0.64.3 源码事实）

Core 扁平离散模式下严格属性校验（`additionalProperties: false`）。**入参属性名必须严格匹配**。若遇冷目录工具，调用 `tools_search` 唤醒或选用热核心等价工具。

| 功能场景 | Core 扁平工具 (Mode A) | Facade-v1 门面 (Mode B) | 说明 |
| :--- | :--- | :--- | :--- |
| **全局任务定位** | `explore(task="...")` | `explore(operation="localize"\|"task", task="...")` | [热] 优先使用，单任务入口 |
| **智能上下文装配** | `smart_context(task="...")` | `explore(operation="context", task="...")` | [热] 优先使用，装配工作集 |
| **工程骨架大纲** | `get_repo_outline()` | `explore(operation="outline")` | [热] 模块大纲，扁平无 repo 参 |
| **架构统一分析** | `analyze(kind="architecture")` | `analyze(operation="architecture")` | [热] 系统分层与架构全景 |
| **符号精确搜索** | `search_symbols(query="...", kind="...")` | `search(operation="symbols", query="...")` | [热] 固定 assist=off |
| **文本全文搜索** | `search_text(query="...", regexp=false)` | `search(operation="text", query="...")` | [热] Trigram加速，带 symbol_id |
| **AST 语法检索** | `search_ast(pattern="..."\|detector="...")` | `search(operation="ast", pattern="..."\|detector="...")` | [热] 支持检测器与S式匹配 |
| **查找文件** | `find_files(query="...", glob="*.*")` | `search(operation="files", query="...")` | [热] 文件名与 Glob 查找 |
| **多轴约束过滤** | `winnow_symbols(text_match="...")` | `search(operation="winnow", query="...")` | [冷] BM25 与图属性联合过滤 |
| **读取非代码资产** | `get_artifact(id="..."\|path="...")` | `read(operation="artifact", target={artifact:"..."})` | [冷] 读取设计规范/文档 |
| **读取文件** | `read_file(path="...", offset=1, limit=100)` | `read(operation="file", target={file:"..."}, options={...})` | [热] 支持 new_user_task 与 offset/limit |
| **读取符号源码** | `get_symbol_source(id="<id>")` | `read(operation="source", target={symbol:"<id>"})` | [热] 读源码实现，入参为规范 ID |
| **读取符号元数据** | `get_symbol(id="<id>")` | `read(operation="symbol_metadata_compat", target={symbol:"<id>"})` | [热] 仅元数据，不带源码 body |
| **批量读取符号源码**| `batch_symbols(symbols=["id1","id2"])` | `read(operation="symbols", target={symbols:[...]})` | [冷] 批量获取源码 |
| **单文件符号概览** | `get_file_summary(path="<path>")` | `read(operation="summary", target={file:"<path>"})` | [热] 扁平入参为 path |
| **编辑前拓扑分析** | `get_editing_context(path="<path>")` | `read(operation="editing_context", target={file:"..."})`| [热] 扁平入参为 path |
| **反向调用者** | `get_callers(id="<id>", depth=2)` | `relations(operation="callers", target={symbol:"<id>"})`| [热] 查函数/方法调用者 |
| **符号引用点** | `find_usages(id="<id>", context="call")` | `relations(operation="usages", target={symbol:"<id>"})` | [热] 语法级精确定位 |
| **接口实现查找** | `find_implementations(id="<id>")` | `relations(operation="implementations", target={symbol:"..."})` | [热] 查找接口实现 |
| **类/接口继承层次** | `get_class_hierarchy(id="<id>")` | `relations(operation="hierarchy", target={symbol:"<id>"})` | [冷] 继承体系树形展开 |
| **方法重写查找** | `find_overrides(id="<id>")` | `relations(operation="overrides", target={symbol:"<id>"})` | [热] 查虚函数/方法重写 |
| **声明跳转** | `find_declaration(use_site="...")` | `relations(operation="declaration", target={query:"..."})` | [冷] 使用点反查声明 |
| **前向/反向依赖** | `get_dependencies(id="<id>")` / `get_dependents(id="<id>")` | `relations(operation="dependencies"\|"dependents", target={symbol:"<id>"})` | [热] 依赖拓扑/评估影响面 |
| **深度调用链路** | `get_call_chain(id="<id>", depth=4)` | `trace(operation="call_chain", target={symbol:"<id>"})` | [热] 递归调用路径追踪 |
| **控制流分析** | `get_cfg(id="<id>")` | `trace(operation="cfg", target={symbol:"<id>"})` | [冷] 控制流图分析 |
| **节点最短路径** | `trace_path(source_id="...", sink_id="...")` | `trace(operation="path", target={symbol:"..."}, to={symbol:"..."})` | [冷] 拓扑最短关联路径 |
| **修改文件** | `edit_file(path="...", old_string="...", new_string="...", dry_run=true\|false)` | `edit(operation="file", target={file:"..."}, dry_run=true\|false)` | [热] 预览 dry_run=true 禁带凭据 |
| **精确修改符号** | `edit_symbol(id="...", old_source="...", new_source="...", dry_run=true\|false)` | `edit(operation="symbol", target={symbol:"..."}, dry_run=true\|false)` | [热] 精确替换 AST 节点源码 |
| **覆盖写入文件** | `write_file(path="...", content="...")` | `edit(operation="write", target={file:"..."}, content="...")` | [热] 全量覆盖写入 |
| **事务型批量修改** | `batch_edit(edits=[...], dry_run=true)` | `edit(operation="batch", changes=[...], dry_run=true)` | [热] 支持 edit/move/delete_file |
| **符号安全重命名** | `rename_symbol(id="...", new_name="...", dry_run=true)` | `refactor(operation="rename", target={symbol:"..."}, dry_run=true)`| [热] 全图引用联动重构 |
| **安全删除符号** | `safe_delete_symbol(id="...", dry_run=true)` | `refactor(operation="delete", target={symbol:"<id>"}, dry_run=true)`| [冷] 门禁校验无残留引用 |
| **符号物理移动** | `move_symbol(id="...", target_file="...", dry_run=true)` | `refactor(operation="move", target={symbol:"..."}, destination="...")` | [热] 跨文件移动(Go) |
| **内联符号** | `inline_symbol(id="...", dry_run=true)` | `refactor(operation="inline", target={symbol:"..."}, dry_run=true)` | [热] 内联展开微小函数(Go) |
| **守护规则检查** | `check_guards(ids="id1,id2")` | `change(operation="guards", target={symbols:[...]})` | [热] 逗号分隔 ids 字符串 |
| **受波及单测定位** | `get_test_targets(ids="id1,id2")` | `change(operation="tests", target={symbols:[...]})` | [热] 逗号分隔 ids 字符串 |
| **函数签名契约校验**| `verify_change(changes='[{"symbol_id":"...","new_signature":"..."}]')` | `change(operation="verify", source={changes:[...]})` | [热] 签名破坏性校验 |
| **变更影响评估** | `explain_change_impact(ids="<id>")` / `analyze(kind="impact")` | `change(operation="impact", target={symbol:"<id>"})` | [热] 扁平入参为 ids |
| **变更风险契约** | `change_contract(diff="..."\|workspace_edit="...")` | `change(operation="contract", source={...})` | [冷] 预测风险与停止条件 |
| **LSP 编辑模拟** | `preview_edit(workspace_edit="...")` | `change(operation="preview", source={workspace_edit:"..."})` | [热] 专用于 WorkspaceEdit |
| **链式编辑沙箱模拟**| `simulate_chain(steps="[...]", keep=false)` | `change(operation="simulate")` | [热] 变更链路沙箱推演 |
| **检测未提交改动** | `detect_changes()` | `change(operation="detect")` | [热] 工作区脏改动检测 |
| **Diff 上下文提取**| `diff_context(scope="unstaged")` | `review(operation="diff_context", source={scope:"unstaged"})`| [热] 提取 diff 拓扑上下文 |
| **图增强代码审查** | `review(scope="unstaged")` | `review(operation="run", source={scope:"unstaged"})` | [热] 综合审查引擎 |
| **API 跨服务契约** | `contracts(action="list"\|"check"\|"validate"\|"bridge")` | `analyze(operation="contracts")` | [热] 跨服务契约路由校验 |
| **复杂度健康评分** | `audit_health()` | `analyze(operation="health")` | [热] A-F 级图健康度评分 |
| **代码热点与扰动率**| `get_churn_rate()` | `analyze(operation="churn")` | [热] 函数级代码提交扰动率 |
| **未重读增量变动** | `get_recent_changes()` | `analyze(operation="recent_changes")` | [热] Watch 增量变更检测 |
| **持久化规约记忆** | `store_memory(kind="invariant", title="...", body="...")` | `remember(operation="memory", arguments={...})` | [热] 记录跨会话不变量 |
| **保存代码决策笔记**| `save_note(file_path="...", body="...", tags="tag1,tag2")` | `remember(operation="note", arguments={...})` | [热] 扁平入参为 file_path |
| **召回规约记忆** | `surface_memories(task="...", symbol_ids="...")` | `recall(operation="surface", arguments={task:"..."})` | [热] 召回历史记忆 |
| **提炼会话摘要** | `distill_session()` | `recall(operation="distill")` | [热] 提炼关键上下文 |
| **跨项目穿透检索** | `query_project(project="...", query="...")` | `workspace(operation="project", project="...", query="...")` | [热] 跨库穿透免切检索 |
| **热切换活跃工程** | `set_active_project(project="...")` | `workspace_admin(operation="set_active_project", arguments={...})`| [热] 动态重定向主工程 |
| **当前活跃工程** | `get_active_project()` | `workspace(operation="active_project")` | [热] 查询当前绑定工程 |
| **已索引仓库列表** | `list_repos()` | `workspace(operation="repos")` | [热] 查询已 track 仓库 |
| **全局图谱统计** | `graph_stats()` | `workspace(operation="graph")` | [热] 节点边与容量统计 |
| **守护进程健康检查**| `index_health()` | `workspace(operation="index")` | [热] 索引健康度诊断 |
| **发现延迟目录工具**| `tools_search(query="...")` | `capabilities(operation="legacy_search")` | [热] 动态激活冷目录工具 |

3.2 参数容器与 arguments 规范

- **冷门面（8 个）**：`publish_review`, `pr`, `recall`, `remember`, `workspace`, `workspace_admin`, `overlay`, `response` 在 Schema 中声明了 `arguments` 属性，参数封装在 `arguments: {...}` 中。
- **热门面**：`explore`, `search`, `read`, `relations`, `trace`, `analyze`, `ask`, `change`, `review`, `edit`, `refactor` 以及 `session` **严禁**包裹外层 `arguments`，参数放顶层或对应容器（`target` / `options` / `guard` / `source` / `context`）。

3.3 固定参数（fixed_arguments）全景

以下参数由运行时强制固化，调用时无需或禁止覆盖：
- `explore.localize`: 固定 `localize=true`。
- `search.symbols`: 固定 `assist=off`。
- `analyze.co_change`: 固定 `refresh=false`。
- `analyze.help`: 固定 `kind="help"`。
- `change.contract`: 固定 `ack=false`。
- `change.simulate`: 固定 `keep=false`。
- `edit.wiki`: 固定 `enhance=false`。
- `edit.apply_overlay`: 固定 `to_disk=true`。
- `recall.surface`: 固定 `mark_accessed=false`。
- `overlay.simulate`: 固定 `keep=true`。
- `overlay.merge`: 固定 `to_disk=false`。
- `remember.risk_ack`: 固定 `ack=true`。
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

4.1 search 7 大操作与高级过滤
可用操作：`artifacts`, `ast`, `completion`, `files`, `symbols`, `text`, `winnow`。
- `symbols`, `text`, `completion` 必须提供非空 `query`。
- `ast`：通过语法模式或检测器检索（支持 `pattern`, `detector`, `language`，门面下 `query` 自动别名转 `pattern`）。
- `artifacts`：搜设计文档、架构规范、OpenAPI 模式等非代码资产（支持 `query`, `kind`）。
- `winnow`：多轴结构化检索，结合 BM25 文本与图属性筛选（支持 `text_match`、`kind`、`language`、`path_prefix`、`min_fan_in`、`limit` 等）。

4.2 read 7 大操作与单一选择器
可用操作：`artifact`, `editing_context`, `file`, `history`, `source`, `summary`, `symbols`。
- **选择器单一性**：`target` 必须为对象且有且仅能有一个键（`file`, `symbol`, `symbols`, `query`, `artifact`, `repo` 选 1 个）。
- `read.file` 支持 `options={offset:1, limit:100}` 或 `context={start_line:1, end_line:100}`（底层自动换算）。首读明确文件支持 `options={new_user_task:true}`。
- `read.source`：精读符号实现体，可指定 `context_lines` 扩展外围行数。

4.3 relations (11个) 与 trace (7个)
- **relations（11个）**：`callers`（调用者）, `cluster`（连通社区）, `declaration`（声明定位，扁平入参 use_site）, `dependencies`（前向依赖）, `dependents`（反向依赖）, `hierarchy`（类继承树）, `implementations`（接口实现）, `import_path`（导入路径）, `overrides`（虚方法重写）, `references`（引用完整性）, `usages`（代码使用点）。
- **trace（7个）**：`call_chain`（调用链路追踪）, `cfg`（控制流图）, `flow`（端到端数据流向）, `graph`（通用图谱查询）, `path`（节点最短路径，入参 source_id/sink_id）, `taint`（污点传播分析）, `walk`（图漫游拓扑步进）。

4.4 analyze 统一分析全景（Core 78 种 kind vs Facade 22 种 operation）
- **Core 模式 `analyze(kind="...")`**：内置 78 种分析 kind。涵盖架构拓扑（`cycles`, `would_create_cycle`, `clusters`, `suggest_boundaries`, `hotspots`, `components`）、代码健康（`dead_code`, `coverage_gaps`, `doc_staleness`, `todos`）、并发与安全（`race_writes`, `channel_ops`, `goroutine_spawns`, `sast`, `hygiene`, `unsafe_patterns`, `routes`, `models`）等。传 `kind="help"` 获取全量清单。
- **Facade 门面 `analyze(operation="...")`**：包含 22 个子命令，聚合路由至离散分析（如 `contracts`, `health`, `churn`, `recent_changes`, `architecture`, `clones`, `untested`, `why`, `lint` 等）。

5. 修改、重构与验证

5.1 修改前评估：change 19 大操作
可用操作：`api_impact`（接口影响）, `code_actions`（快速修复）, `compare_branches`（分支比对）, `compare_overlay`（图层比对）, `contract`（变更契约）, `detect`（未提交改动）, `diagnostics`（诊断）, `edit_plan`（编辑计划）, `guards`（防护规则）, `impact`（爆炸半径）, `overlay_branches`（图层分支）, `overlay_state`（图层状态）, `pattern`（模式建议）, `preview`（效果预览）, `ranges`（范围符号）, `receipt`（落盘回执）, `simulate`（沙箱模拟）, `tests`（单测定位）, `verify`（签名校验）。

5.2 edit (10个) 与 refactor (6个) 代码修改
可用 edit 操作：`apply_overlay`（图层落盘）, `batch`（原子批量修改）, `docs`（文档生成）, `export_graph`（图谱导出）, `file`（文件编辑）, `scaffold`（代码脚手架）, `skill`（技能生成器）, `symbol`（AST 源码修改）, `wiki`（维基生成）, `write`（全文件覆盖写入）。
可用 refactor 操作：`apply_code_action`（应用修复动作）, `delete`（安全删除符号）, `fix_all`（全文件批量修复）, `inline`（内联符号）, `move`（移动符号）, `rename`（全图安全重命名）。

【核心契约与模板】
1. **`dry_run: true` 与 `physical_evidence: true` 互斥**：预览必须 `dry_run: true`（禁带 `physical_evidence`）；正式写入 `dry_run: false` 可开启 `physical_evidence: true`。
2. **语法门禁**：默认语法校验，草稿写入可传 `allow_parse_errors: true`；传 `base_sha` 防并发脏写。门面下支持 `match`/`replacement` 别名。
3. **标准化调用模板**：
   - 文件修改：预览 `edit_file(path="<f>", old_string="<o>", new_string="<n>", dry_run=true, expected_occurrences=1)`；写入改 `dry_run=false, physical_evidence=true`。
   - 符号修改：预览 `edit_symbol(id="<id>", old_source="<o>", new_source="<n>", dry_run=true)`；写入改 `dry_run=false, physical_evidence=true`。
   - 批量事务（支持 4 种 op）：`batch_edit(dry_run=true, edits=[{"op":"edit_file","path":"<f>","old_string":"<o>","new_string":"<n>"},{"op":"edit_symbol","id":"<id>","old_source":"<o>","new_source":"<n>"},{"op":"move_file","source":"<s>","destination":"<d>"},{"op":"delete_file","path":"<p>"}])`。
   - 移动与内联：`move_symbol(id="<id>", target_file="<path>", dry_run=true)`；`inline_symbol(id="<id>", dry_run=true)`。
   - 重命名与删除：`rename_symbol(id="<id>", new_name="<name>", dry_run=true)`；`safe_delete_symbol(id="<id>", dry_run=true, propagate=true)`。

5.3 修改后闭环验证
1. `detect_changes()`：获取变动符号集合。
2. `get_test_targets(ids="id1,id2")`：定位受波及单测。
3. `check_guards(ids="id1,id2")`：评估防护规则与架构边界。
4. `change_contract(...)`：评估变更风险契约与停止条件。
5. **真实测试执行**：在宿主环境运行构建与测试命令（`go test`, `npm test` 等）。

6. 状态机、隔离层与持久化记忆

6.1 overlay（10个）虚拟图层
操作：`register`（注册图层）, `push`（推送到分支）, `merge`（合并至内存，固定 to_disk=false）, `keepalive`（租期续签）, `switch`（切换分支）, `drop`（丢弃当前变动）, `drop_branch`（删除分支）, `fork`（派生分支）, `delete`（销毁图层）, `simulate`（图层沙箱模拟，固定 keep=true）。
- 落盘调用 `edit(operation="apply_overlay")`（固定 `to_disk=true`）。

6.2 recall (8个) 与 remember (8个) 记忆系统
- **recall**：`distill_session()`（会话摘要）；`surface_memories(task="...", symbol_ids="...")`（召回规约，固定 mark_accessed=false）；`query_notes(file_path="..." | symbol_id="...")`（查代码笔记）；`query_memories(query="...")`（查持久规约）；`notebook_find` / `notebook_list` / `notebook_show`（笔记本检索）；`check_onboarding_performed`（入职导引状态）。
- **remember**：`store_memory(kind="invariant", title="...", body="...")`（存关键架构不变量）；`save_note(file_path="...", body="...", tags="tag1,tag2")`（记录决策笔记）；`remember(operation="risk_ack")`（确认破坏性变动，固定 ack=true）；`edit_memory` / `rename_memory`（编辑规约）；`suppress_finding`（抑制已知告警）；`notebook_save` / `notebook_used`（笔记本持久化与续签）。

6.3 session（16个）会话控制与事件订阅
- **基础会话控制（6个）**：`agents`（多Agent协调，action: list|register|heartbeat|lock|unlock|unregister）, `cursor`（虚拟导航游标）, `planning_mode`（规划模式开关，arguments: {enabled:true}）, `proxy_enable` / `proxy_disable`（图代理开关）, `workflow`（工作流调度）。
- **事件订阅与退订（10个）**：针对 `daemon_health`, `diagnostics`, `graph_invalidated`, `stale_refs`, `workspace_readiness` 5 类事件的 `subscribe_<event>` / `unsubscribe_<event>`。

6.4 review (7个), pr (6个), response (5个)
- **review**：`review(scope="unstaged")`（审查未暂存改动）；`diff_context(scope="unstaged")`（审查 diff 拓扑上下文）；`review_pack`（打包审查套件）；`critique_review`（审查批判反思）；`pr_review_context`（PR 上下文）；`suggested_review_questions`（建议审查提问）；`sibling_diff_context`（同胞分支比对）。
- **pr**：`list_prs`（PR 列表）；`get_pr_impact`（PR 影响半径）；`conflicts_prs`（冲突分析）；`suggest_reviewers`（审查人推荐）；`pr_risk`（风险评分）；`triage_prs`（PR 分流）。
- **publish_review**：`post_review(pr=123, body="...")`。
- **response**：超大响应裁剪 `response(operation="slice", arguments={start:1, end:50})`；`grep`（正则过滤）；`peek`（首尾预览）；`stats`（句柄统计）；`export_context`（导出上下文）。

7. 通用响应控制与冲突裁决

7.1 通用 Output 响应塑形参数
门面与查询类扁平工具支持统一 `output` 控制对象与顶层参数：`max_bytes`（限制最大字节）、`limit`（限制最大条数）、`format`（`json`、`gcx`、`toon`，gcx 节约约 27% Token）、`cursor`（增量分页游标）、`fields`（稀疏列字段投影）、`scope`（已保存范围限定）。

7.2 冲突裁决优先级与分支视图（View）
- **多分支与工作树（View）治理**：当前工作区由 session/CWD 决定视图，显式检出分支构成自动 Overlay；跨分支只读审计传 `view:{kind:"worktree",checkout_id:"..."}` 或 `view:{kind:"git_ref",value:"refs/heads/release"}`；要求强一致性时传 `require_exact:true` 与 `require_fresh:true`。
- **自顶向下裁决优先级**：
  1. **当前工具返回的真实响应**（error、completion、view、guard、effect 信息）。
  2. **当前运行时 capabilities / Schema 规范**（request_shape、fixed_arguments、available）。
  3. **真实文件系统与代码仓库状态**（workspace、index、repository 物理状态）。
  4. **当前 Gortex 0.64.3 运行时源码实现**。
  5. **本规范文档**。

**多仓库全图谱最终铁律**：已 track 仓库全部统一托管于全局图谱。严禁在未经跨项目穿透检索或静默切域的情况下断定“未索引”并退回原生工具。只有在 `query_project` 与带前缀路径均证实未 track，且用户明确要求本地文件检查时，方可报告未 track 状态。