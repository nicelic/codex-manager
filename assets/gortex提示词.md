# Gortex 使用规范

## 1. 适用范围和最高原则

Gortex 是本项目进行代码导航、检索、关系分析、数据流追踪、影响评估、编辑、重构、验证、审查、项目管理和持久化记忆的权威工具。

对于已经被 Gortex track 的仓库：

- 优先直接调用 Gortex 原生 MCP 工具。
- 不要使用 Read、Grep、Glob、rg、find、PowerShell 或其他 shell 命令替代 Gortex 的索引检索、符号搜索、调用关系、影响分析、编辑、重构、guard 或 contract 检查。
- 不要根据记忆猜测不存在的工具、operation、参数或字段。
- 不要伪造 Gortex 没有返回的文件、符号、调用关系、索引状态、测试结果或安全结论。
- 关系图、摘要和检索结果只能缩小范围，不能代替关键实现体的阅读。
- 任何写操作都必须遵守工具返回的 schema、guard、view 和 effect 约束。

如果 Gortex MCP 已配置，但当前会话没有出现对应的原生工具：

1. 报告 `Gortex MCP integration failure`。
2. 停止当前操作。
3. 不要手动启动 daemon。
4. 不要自动切换到 `gortex call`、CLI、PowerShell 或其他 shell 读取方式。

Gortex daemon 可能管理多个仓库。开始工作前必须确认 workspace、active project、tracked repository 和实际 view；不能假定当前仓库是唯一仓库。

## 2. 任务开始和上下文恢复

每个新的编码、诊断、审查或分析任务，第一步使用：

```text
explore(operation="task", task="<完整用户任务、错误现象、约束和只读/写入要求>")
```

如果任务只是寻找文件、符号或证据，使用：

```text
explore(operation="localize", task="<完整问题>")
```

如果用户已经明确给出要读取的文件路径，并且任务只是读取、总结或审查该文件，可以直接使用：

```text
read(operation="file", target={file:"<path>"}, options={new_user_task:true})
```

`explore(operation="localize")` 返回的 `completion` 是终止契约：

- 必须遵守 `completion.required_action` 和 `completion.final_response`。
- 若状态为 `answer_ready`，直接依据 `completion.final_response` 回答。
- 不要继续调用其他工具，也不要重复读取已经返回的文件或符号。

上下文压缩、恢复会话或进入已修改过的仓库时，优先使用：

```text
recall(operation="distill")
```

如果任务涉及已有设计决策、事故经验或约束，在 `explore` 或 `smart_context` 后按需使用：

```text
recall(operation="surface", task="<当前任务>", target={symbols:["<相关symbol-id>"]})
```

## 3. facade-v1 的 21 个公共 MCP 工具

启用 facade-v1/compact surface 时，Gortex 的公共 MCP facade 包含以下 21 个工具：

```text
analyze
ask
capabilities
change
edit
explore
overlay
pr
publish_review
read
recall
refactor
relations
remember
response
review
search
session
trace
workspace
workspace_admin
```

工具名称是固定的；工具的子操作必须以 `capabilities` 返回的 schema 为准。不要把旧兼容工具名当成新的 facade 工具，也不要自行发明工具名。

## 4. capabilities：发现 operation 和精确 schema

`capabilities` 不负责动态创造新工具名称，而是发现已存在工具的 domain、operation 和参数 schema：

```text
capabilities()
```

列出所有 domain。

```text
capabilities(domain="<tool>")
```

列出指定工具的 operation。

```text
capabilities(domain="<tool>", operation="<operation>", detail="schema")
```

获取指定 operation 的精确输入 schema。

```text
capabilities(domain="<tool>", operation="<operation>", detail="summary")
```

获取指定 operation 的摘要。

当字段、嵌套对象、固定值、必填字段或 effect 不确定时，先查询 schema，不得猜测。

## 5. explore 的全部 operation

```text
explore(operation="closure", ...)
explore(operation="context", ...)
explore(operation="localize", ...)
explore(operation="outline", ...)
explore(operation="plan", ...)
explore(operation="prefetch", ...)
explore(operation="suggest", ...)
explore(operation="task", ...)
explore(operation="wakeup", ...)
```

`localize` 是定位后终止的导航模式；`task` 用于继续诊断或实施；`context`、`closure`、`outline`、`prefetch`、`suggest`、`plan` 和 `wakeup` 按任务需要使用。

## 6. search 的全部 operation

```text
search(operation="artifacts", query="<知识文件或 manifest artifact>")
search(operation="ast", query="<结构模式>")
search(operation="completion", query="<名称或概念>")
search(operation="files", query="<文件名>")
search(operation="symbols", query="<符号名或概念>")
search(operation="text", query="<字面量或正则文本>")
search(operation="winnow", query="<结构化约束链>")
```

- `symbols` 用于符号发现，公共 facade 固定为 `assist="off"`。
- `text` 用于索引仓库中的字面量或正则检索。
- `files` 用于按文件名查找。
- `artifacts` 用于 `.gortex.yaml::artifacts` 中的非代码知识文件。
- `completion` 是图扩展检索，不等于普通文本搜索。
- `winnow` 用于结构化约束链检索。

## 7. read 的全部 operation

```text
read(operation="artifact", ...)
read(operation="editing_context", target={file:"<path>"})
read(operation="file", target={file:"<path>"})
read(operation="history", ...)
read(operation="source", target={symbol:"<symbol-id>"})
read(operation="summary", target={file:"<path>"})
read(operation="symbols", target={symbols:["<id1>","<id2>"]})
```

- `file` 读取文件内容。
- `source` 读取单个符号实现体。
- `symbols` 批量读取签名、源码和有限的一跳关系。
- `summary` 读取文件定义的符号概览。
- `editing_context` 是修改文件前的主要上下文工具。
- `artifact` 读取 manifest artifact 及其关联内容。
- `history` 读取当前会话中符号的修改记录。

以下代码必须尽量读取完整函数体，不要只依赖摘要：数据库迁移、重试/回退/错误恢复、并发/锁/goroutine、兼容性分支、权限和安全边界、文件写入、网络调用、事务和状态机。行为关键代码不要使用 `compress_bodies:true`。

## 8. relations 的全部 operation

```text
relations(operation="callers", ...)
relations(operation="cluster", ...)
relations(operation="declaration", ...)
relations(operation="dependencies", ...)
relations(operation="dependents", ...)
relations(operation="hierarchy", ...)
relations(operation="implementations", ...)
relations(operation="import_path", ...)
relations(operation="overrides", ...)
relations(operation="references", ...)
relations(operation="usages", ...)
```

典型调用：

```text
relations(operation="usages", target={symbol:"<id>"})
relations(operation="callers", target={symbol:"<id>"})
relations(operation="dependencies", target={symbol:"<id>"})
relations(operation="dependents", target={symbol:"<id>"})
relations(operation="implementations", target={symbol:"<id>"})
```

`relations` 结果用于定位和缩小范围，不能替代关键源码阅读。

## 9. trace 的全部 operation

```text
trace(operation="call_chain", target={symbol:"<id>"})
trace(operation="cfg", target={symbol:"<id>"})
trace(operation="flow", target={symbol:"<source-id>"}, to={symbol:"<sink-id>"})
trace(operation="graph", ...)
trace(operation="path", ...)
trace(operation="taint", ...)
trace(operation="walk", ...)
```

- `call_chain` 追踪调用图。
- `path` 查询最短调用路径并在不可达时返回原因。
- `flow` 查询两个符号之间的数据流路径。
- `taint` 通过 source/sink pattern 扫描数据流。
- `cfg` 返回函数内部控制流和 def-use 信息。
- `graph` 是只读图查询 DSL。
- `walk` 是有 token 预算的自由图遍历。

## 10. analyze 和 ask

`analyze` 是统一分析门面，`kind` 数量很多，不能凭记忆猜测。先使用：

```text
capabilities(domain="analyze")
```

再根据返回 schema 调用。可用分析包括但不限于：

```text
analyze(kind="architecture", ...)
analyze(kind="cycles", ...)
analyze(kind="dead_code", ...)
analyze(kind="health", ...)
analyze(kind="impact", ...)
analyze(kind="sast", ...)
analyze(kind="coverage_gaps", ...)
analyze(kind="race_writes", ...)
analyze(kind="untested", ...)
```

公共 `analyze` 是只读分析边界。需要改变图状态的 `blame`、`coverage`、`sql_rebuild`、`temporal_verify` 应通过 `workspace_admin` 的固定 operation 使用，不得伪装成普通只读分析。

研究型问题使用：

```text
ask(question="<需要研究的问题>", options={...}, output={...})
```

`ask` 没有通用 `operation` 字段；不要写成 `ask(operation="research")`，除非当前 schema 明确要求。

## 11. workspace、项目和 view

仓库、项目、worktree 或索引范围不明确时，先使用：

```text
workspace(operation="info")
workspace(operation="repos")
workspace(operation="active_project")
workspace(operation="graph")
workspace(operation="index")
workspace(operation="checkouts")
workspace(operation="project")
workspace(operation="proxy")
workspace(operation="scopes")
```

`workspace` 的完整 operation：

```text
active_project
checkouts
graph
index
info
project
proxy
repos
scopes
```

不要随意向工具传递未经 schema 允许的 `repo`、`cwd`、`workspace` 或 `root` 字段。如果仓库尚未 track：

- 明确报告“该仓库不在 Gortex 索引范围内”。
- 不要假装已经完成图分析。
- 只有用户明确要求管理索引时，才使用 `workspace_admin(operation="track")`、`index` 或 `reindex`。

view 结果应检查：

```text
exact
actual_view
requested_view
fallback_reason
view_fingerprint
resolved_ref
resolved_commit
```

如果 `exact:false`，必须说明结果是 fallback 只读结果。严格匹配使用 `require_exact:true`；等待最新文件状态使用 `require_fresh:true` 和绝对 RFC3339 `wait_deadline`。

## 12. 修改前的影响分析和契约验证

任何代码、配置或文档写入前，先执行：

```text
change(operation="impact", target={symbol:"<id>"})
```

批量目标：

```text
change(operation="impact", target={symbols:["<id1>","<id2>"]})
```

公共 API、函数签名、接口或类型契约变化，还必须使用：

```text
change(
  operation="verify",
  source={changes:[{symbol_id:"<id>", new_signature:"<完整新签名>"}]}
)
```

路由处理器或公共 API 边界变化，按需使用：

```text
change(operation="api_impact", ...)
```

`change` 的完整 operation：

```text
api_impact
code_actions
compare_branches
compare_overlay
contract
detect
diagnostics
edit_plan
guards
impact
overlay_branches
overlay_state
pattern
preview
ranges
receipt
simulate
tests
verify
```

`change(operation="contract")` 是风险和契约审查，不是持久化风险确认；它的公共 facade 默认 `ack:false`。确实需要确认持久化风险时，使用 `remember(operation="risk_ack")`，不得猜测 `ack:true` 绕过安全边界。

## 13. edit 和 refactor

`edit` 的完整 operation：

```text
edit(operation="apply_overlay", ...)
edit(operation="batch", ...)
edit(operation="docs", ...)
edit(operation="export_graph", ...)
edit(operation="file", ...)
edit(operation="scaffold", ...)
edit(operation="skill", ...)
edit(operation="symbol", ...)
edit(operation="wiki", ...)
edit(operation="write", ...)
```

文本或文件级修改使用 `edit(file|write|batch|apply_overlay)`；符号级修改使用 `edit(symbol)`。涉及整个文件的 `move_file`/`delete_file` 只能作为 `edit(batch)` 的批量项，它们不会自动重写调用者或 import。

语义重构使用：

```text
refactor(operation="apply_code_action", ...)
refactor(operation="delete", ...)
refactor(operation="fix_all", ...)
refactor(operation="inline", ...)
refactor(operation="move", ...)
refactor(operation="rename", ...)
```

优先先使用 `dry_run:true` 预览；确认计划、目标和 guard 后再执行 `dry_run:false`。工具返回 `base_sha`、`content_sha256`、`before_sha256` 或 `etag` 时，尽量使用对应 guard 防止覆盖并发修改。

## 14. 修改后的验证流程

修改完成后必须执行：

```text
change(operation="detect")
```

从结果中提取受影响的 symbol IDs，再执行：

```text
change(operation="tests", target={symbols:["<affected-ids>"]})
change(operation="guards", target={symbols:["<affected-ids>"]})
change(operation="contract", target={symbols:["<affected-ids>"]})
```

按需使用：

```text
change(operation="diagnostics", ...)
change(operation="code_actions", ...)
change(operation="receipt", ...)
```

`change(operation="tests")` 只返回测试目标或建议命令，不代表测试已经执行。必须使用项目真实的构建/测试命令运行测试，并在最终回答中说明执行的命令、结果，或未执行的具体原因。

## 15. recall、remember 和长期记忆

`recall` 的完整 operation：

```text
recall(operation="distill", ...)
recall(operation="memories", ...)
recall(operation="notebook_find", ...)
recall(operation="notebook_list", ...)
recall(operation="notebook_show", ...)
recall(operation="notes", ...)
recall(operation="onboarding", ...)
recall(operation="surface", ...)
```

`remember` 的完整 operation：

```text
remember(operation="edit_memory", ...)
remember(operation="memory", ...)
remember(operation="note", ...)
remember(operation="notebook", ...)
remember(operation="notebook_used", ...)
remember(operation="rename_memory", ...)
remember(operation="risk_ack", ...)
remember(operation="suppress_finding", ...)
```

使用 `note` 保存当前会话的决定、临时约束、未完成事项和 bug 复现条件；使用 `memory` 保存跨会话的不变量、架构约束、安全规则、团队约定、兼容性要求和已确认 incident 经验。

在修改以前曾经修改过的符号前，先查询相关 notes 或 memories。不要保存能直接从 diff、图或现有项目文档中得到的重复事实。

## 16. overlay

overlay 是会话级缓冲区/分支状态，不等于已经写入磁盘。完整 operation：

```text
overlay(operation="delete", ...)
overlay(operation="drop", ...)
overlay(operation="drop_branch", ...)
overlay(operation="fork", ...)
overlay(operation="keepalive", ...)
overlay(operation="merge", ...)
overlay(operation="push", ...)
overlay(operation="register", ...)
overlay(operation="simulate", ...)
overlay(operation="switch", ...)
```

不存在 `overlay(operation="list")`。查询 overlay 状态应使用：

```text
change(operation="overlay_state", ...)
change(operation="overlay_branches", ...)
```

比较基础图与 overlay：

```text
change(operation="compare_overlay", ...)
change(operation="compare_branches", ...)
```

只推演 WorkspaceEdit 而不写磁盘：

```text
change(operation="preview", ...)
change(operation="simulate", ...)
```

`change(simulate)` 的公共默认 `keep:false`；需要把推演保存在 session overlay 时，使用 `overlay(simulate)` 的 `keep:true` 语义，并以实际 schema 为准。

执行 primary closure、family forget 或 set-primary 等高影响操作前，必须先预览、说明影响范围并获得用户明确确认。

## 17. review、PR 和外部写操作

`review` 的完整 operation：

```text
review(operation="critique", ...)
review(operation="diff_context", ...)
review(operation="pack", ...)
review(operation="pr_context", ...)
review(operation="questions", ...)
review(operation="run", ...)
review(operation="sibling_context", ...)
```

`pr` 的完整 operation：

```text
pr(operation="conflicts", ...)
pr(operation="impact", ...)
pr(operation="list", ...)
pr(operation="reviewers", ...)
pr(operation="risk", ...)
pr(operation="triage", ...)
```

向远程 Forge 发布审查结果属于外部写操作，只能使用：

```text
publish_review(operation="post", ...)
```

发布前必须说明目标仓库、PR、评论范围和外部副作用。

## 18. session 和 response

`session` 的完整 operation：

```text
session(operation="agents", ...)
session(operation="cursor", ...)
session(operation="planning_mode", ...)
session(operation="proxy_disable", ...)
session(operation="proxy_enable", ...)
session(operation="subscribe", channel="<channel>", ...)
session(operation="unsubscribe", channel="<channel>", ...)
session(operation="workflow", ...)
```

订阅 channel 必须以 schema 为准；当前支持的 channel 包括：

```text
daemon_health
diagnostics
graph_invalidated
stale_refs
workspace_readiness
```

`session(operation="planning_mode")` 的 planning 模式会移除并阻止编辑工具；不要把 session 状态变化误认为代码已经修改。

`response` 的完整 operation：

```text
response(operation="export_context", ...)
response(operation="grep", ...)
response(operation="peek", ...)
response(operation="slice", ...)
response(operation="stats", ...)
```

响应被截断或需要重新查看时，优先分页并使用 `max_bytes`、`max_tokens`、`cursor` 或 `fields`，不要重复执行同一查询。

## 19. workspace_admin

`workspace_admin` 是控制写边界，必须确认用户确实要求管理索引、项目或持久化状态。完整 operation：

```text
workspace_admin(operation="blame", ...)
workspace_admin(operation="coverage", ...)
workspace_admin(operation="delete_scope", ...)
workspace_admin(operation="enrich_churn", ...)
workspace_admin(operation="enrich_releases", ...)
workspace_admin(operation="feedback", ...)
workspace_admin(operation="index", ...)
workspace_admin(operation="reindex", ...)
workspace_admin(operation="save_scope", ...)
workspace_admin(operation="set_active_project", ...)
workspace_admin(operation="sql_rebuild", ...)
workspace_admin(operation="temporal_verify", ...)
workspace_admin(operation="track", ...)
workspace_admin(operation="untrack", ...)
```

尤其注意：`track`、`untrack`、`set_active_project`、`delete_scope`、`sql_rebuild` 和 `temporal_verify` 会改变持久化状态或索引，不能因为看到 worktree 或查询失败就自动调用。

## 20. 分析资源 URI

如果宿主支持 MCP resources，优先读取资源；资源不是普通工具调用，也不要把资源 URI 当成 `analyze` operation。固定资源包括：

```text
gortex://active-project
gortex://audit
gortex://god-nodes
gortex://guide
gortex://index-health
gortex://questions
gortex://repos
gortex://report
gortex://schema
gortex://session
gortex://stats
gortex://surprises
gortex://workspace
```

可用资源模板包括：

```text
gortex://communities
gortex://community/{id}
gortex://guide/{topic}
gortex://process/{id}
gortex://processes
```

若宿主不支持 resources，使用 `capabilities`、`analyze` 或 `workspace` 的实际可用 operation 替代，不得假装资源已经读取成功。

## 21. CLI 镜像和 Windows PowerShell 例外

原生 Gortex MCP 可用时，必须直接调用 MCP，不能通过 PowerShell 中转。

只有同时满足以下条件，才允许 PowerShell：

1. 目标仓库或文件明确未被 Gortex track；
2. 用户明确允许绕过 Gortex；
3. 操作只是只读检查，或用户明确要求执行外部命令；
4. 不会把 PowerShell 结果伪装成 Gortex 图分析结果。

PowerShell 可用于检查未索引目录、读取原始配置、验证 Windows 环境和执行项目真实测试命令；不得用于绕过已索引代码的符号搜索、关系查询、影响分析、编辑、重构、guard 或 contract 检查。

没有原生 MCP 且用户明确允许 CLI 镜像时，Windows 优先使用：

```text
gortex.exe mcp
gortex.exe call <tool>
gortex.exe tools
gortex.exe doctor
gortex.exe status
gortex.exe version
```

`gortex.exe call <tool>` 的常用参数：

```text
--json
--json-file
--arg
--dry
--format
--legacy
```

示例：

```powershell
gortex.exe call read --json '{"operation":"file","target":{"file":"README.md"}}'
```

`gortex.exe call` 需要 daemon 和 tracked repository；`--dry` 可在不调用 daemon 时检查降阶后的参数对象。不要发明 `gortex <tool>` 这种未注册的顶级命令。

## 22. Windows / Codex 运行约束

Windows 10 使用 Gortex 时确认：

- `gortex.exe` 位于 PATH 中，或 MCP 配置使用绝对路径；
- Codex MCP 配置位于 `~\.codex\config.toml`；
- Codex 全局提示词位于 `~\.codex\AGENTS.md`；
- 若存在 `~\.codex\AGENTS.override.md`，确认它没有覆盖本规范；
- MCP server 使用 `command="...\\gortex.exe"`、`args=["mcp"]`；
- `features.code_mode.direct_only_tool_namespaces` 如已配置，应包含 `mcp__gortex` 和 `gortex`；
- Hooks 未被配置为禁用；
- 新增或修改 Hooks 后，在 Codex 中运行 `/hooks`，检查并信任 Gortex Hooks；
- 使用 `gortex.exe doctor --json` 检查配置、Hook 活动、daemon、索引和采用率；
- 如果 MCP 配置注入了自定义 `GORTEX_DAEMON_*` 或 `XDG_*` 路径，运行 doctor/status 时必须使用同一组环境变量，否则可能误报 daemon 不运行。

提示词只能约束行为，不能代替 MCP 配置、Hook 信任、PATH、权限、tracked 状态或 daemon 的实际可用性。

## 23. 更新补充规范

本节针对当前 Gortex 的源码、MCP facade、CLI 和 Agent 集成行为进行补充。若本节与上游实际返回的 capability/schema 不一致，以当前运行实例的 Gortex MCP `capabilities`、workspace 状态和具体工具返回为准；不得凭记忆补写参数。

### 23.1 View 路由与写入边界

Gortex 请求可以通过通用 `view` 选择器指定图视图：

```text
view={"kind":"auto"}
view={"kind":"base","graph_id":"<graph-id>"}
view={"kind":"worktree","checkout_id":"<checkout-id>"}
view={"kind":"worktree","path":"<absolute-worktree-path>"}
view={"kind":"git_ref","value":"refs/heads/<branch>"}
view={"kind":"commit","value":"<full-lowercase-object-id>"}
```

可用的通用视图控制包括：

```text
require_exact
require_fresh
wait_deadline   # 必须使用绝对 RFC3339 时间
require_complete
required_capabilities
optional_capabilities
```

必须检查并在需要时报告 `exact`、`actual_view`、`requested_view`、`fallback_reason`、`view_fingerprint`、`resolved_ref` 和 `resolved_commit`。`exact:false` 表示发生了 fallback；fallback 视图永远只读。`git_ref`、`commit` 以及 fallback 视图不能写入。显式 worktree 写入仅允许 coordinator-backed 且 exact 的路径；不能把任意路径或只读图当作可写目标。

### 23.2 `explore` 请求形状

- `explore.task` 使用顶层 `task` 字段。
- `explore.localize` 的 capability request shape 为：

```text
explore(operation="localize", options={task:"<完整问题>"})
```

- `options.new_user_task=true` 只用于新用户请求第一次调用的 `explore.task`、`explore.localize` 或 `read.file`，不要在后续分页或重复读取中反复设置。

### 23.3 Compact facade 与实际 MCP surface

21 个 facade-v1 公共工具仍为：

```text
analyze ask capabilities change edit explore overlay pr publish_review
read recall refactor relations remember response review search session trace
workspace workspace_admin
```

但 compact/facade-v1 并非所有连接都会自动启用：

- 非空 `clientInfo.name` 的 MCP 初始化连接默认使用 compact/facade-v1 hide 行为。
- 空或尚未初始化的 session 可能保留 server default。
- 当前 server fallback 可能是 `core + defer`（约 34 个 eager 工具），不能把它误写成固定的 21 工具 surface。
- facade aliases 包括 `compact`、`facade` 和 `agent-v2`。
- legacy presets 仍可能存在：`agent`、`core`、`full`、`readonly`、`edit`、`nav`、`localization`。

因此，开始任务时必须以当前 session 的实际 tool inventory 和 `capabilities` 为准；不要假设某个 preset、工具或别名一定存在。

### 23.4 Checkout 管理与 legacy MCP 工具

`workspace.checkouts` 只映射 checkout 列表查询。以下能力仍是独立的 legacy MCP 工具，不属于 21 个 facade 工具：

```text
set_primary_checkout
forget_checkout
reconcile_checkouts
explain_view
```

对应 CLI 为：

```text
gortex repos families
gortex repos set-primary
gortex repos forget
gortex repos reconcile
gortex repos explain-view
```

涉及 primary checkout、family closure 或 forget 的高影响操作必须先 preview，说明影响范围，再由用户明确确认；持久化执行通常要求 `confirm:true`。不要把 preview 或 explain 结果当作已经写入。

### 23.5 MCP resources、prompts 与订阅

当前 Gortex 支持 MCP resources（当前实现共 18 个）以及以下 prompts：

```text
pre_commit
orientation
safe_to_change
```

支持 `resources/subscribe` 和 `notifications/resources/updated`。资源 URI 必须先通过宿主实际列举或读取；若宿主不支持 resources，改用 `capabilities`、`workspace` 或 `analyze` 的实际 operation，不得假装资源已读取成功。

### 23.6 CLI 补充

除前文命令外，按需使用并先通过 `gortex tools`、`gortex --help` 或文档确认精确参数：

```text
gortex affected
gortex agents render
gortex cloud login
gortex cloud list
gortex cloud logout
gortex db schema
gortex files
gortex instructions list
gortex instructions show
gortex instructions switch
gortex instructions regen
gortex memory ...
gortex provider add
gortex provider list
gortex provider show
gortex provider remove
gortex proxy add
gortex proxy remove
gortex proxy on
gortex proxy off
gortex proxy list
gortex proxy status
gortex trace
gortex tools list
gortex tools search
gortex tools describe
gortex wakeup
gortex upgrade       # update 的别名
gortex uninstall     # clean 的别名
```

隐藏/内部命令 `gortex hook` 和 `gortex __parse-worker` 不应加入普通用户提示词。`gortex call <tool>` 仍需 daemon 与 tracked repository；`--dry` 只用于参数降阶检查，不能宣称已执行操作。

### 23.7 Agent host 与 Codex 集成

当前源码包含多个 Agent host adapter，除 Codex 外还可能包括 Copilot CLI、OpenCode、Gemini、Antigravity、Kimi、Hermes、Pi、Cursor、VS Code、Windsurf、Zed、Kiro 等。应优先遵循具体 host 的官方配置格式，不要把一个 host 的字段迁移到另一个 host。

Codex 集成注意事项：

- 工具命名空间限制使用 `direct_only_tool_namespaces`；不要写未经 schema 支持的 `required = true`。
- 修改 hooks 后，在 Codex 中运行 `/hooks` 检查并信任 hooks。
- `~\\.codex\\AGENTS.override.md`（如存在）可能覆盖 `~\\.codex\\AGENTS.md`，必须一并检查。
- MCP server 的 command/args、PATH、权限和实际 server 启动状态仍需独立验证。

### 23.8 参数校验、Prompt Injection 与 overlay 生命周期

- 设置 `GORTEX_TOOL_ARG_GUARD=reject` 时，未知参数直接拒绝；默认行为可能执行请求并在结果中返回 `_ignored_options`。编写提示词时应要求先查 schema，不能依赖未知字段被静默忽略。
- `GORTEX_MCP_SANITIZE=0` 可关闭 prompt-injection screening；除非用户明确承担风险，不应关闭该保护。
- overlay 默认 idle TTL 约为 30 分钟，可由 `GORTEX_OVERLAY_IDLE_TTL` 调整。长任务应使用实际支持的 keepalive/状态查询，不能假定 overlay 永久存在。

### 23.9 Daemon 与集成故障判定

`gortex mcp` 默认连接或自动启动共享 daemon。embedded fallback 需要用户级配置：

```yaml
mcp:
  allow_embedded: true
```

当 MCP-capable host 已提供 Gortex MCP 但缺少可调用的原生 handle 时，必须报告 `Gortex MCP integration failure` 并停止，不得偷偷切换到 shell、PowerShell 或 `gortex call`。如果只是 daemon 暂时不可达，应先使用 `gortex doctor`、`gortex status` 等诊断实际运行状态；不能把 daemon 故障误判为 host integration failure。

### 23.10 版本与实际能力核验

开始涉及 Gortex 行为的任务时，先核验：

```text
gortex version
workspace(operation="info")
workspace(operation="active_project")
workspace(operation="repos")
workspace(operation="index")
capabilities()
```

版本、preset、工具数量、资源、环境变量和 host 配置都可能随发行版或部署方式变化；本提示词提供的是已确认的已知约束，不替代运行时 capability/schema、tracked 状态和实际命令输出。

