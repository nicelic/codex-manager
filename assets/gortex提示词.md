Gortex 使用规范

> 本规范面向使用 Gortex MCP 与 Agent 宿主适配器（Google Antigravity、Claude Code、Codex、Cursor 等）的代码 Agent。以 Gortex 0.64.2 运行时源码为权威依据。

1. 最高原则和使用边界

Gortex 是对其 track 仓库进行代码定位、源码阅读、关系分析、数据流追踪、影响评估、编辑、重构、验证、审查、项目管理和持久化记忆的权威工具。

- **优先原生 MCP 句柄与 Antigravity 宿主约束**：凡已 track 仓库，优先通过 Gortex 原生 MCP 句柄执行，避免退回宿主原生 `find_by_name`、`grep_search`、`view_file`、PowerShell 或 shell。若 MCP 不可达，报告 Gortex MCP integration failure。
- **0.64.2 全局技能只读豁免**：`read_file` 放行全局技能只读（`~/.agents/skills/*/SKILL.md`、`~/.config/opencode/skills/*/SKILL.md`、`~/.copilot/skills/*/SKILL.md`、`~/.claude/skills/*/SKILL.md`）。
- **严禁凭记忆捏造**：严禁伪造未返回的 symbol、关系、view、索引状态或安全结论。
- **关键实现体完整阅读与 Token 瘦身**：摘要和搜索结果不能替代关键实现阅读；行为关键代码读完整源码。长代码巡检时，可在 `read_file`、`get_symbol_source`、`get_editing_context` 中使用 `compress_bodies: true`（存根化削减 60%~70% Token，支持 `keep="f1,f2"` 保留指定函数）。
- **遵守写契约**：所有写操作遵守当前 schema、effect、fixed_arguments、guard、view、overlay 和 partial failure 结果。

1.1 协议名与双模感知（Core 扁平离散 vs Facade-v1 门面）

根据 MCP 连接预设不同，Gortex 暴露两种工具形态，以当前 session inventory 自适应路由：
1. **Core 扁平离散工具模式（Antigravity 默认预设）**：直接暴露 `explore`, `smart_context`, `get_repo_outline`, `read_file`, `get_symbol_source`, `get_callers`, `search_symbols`, `search_text`, `edit_file`, `batch_edit`, `query_project` 等工具。参数扁平传入。
   - **热核心集（Hot Eager Tools）**：常用基础工具开局立即可见可用。
   - **延迟目录（Deferred Catalog）**：部分冷工具（如 `search_ast`, `winnow_symbols` 等）托管于冷目录，若当前环境提示工具未暴露，通过 `tools_search(query="...")` 唤醒发现，或选用热集等价工具（如用 `analyze(kind="...")`）。
2. **Facade-v1 紧凑门面模式（21 个顶级工具）**：暴露 `read`, `explore`, `search`, `relations`, `trace`, `edit`, `change`, `refactor`, `workspace` 等门面，使用 `operation` 与 `target` 单一选择器规范。

- **多项目图谱一致性铁律**：Gortex 统一维护所有已 track 仓库的全局知识图谱。必须通过第 3.4 节机制在 Gortex 内精准执行。

2. 任务开启与探索流转（Explore & Discovery Workflow）

- **优先使用 explore 系列工具**：
  1. **代码定位与任务探索**：**优先使用 `explore(task="<完整问题>")`**（门面：`explore(operation="localize"|"task", task="...")`）。一键汇聚排查目标附近的符号、关键源码与调用链，内置 `completion` 状态机（`answer_ready`, `localized`, `needs_exact_read`, `needs_refinement` 等）辅助高效收敛。
  2. **智能上下文装配**：**优先使用 `smart_context(task="<任务>")`**（门面：`explore(operation="context", task="...")`）。装配任务最小完备上下文、关联拓扑与编辑规划。支持 `fidelity="graded"` 生成分级上下文清单，支持 `token_budget` 控制上下文开销，支持 `entry_point` 指定起始符号/文件。
  3. **架构大纲速览**：**优先使用 `get_repo_outline()`**（门面：`explore(operation="outline")`）。快速获取项目全局与核心模块骨架。

2.1 `explore` 9 大子命令（operations）深度指南
根据 `internal/mcp/facade_registry.go` 源码，9 个子命令精确职责与参数：
1. **`localize`（终结性定位）**：门面 `explore(operation="localize", task="...")`；扁平模式 `explore(task="...", path="...", token_budget=1600)`。专用于找代码位置与证据，强约束 terminality 状态机。
2. **`task`（诊断与任务分析）**：门面 `explore(operation="task", task="...")`；扁平模式 `explore(task="...")`。故障诊断与多步因果分析，持续汇聚证据链。
3. **`context`（智能上下文装配）**：门面 `explore(operation="context", task="...")`；扁平模式 `smart_context(task="...", entry_point="...", fidelity="graded", token_budget=8000)`。装配最小完备工作集，生成编辑建议。
4. **`outline`（工程骨架）**：门面 `explore(operation="outline")`；扁平模式 `get_repo_outline(repo="...")`。提取顶层包、模块分层大纲。
5. **`closure`（图依赖闭包）**：门面 `explore(operation="closure")`；扁平模式 `context_closure(ids="...")`。计算符号传递依赖闭包与相关邻接点。
6. **`plan`（变更规划推演）**：门面 `explore(operation="plan")`；扁平模式 `plan_turn(task="...")`。改动规划推演与多步实施步骤生成。
7. **`suggest`（查询词建议）**：门面 `explore(operation="suggest")`；扁平模式 `suggest_queries(query="...")`。将模糊自然语言映射为图谱高频规范词。
8. **`prefetch`（上下文预取）**：门面 `explore(operation="prefetch")`；扁平模式 `prefetch_context(task="...")`。后台预热邻居节点缓存。
9. **`wakeup`（图谱唤醒）**：门面 `explore(operation="wakeup")`；扁平模式 `gortex_wakeup()`。心跳自检与重新连接图谱。

2.2 检索、阅读与图谱协同
各工具正交协同，开发者与模型按需自由组合：
- **`search_text(query="...", regexp=false, path="...", repo="...")`**：Trigram 索引加速的全文/正则搜索。支持快速定位特征代码、字面量、日志报错与配置。**每条命中均附带 enclosing `symbol_id` 与 `symbol_name`**，可方便提取符号直接衔接图谱工具。
- **`search_symbols(query="...", kind="...", path="...")`**：按名称/驼峰快速查找函数、类、接口等 AST 定义。
- **`find_files(query="...", glob="*.*")`**：按路径前缀或 Glob 通配查找工程物理文件。
- **`get_callers` / `get_call_chain` / `find_usages` / `find_implementations`**：沿 AST 拓扑追踪上下游调用链、实现与引用点。
- **`read_file` / `get_symbol_source`**：阅读文件或具体函数实现源码，支持按需审视多文件上下文。巡检长文件建议配合 `offset/limit` 或 `compress_bodies`。

3. 双模调用对照字典与参数防错规范

3.1 双模核心对照表（基于 Gortex 0.64.2 源码事实）

Core 扁平离散模式下严格属性校验（`additionalProperties: false`）。**符号与图谱工具扁平入参属性名为 `id`**。若遇冷目录工具，调用 `tools_search` 唤醒或选用热核心等价工具。

| 功能场景 | Core 扁平工具 (Mode A) | Facade-v1 门面 (Mode B) | 说明 |
| :--- | :--- | :--- | :--- |
| **全局任务定位** | `explore(task="...")` | `explore(operation="localize"\|"task", task="...")` | [热核心] 优先使用，单任务入口 |
| **智能上下文装配** | `smart_context(task="...")` | `explore(operation="context", task="...")` | [热核心] 优先使用，装配上下文与编辑计划 |
| **工程骨架大纲** | `get_repo_outline()` | `explore(operation="outline")` | [热核心] 优先使用，模块架构大纲 |
| **架构统一分析** | `analyze(kind="architecture")` | `analyze(operation="architecture")` | [热核心] 系统分层与架构全景 |
| **符号精确搜索** | `search_symbols(query="...", kind="...")` | `search(operation="symbols", query="...")` | [热核心] 固定 assist=off |
| **文本全文搜索** | `search_text(query="...", regexp=false)` | `search(operation="text", query="...")` | [热核心] Trigram加速，命中带symbol_id |
| **查找文件** | `find_files(query="...", glob="*.*")` | `search(operation="files", query="...")` | [热核心] 文件通配查找 |
| **AST 语法反模式** | `analyze(kind="sast"\|"hygiene")` | `search(operation="ast", query="...")` | [热核心] 静态安全与规范审计 |
| **多轴约束过滤** | `winnow_symbols(text_match="...")` | `search(operation="winnow", query="...")` | [冷目录] 结合BM25与图属性精确过滤 |
| **读取非代码资产** | `get_artifact(id="..."\|path="...")` | `read(operation="artifact", target={artifact:"..."})` | [冷目录] 读取设计规范/文档 |
| **读取文件** | `read_file(path="...", offset=1, limit=100)` | `read(operation="file", target={file:"..."})` | [热核心] 支持 offset/limit/compress_bodies |
| **读取符号元数据/源码**| `get_symbol(id="<id>")` / `get_symbol_source(id="<id>")` | `read(operation="source", target={symbol:"<id>"})` | [热核心] 传规范 Node ID，读取定义/源码 |
| **批量读取符号源码**| `batch_symbols(symbols=["id1","id2"])` | `read(operation="symbols", target={symbols:[...]})` | [冷目录] 批量获取源码 |
| **单文件符号概览** | `get_file_summary(file="<path>")` | `read(operation="summary", target={file:"<path>"})` | [热核心] 文件符号摘要 |
| **编辑前拓扑分析** | `get_editing_context(file="<path>")` | `read(operation="editing_context", target={file:"..."})`| [热核心] 修改前获取依赖拓扑 |
| **反向调用者** | `get_callers(id="<id>", depth=2)` | `relations(operation="callers", target={symbol:"<id>"})`| [热核心] 查引用调用者 |
| **符号引用点** | `find_usages(id="<id>", context="call")` | `relations(operation="usages", target={symbol:"<id>"})` | [热核心] 语法级引用定位 |
| **接口实现查找** | `find_implementations(id="<id>")` | `relations(operation="implementations", target={symbol:"..."})` | [热核心] 查找接口实现 |
| **类/接口继承层次** | `get_class_hierarchy(id="<id>")` | `relations(operation="hierarchy", target={symbol:"<id>"})` | [冷目录] 继承体系展开 |
| **方法重写查找** | `find_overrides(id="<id>")` | `relations(operation="overrides", target={symbol:"<id>"})` | [热核心] 查虚函数/方法重写 |
| **声明跳转** | `find_declaration(id="<id>")` | `relations(operation="declaration", target={symbol:"<id>"})` | [冷目录] 引用点定位声明 |
| **前向/反向依赖** | `get_dependencies(id="<id>")` / `get_dependents(id="<id>")` | `relations(operation="dependencies"\|"dependents", target={symbol:"<id>"})` | [热核心] 依赖拓扑/评估爆炸半径 |
| **深度调用链路** | `get_call_chain(id="<id>", depth=4)` | `trace(operation="call_chain", target={symbol:"<id>"})` | [热核心] 递归调用路径追踪 |
| **控制流分析** | `get_cfg(id="<id>")` | `trace(operation="cfg", target={symbol:"<id>"})` | [冷目录] 控制流图分析 |
| **节点最短路径** | `trace_path(target="...", to="...")` | `trace(operation="path", target={...}, to={...})` | [冷目录] 拓扑最短关联路径 |
| **修改文件** | `edit_file(path="...", old_string="...", new_string="...", dry_run=true\|false)` | `edit(operation="file", target={file:"..."}, dry_run=true\|false)` | [热核心] 预览 dry_run=true 禁带凭据 |
| **精确修改符号** | `edit_symbol(id="...", old_source="...", new_source="...", dry_run=true\|false)` | `edit(operation="symbol", target={symbol:"..."}, dry_run=true\|false)` | [热核心] 精确修改 AST 节点 |
| **覆盖写入文件** | `write_file(path="...", content="...")` | `edit(operation="write", target={file:"..."}, content="...")` | [热核心] 全量覆盖写入 |
| **事务型批量修改** | `batch_edit(edits=[...], dry_run=true)` | `edit(operation="batch", changes=[...], dry_run=true)` | [热核心] 扁平入参名为 edits |
| **符号安全重命名** | `rename_symbol(id="...", new_name="...", dry_run=true)` | `refactor(operation="rename", target={symbol:"..."}, dry_run=true)`| [热核心] 全图引用联动重构 |
| **安全删除符号** | `safe_delete_symbol(id="...", dry_run=true)` | `refactor(operation="delete", target={symbol:"<id>"}, dry_run=true)`| [冷目录] 亦可通过 analyze(impact) 辅助 |
| **符号物理移动** | `move_symbol(id="...", destination="...", dry_run=true)` | `refactor(operation="move", target={symbol:"..."}, dry_run=true)` | [热核心] 跨文件移动符号(Go) |
| **内联符号** | `inline_symbol(id="...", dry_run=true)` | `refactor(operation="inline", target={symbol:"..."}, dry_run=true)` | [热核心] 内联展开符号(Go) |
| **守护规则检查** | `check_guards(ids="id1,id2")` | `change(operation="guards", target={symbols:[...]})` | [热核心] 逗号分隔 ids 字符串 |
| **受波及单测定位** | `get_test_targets(ids="id1,id2")` | `change(operation="tests", target={symbols:[...]})` | [热核心] 逗号分隔 ids 字符串 |
| **函数签名契约校验**| `verify_change(changes='[{"symbol_id":"...","new_signature":"..."}]')` | `change(operation="verify", source={changes:[...]})` | [热核心] 变更破坏性校验 |
| **变更影响评估** | `explain_change_impact(id="<id>")` / `analyze(kind="impact")` | `change(operation="impact", target={symbol:"<id>"})` | [热核心] 评估爆炸半径 |
| **变更风险契约** | `change_contract(diff="..."\|workspace_edit="...")` | `change(operation="contract", source={...})` | [冷目录] 预测风险与停止条件 |
| **LSP 编辑模拟** | `preview_edit(workspace_edit="...")` | `change(operation="preview", source={workspace_edit:"..."})` | [热核心] 专用于 WorkspaceEdit |
| **链式编辑沙箱模拟**| `simulate_chain(steps="[...]", keep=false)` | `change(operation="simulate")` | [热核心] 变更链路沙箱推演 |
| **检测未提交改动** | `detect_changes()` | `change(operation="detect")` | [热核心] 工作区脏改动检测 |
| **Diff 上下文提取**| `diff_context(scope="unstaged")` | `review(operation="diff_context", source={scope:"unstaged"})`| [热核心] review 变更上下文 |
| **图增强代码审查** | `review(scope="unstaged")` | `review(operation="run", source={scope:"unstaged"})` | [热核心] 综合审查引擎 |
| **持久化规约记忆** | `store_memory(kind="invariant", title="...", body="...")` | `remember(operation="memory", arguments={...})` | [热核心] 记录跨会话不变量 |
| **保存代码决策笔记**| `save_note(file="...", body="...", tags=[...])` | `remember(operation="note", arguments={...})` | [热核心] 局部决策标记 |
| **召回规约记忆** | `surface_memories(task="...")` | `recall(operation="surface", arguments={task:"..."})` | [热核心] 召回历史记忆 |
| **提炼会话摘要** | `distill_session()` | `recall(operation="distill")` | [热核心] 提炼关键上下文 |
| **跨项目穿透检索** | `query_project(project="...", query="...")` | `workspace(operation="project", project="...", query="...")` | [热核心] 跨库穿透免切检索 |
| **热切换活跃工程** | `set_active_project(project="...")` | `workspace_admin(operation="set_active_project", arguments={...})`| [热核心] 动态重定向主工程 |
| **当前活跃工程** | `get_active_project()` | `workspace(operation="active_project")` | [热核心] 查询当前绑定工程 |
| **已索引仓库列表** | `list_repos()` | `workspace(operation="repos")` | [热核心] 查询已 track 仓库 |
| **全局图谱统计** | `graph_stats()` | `workspace(operation="graph")` | [热核心] 节点边与容量统计 |
| **守护进程健康检查**| `index_health()` | `workspace(operation="index")` | [热核心] 索引健康度诊断 |
| **发现延迟目录工具**| `tools_search(query="...")` | `capabilities(operation="legacy_search")` | [热核心] 动态激活冷目录工具 |

3.2 参数容器与 arguments 规范

- **冷门面**（`publish_review`, `pr`, `recall`, `remember`, `workspace`, `workspace_admin`, `overlay`, `response`, `session`）声明了 `arguments` 属性，参数封装在 `arguments: {...}` 中。
- **热门面**（`explore`, `search`, `read`, `relations`, `trace`, `analyze`, `ask`, `change`, `review`, `edit`, `refactor`）未声明 arguments 属性，**严禁**包裹外层 `arguments`，参数放顶层或对应容器（`target` / `options` / `guard` / `source` / `context`）。

3.3 固定参数（fixed_arguments）全景

以下参数由运行时强制固化：
- `search.symbols`: 固定 `assist=off`。
- `analyze.co_change`: 固定 `refresh=false`。
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
- `ast`：通过语法模式或检测器检索（支持 `pattern`, `detector`, `language`）。
- `artifacts`：搜设计文档、架构规范、OpenAPI 模式等非代码资产（支持 `query`, `kind`）。
- `winnow`：多轴结构化检索，结合 BM25 文本与图属性筛选（支持 `text_match`、`kind`、`language`、`path_prefix`、`min_fan_in`、`limit` 等）。

4.2 read 7 大操作与单一选择器
可用操作：`artifact`, `editing_context`, `file`, `history`, `source`, `summary`, `symbols`。
- **选择器单一性**：`target` 必须为对象且有且仅能有一个键（`file`, `symbol`, `symbols`, `query`, `artifact`, `repo` 选 1 个）。
- `read.file` 支持 `options={offset:1, limit:100}` 或 `context={start_line:1, end_line:100}`。
- `read.source`：精读符号实现体，可指定 `context_lines` 扩展外围行数。

4.3 relations (11个) 与 trace (7个)
- **relations（11个）**：`callers`（调用者）, `cluster`（连通社区）, `declaration`（声明定位）, `dependencies`（前向依赖）, `dependents`（反向依赖）, `hierarchy`（类继承树）, `implementations`（接口实现）, `import_path`（导入路径）, `overrides`（虚方法重写）, `references`（引用完整性）, `usages`（代码使用点）。
- **trace（7个）**：`call_chain`（调用链路追踪）, `cfg`（控制流图）, `flow`（端到端数据流向）, `graph`（通用图谱查询）, `path`（节点最短路径）, `taint`（污点传播分析）, `walk`（图漫游拓扑步进）。

4.4 analyze 只读统一分析核心 Kind 全景
`analyze` 内置 78 种分析 kind，核心速查：
- **拓扑结构**：`architecture`（分层依赖边界）, `cycles`（循环依赖诊断）, `would_create_cycle`（成环预测）, `components`/`clusters`（连通分量与社区发现，算法支持 leiden/louvain）, `suggest_boundaries`（模块边界重构建议）, `hotspots`（复杂度与修改热点）。
- **质量与健康**：`dead_code`（死代码检测）, `untested`（未测符号统计）, `coverage_gaps`（未覆盖盲区）, `clones`（重复代码比对）, `churn`（代码抖动率）, `todos`（待办事项汇总）, `doc_staleness`（文档过时分析）。
- **并发与语言**：`race_writes`（并发写竞态）, `channel_ops`/`unclosed_channels`（Go通道泄漏）, `goroutine_spawns`（协程衍生追踪）, `cgo_users`（Cgo边界审计）, `error_surface`（错误暴露面）。
- **安全与框架**：`sast`（静态安全漏洞，支持 cwe 过滤）, `hygiene`（代码坏味道）, `unsafe_patterns`（高危 API 模式）, `routes`（已注册 HTTP/RPC 路由）, `models`（数据表实体模型）。
- **变动管控**：`blame`, `coverage`, `sql_rebuild`, `temporal_verify` 需通过 `workspace_admin` 授权执行。

5. 修改、重构与验证

5.1 修改前评估：change 19 大操作
可用操作：`api_impact`（公共接口影响）, `code_actions`（快速修复动作）, `compare_branches`（分支比对）, `compare_overlay`（图层比对）, `contract`（变更契约评估）, `detect`（未提交改动检测）, `diagnostics`（LSP 诊断信息）, `edit_plan`（编辑计划生成）, `guards`（防护规则检查）, `impact`（爆炸半径评估）, `overlay_branches`（图层分支列表）, `overlay_state`（图层状态列表）, `pattern`（模式建议）, `preview`（LSP 效果预览）, `ranges`（范围对应符号）, `receipt`（修改落盘回执）, `simulate`（链式沙箱模拟）, `tests`（单测目标定位）, `verify`（签名破坏性校验）。

5.2 edit (10个) 与 refactor (6个) 代码修改
可用 edit 操作：`apply_overlay`（图层写入磁盘）, `batch`（原子事务批量修改）, `docs`（文档自动生成）, `export_graph`（图谱导出）, `file`（单文件编辑）, `scaffold`（代码脚手架生成）, `skill`（技能生成生成器）, `symbol`（AST 符号源码修改）, `wiki`（项目维基生成）, `write`（全文件覆盖写入）。
可用 refactor 操作：`apply_code_action`（应用修复动作）, `delete`（安全删除符号）, `fix_all`（全文件批量修复）, `inline`（内联符号）, `move`（移动符号）, `rename`（全图安全重命名）。

【核心契约与模板】
1. **`dry_run: true` 与 `physical_evidence: true` 互斥**：预览必须 `dry_run: true`（禁带 `physical_evidence`）；正式写入 `dry_run: false` 可开启 `physical_evidence: true`。
2. **语法门禁**：默认语法校验，写入草稿片段可传 `allow_parse_errors: true`；传 `base_sha` 防并发脏写。
3. **标准化调用模板**：
   - 文件修改：预览 `edit_file(path="<f>", old_string="<o>", new_string="<n>", dry_run=true, expected_occurrences=1)`；写入改 `dry_run=false, physical_evidence=true`。
   - 符号修改：预览 `edit_symbol(id="<id>", old_source="<o>", new_source="<n>", dry_run=true)`；写入改 `dry_run=false, physical_evidence=true`。
   - 批量事务：`batch_edit(dry_run=true, edits=[{"op":"edit_file","path":"<f>","old_string":"<o>","new_string":"<n>"},{"op":"edit_symbol","id":"<id>","old_source":"<o>","new_source":"<n>"}])`。
   - 重命名与删除：`rename_symbol(id="<id>", new_name="<name>", dry_run=true)`；`safe_delete_symbol(id="<id>", dry_run=true, propagate=true)`。

5.3 修改后闭环验证
1. `detect_changes()`：获取变动符号集合。
2. `get_test_targets(ids="id1,id2")`：定位受波及单测。
3. `check_guards(ids="id1,id2")`：评估防护规则。
4. **真实测试执行**：在宿主环境运行构建与测试命令（`go test`, `npm test` 等）。

6. 状态机、隔离层与持久化记忆

6.1 overlay（10个）虚拟图层
操作：`register`（注册图层）, `push`（推送到分支）, `merge`（合并至会话内存，固定 to_disk=false）, `keepalive`（租期续签）, `switch`（切换图层分支）, `drop`（丢弃当前变动）, `drop_branch`（删除指定分支）, `fork`（派生新分支）, `delete`（销毁图层会话）, `simulate`（图层沙箱模拟并持久化，固定 keep=true）。
- 落盘调用 `edit(operation="apply_overlay")`（固定 `to_disk=true`）。

6.2 recall (8个) 与 remember (8个) 记忆系统
- **recall**：`distill_session()`（提炼会话摘要）；`surface_memories(task="...")`（召回关键规约，固定 mark_accessed=false）；`query_notes(file="..." | symbol_id="...")`（查代码笔记）；`query_memories(query="...")`（查持久规约）；`notebook_find` / `notebook_list` / `notebook_show`（笔记本检索）；`check_onboarding_performed`（新手入职导引状态）。
- **remember**：`store_memory(kind="invariant", title="...", body="...")`（存关键架构不变量）；`save_note(file="...", body="...", tags=[...])`（记录决策笔记）；`remember(operation="risk_ack")`（确认破坏性变动，固定 ack=true）；`edit_memory` / `rename_memory`（编辑规约）；`suppress_finding`（抑制已知告警）；`notebook_save` / `notebook_used`（笔记本持久化）。

6.3 session（8个）会话控制与事件订阅
操作：`agents`（多Agent协调，action: list|register|heartbeat|lock|unlock|unregister）, `cursor`（虚拟导航游标）, `planning_mode`（规划模式开关，arguments: {enabled:true}）, `proxy_enable` / `proxy_disable`（图代理开关）, `subscribe` / `unsubscribe`（后台事件订阅，频道：`daemon_health`, `diagnostics`, `graph_invalidated`, `stale_refs`, `workspace_readiness`）, `workflow`（工作流管道调度）。

6.4 review (7个), pr (6个), response (5个)
- **review**：`review(scope="unstaged")`（审查未暂存改动）；`diff_context(scope="unstaged")`（审查 diff 拓扑上下文）；`review_pack`（打包审查套件）；`critique_review`（审查批判反思）；`pr_review_context`（PR 上下文）；`suggested_review_questions`（建议审查提问）；`sibling_diff_context`（同胞分支比对）。
- **pr**：`list_prs`（PR 列表）；`get_pr_impact`（PR 影响半径）；`conflicts_prs`（冲突分析）；`suggest_reviewers`（审查人推荐）；`pr_risk`（风险评分）；`triage_prs`（PR 分流）。
- **publish_review**：`post_review(pr=123, body="...")`。
- **response**：超大响应裁剪 `response(operation="slice", arguments={start:1, end:50})`；`response(operation="grep", arguments={pattern:"..."})`；`response(operation="export_context")`。

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
