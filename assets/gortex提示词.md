# Gortex 使用规范

> 本规范面向使用 Gortex MCP、Gortex CLI 和 Agent host adapter 的代码 Agent。以 Gortex 0.64.1 运行时源码为权威依据。

## 1. 最高原则和使用边界

Gortex 是对其 track 仓库进行代码定位、源码阅读、关系分析、数据流追踪、影响评估、编辑、重构、验证、审查、项目管理和持久化记忆的权威工具。

- 对 indexed repository，优先使用当前 host 提供的 Gortex 原生 MCP handle。
- 不要用 Read、Grep、Glob、rg、find、PowerShell 或 shell 替代 Gortex 的索引搜索、符号搜索、关系、影响、编辑、重构、guard 或 contract。
- 不要凭记忆发明 tool、operation、参数、字段、preset、命令或关系。
- 不要伪造 Gortex 没有返回的文件、symbol、关系、view、索引状态、测试结果或安全结论。
- 摘要、搜索结果和关系图只能缩小范围，不能替代关键实现体阅读；行为关键代码不要压缩 body。
- 数据库迁移、重试/回退、并发/锁、权限/安全、文件写入、网络调用、事务和状态机必须读取完整实现。
- 所有写操作遵守当前 schema、effect、fixed_arguments、guard、view、overlay 和 partial failure 结果。

协议名与宿主 callable tool 名不同。explore/read/search 是 Facade 名称；实际名称可能是 mcp__gortex__explore、mcp__plugin_gortex_gortex__explore、gortex__explore 等，必须以当前 session inventory 为准。

如果宿主已配置 Gortex MCP，但当前会话没有原生 handle：
1. 报告 Gortex MCP integration failure。
2. 停止当前 indexed-code 操作。
3. 不要手动启动 daemon。
4. 不要自动切到 gortex call、CLI、PowerShell 或 shell。

daemon 不可达时，不要假装图分析完成；用户明确要求诊断时才可使用 gortex version、gortex doctor --json、gortex status、gortex daemon status、gortex daemon logs。不要因为 daemon 故障自动 start、track、reindex、untrack。

目标不在 tracked 范围时，明确报告未被索引；只读任务可在用户允许下使用本地文件检查。已属于 tracked Git family 但 checkout route 尚未就绪时，不要重复 track，应检查 reconciliation/route 并等待或按用户要求诊断。

## 2. 任务开始和 localize

开始 Gortex 任务先确认：

~~~text
workspace(operation="info")
workspace(operation="active_project")
workspace(operation="repos")
workspace(operation="index")
capabilities()
~~~

新任务按目标选择首调用，不要把所有任务都强制用 explore.task：

~~~text
read(operation="file", target={file:"<path>"}, options={new_user_task:true})
explore(operation="localize", task="<完整问题>", options={new_user_task:true})
explore(operation="task", task="<完整任务、错误和约束>", options={new_user_task:true})
recall(operation="distill")
~~~

- 已知文件且只是读取、总结、审查：read.file。
- 需要找文件、symbol、调用点或证据：explore.localize。
- 需要诊断、实现、修改或继续任务：explore.task。
- recall.distill 用于恢复上下文；recall.surface 按需查询既有决策/约束。
- new_user_task=true 只用于新请求第一次调用，不用于分页、重试或后续读取。

可用 explore operation（9个）：`closure`, `context`, `localize`, `outline`, `plan`, `prefetch`, `suggest`, `task`, `wakeup`。

### 2.1 localize 形状与参数限制

当前运行时要求 task 必须在顶层：

~~~text
explore(
  operation="localize",
  task="<完整问题>",
  options={new_user_task:true}
)
~~~

- **严禁**写成 `explore(operation="localize", options={task:"..."})`（运行时会报错 `explore.localize requires task`）。
- **严禁**在 `explore(operation="task")` 中传入 `localize: true`，运行时会硬拒绝报错 `explore.task does not accept localize=true`。

### 2.2 completion 状态流转

localize 可能返回：
`needs_exact_read`, `needs_refinement`, `needs_recovery`, `localized`, `answer_ready`, `refinement_in_flight`, `exact_read_in_flight`, `recovery_in_flight`。

并带有 required_action、instruction、final_response、allowed_tool_calls、allowed_symbols、allowed_operations、exact_symbol 等字段。

- answer_ready：依据 final_response 和证据回答，停止导航。
- localized：定位结束，可以继续诊断、实现、修改、测试或回答。
- needs_exact_read：只读契约指定的精确对象（若候选错误，可用 `read.file` 命名文件跳出）。
- needs_refinement：按 allowed symbols/operations 缩小范围。
- needs_recovery：只执行工具返回的有限 recovery（recovery allowance 上限为 2，不能无限重试）。
- completion 只约束定位后续，不等于代码已修改或测试已运行。

## 3. workspace、view 和 Facade

### 3.1 workspace/view

可用 workspace operation（只读，9个）：
`active_project`, `checkouts`, `graph`, `index`, `info`, `project`, `proxy`, `repos`, `scopes`。

可用 workspace_admin operation（控制写入，14个）：
`blame`, `coverage`, `delete_scope`, `enrich_churn`, `enrich_releases`, `feedback`, `index`, `reindex`, `save_scope`, `set_active_project`, `sql_rebuild`, `temporal_verify`, `track`, `untrack`。

通用 view：
~~~text
view={kind:"auto"}
view={kind:"base", graph_id:"<graph-id>"}
view={kind:"worktree", checkout_id:"<checkout-id>"}
view={kind:"worktree", path:"<absolute-path>"}
view={kind:"git_ref", value:"refs/heads/<branch>"}
view={kind:"commit", value:"<full-lowercase-object-id>"}
~~~

- exact=false 是 fallback，永远只读。
- git_ref、commit 和 fallback view 不可写，**且不可请求 physical_evidence**。
- 只有 exact 且 coordinator-backed 的可写 worktree 才支持 mutation。
- 不要把 fallback、immutable view 或 explain/preview 结果写成已写入。

### 3.2 Facade surface 与参数容器规则

公共 facade-v1 名称（21个）：
`analyze`, `ask`, `capabilities`, `change`, `edit`, `explore`, `overlay`, `pr`, `publish_review`, `read`, `recall`, `refactor`, `relations`, `remember`, `response`, `review`, `search`, `session`, `trace`, `workspace`, `workspace_admin`。

capabilities 查询：
~~~text
capabilities()
capabilities(domain="<tool>")
capabilities(domain="<tool>", operation="<operation>", detail="summary")
capabilities(domain="<tool>", operation="<operation>", detail="schema")
~~~

#### 参数容器与 arguments 陷阱（极其重要）
- **冷门面**（`publish_review`, `pr`, `recall`, `remember`, `workspace`, `workspace_admin`, `overlay`, `response`）及 `session` 在 input_schema 中声明了 `arguments` 容器，其操作参数允许封装在 `arguments: {...}` 中。
- **热门面**（`explore`, `search`, `read`, `relations`, `trace`, `analyze`, `ask`, `change`, `review`, `edit`, `refactor`）未声明 arguments 属性！**严禁**在外层包裹 `arguments: {...}`，否则触发硬错误：`arguments is an unexpected top-level key ... arguments is the JSON-RPC envelope, not a parameter`。参数必须直接放顶层或对应容器（target/options/guard/source/context）。

#### 固定参数（fixed_arguments）
~~~text
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
~~~

GORTEX_TOOL_ARG_GUARD=reject 时未知参数直接拒绝；默认模式下未知参数附加 `_ignored_options`。

## 4. 读取、搜索、关系和分析

### 4.1 search operation（7个）

可用操作：`artifacts`, `ast`, `completion`, `files`, `symbols`, `text`, `winnow`。

~~~text
search(operation="files", query="...")
search(operation="symbols", query="...")
search(operation="text", query="...")
search(operation="ast", query="<pattern>")
search(operation="winnow", query="...")
~~~
- symbols, text, completion 必须显式提供非空 `query`，否则报错 `search.<op> requires query`。
- symbols 固定 assist=off（保证本地确定性搜索）。
- ast 的 query 映射至 pattern；winnow 的 query 映射至 text_match。

### 4.2 read operation（7个）

可用操作：`artifact`, `editing_context`, `file`, `history`, `source`, `summary`, `symbols`。

~~~text
read(operation="file", target={file:"..."}, offset=1, limit=100)
read(operation="editing_context", target={file:"..."})
read(operation="source", target={symbol:"..."})
read(operation="symbols", target={symbols:["id1","id2"]})
read(operation="history", target={symbol:"..."})
read(operation="summary", target={file:"..."})
read(operation="artifact", target={artifact:"..."})
~~~

#### target 选择器单一性规则（高危校验）
- target 必须为对象且**有且仅能有一个选择器**：只能从 `file`, `symbol`, `symbols`, `query`, `artifact`, `repo` 中选 1 个。
- 传空 `{}` 或多键 `{file:"...", symbol:"..."}` 均会报错 `target must contain exactly one selector`。
- 传未知键（如 `{path:"..."}`）会报错 `unknown target selector "path"`。
- 单数 `symbol` 传入数组会报错 `singular selector requires exactly one ID`；批量查询必须用 `symbols`。
- `read.symbols` 的 `target.symbols` 必须是非空数组，传空会报错 `read.symbols requires a non-empty symbol ID array`。
- `read.file` 支持 `offset`/`limit` 或 `start_line`/`end_line`。
- 在 `git_ref` 或 `commit` 虚拟视图下，严禁请求 `physical_evidence: true`（会报错提示非磁盘文件）。

### 4.3 relations & trace operation

relations（11个）：`callers`, `cluster`, `declaration`, `dependencies`, `dependents`, `hierarchy`, `implementations`, `import_path`, `overrides`, `references`, `usages`。
~~~text
relations(operation="callers", target={symbol:"..."})
relations(operation="declaration", target={query:"use_site"})
relations(operation="dependencies", target={symbol:"..."})
relations(operation="implementations", target={symbol:"..."})
relations(operation="usages", target={symbol:"..."})
~~~

trace（7个）：`call_chain`, `cfg`, `flow`, `graph`, `path`, `taint`, `walk`。
~~~text
trace(operation="call_chain", target={symbol:"..."})
trace(operation="flow", target={symbol:"<src>"}, to={symbol:"<sink>"}, options={max_depth:6})
trace(operation="path", target={symbol:"<src>"}, to={symbol:"<sink>"})
trace(operation="taint", target={query:"<src_pat>"}, to={query:"<sink_pat>"})
~~~
- `flow`、`path`、`taint` 同时需要 `target`（源）与 `to`（宿），且均遵守单一选择器规范。

### 4.4 analyze operation

`analyze` 是只读统一分析门面，内置 77 种分析 kind（如 `architecture`, `cycles`, `dead_code`, `health`, `impact`, `sast`, `coverage_gaps`, `race_writes`, `untested`, `clones`, `churn` 等）。
- **变动类阻断**：`blame`, `coverage`, `sql_rebuild`, `temporal_verify` 会持久化变更图数据或调用外部模型，在 `analyze` 下会被直接拦截拒执，必须通过 `workspace_admin(operation="<kind>")` 执行。
- 默认无 kind 调用时等同于 `analyze(operation="help")`。

## 5. 修改、重构和验证

### 5.1 修改前评估

~~~text
change(operation="impact", target={symbol:"<id>"})
change(operation="edit_plan", target={symbols:["<id1>","<id2>"]})
change(operation="guards", target={symbols:["<id1>"]})
change(operation="tests", target={symbols:["<id1>"]})
change(operation="contract", target={symbol:"<id>"})
change(operation="verify", source={changes:[{symbol_id:"<id>", new_signature:"<sig>"}]})
~~~

- `change.contract` 必须提供 `target.symbol`/`target.symbols` 或显式 non-symbol source。
- `remember(operation="risk_ack")` 必须在当前确实存在待确认的变动符号时调用，无变动时会报错 `change_contract ack: no changed symbols to acknowledge`。

change operation（19个）：
`api_impact`, `code_actions`, `compare_branches`, `compare_overlay`, `contract`, `detect`, `diagnostics`, `edit_plan`, `guards`, `impact`, `overlay_branches`, `overlay_state`, `pattern`, `preview`, `ranges`, `receipt`, `simulate`, `tests`, `verify`。

### 5.2 edit 与 refactor（核心防错规范）

edit operation（10个）：`apply_overlay`, `batch`, `docs`, `export_graph`, `file`, `scaffold`, `skill`, `symbol`, `wiki`, `write`。
refactor operation（6个）：`apply_code_action`, `delete`, `fix_all`, `inline`, `move`, `rename`。

#### 【重要铁律】两阶段修改规范与参数冲突排错

1. **`dry_run: true` 与 `physical_evidence: true` 严格互斥！**
   - 源码 `mutation_evidence.go:17` 硬编码拒绝此组合：`physical_evidence requires a real write; dry_run leaves no disk bytes to attest`。
   - **预览阶段**必须使用 `dry_run: true`，严禁传入 `physical_evidence`。
   - **真实写入阶段**必须使用 `dry_run: false`，才可开启 `options={physical_evidence:true}`。
2. **`digest` 强依赖 `physical_evidence`**：
   - 传了 `digest` 必须同时有 `physical_evidence: true`，且当前仅支持 `sha256`。
3. **`guard.expected_occurrences` 仅用于 `edit_file`**：
   - 当实际匹配数量与 `expected_occurrences` 不一致时，源码会拒绝修改并报警。
   - `edit_symbol` 基于 AST 节点 ID，不接受 `expected_occurrences`。
4. **前后内容一致拒绝**：
   - `old_string == new_string` 或 `old_source == new_source` 会被硬拒绝。
5. **语言支持边界**：
   - `refactor.move`（`move_symbol`）和 `refactor.inline`（`inline_symbol`）在当前版本**仅支持 Go 文件**。
6. **Secret 配置保护**：
   - 对包含密码或 token 的敏感配置（如 `.env`）请求 physical_evidence 会被拦截，除非使用 `allow_secrets: true`。

#### 正确调用形状示例

##### 形状 A：修改普通文件（edit.file）

~~~text
// 阶段 1：预览 Diff（dry-run，绝对不要带 physical_evidence）
edit(
  operation="file",
  target={file:"<file>"},
  match="<existing text>",
  replacement="<replacement>",
  dry_run=true,
  guard={expected_occurrences:1},
  options={
    replace_all:false,
    base_sha:"<sha>"
  }
)

// 阶段 2：正式写入（必须 dry_run=false，开启磁盘哈希凭证）
edit(
  operation="file",
  target={file:"<file>"},
  match="<existing text>",
  replacement="<replacement>",
  dry_run=false,
  guard={expected_occurrences:1},
  options={
    replace_all:false,
    base_sha:"<sha>",
    physical_evidence:true,
    mutation_id:"<unique-id>"
  }
)
~~~

##### 形状 B：精确修改符号（edit.symbol）

~~~text
// 阶段 1：预览 Diff
edit(
  operation="symbol",
  target={symbol:"<id>"},
  match="<existing source>",
  replacement="<replacement source>",
  dry_run=true,
  options={base_sha:"<sha>"}
)

// 阶段 2：正式写入
edit(
  operation="symbol",
  target={symbol:"<id>"},
  match="<existing source>",
  replacement="<replacement source>",
  dry_run=false,
  options={
    base_sha:"<sha>",
    physical_evidence:true,
    mutation_id:"<unique-id>"
  }
)
~~~

##### 形状 C：事务型批量修改（edit.batch）

~~~text
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
~~~
- `changes` 数组中每项的必填字段缺失时会立刻报错；`move_file` 和 `delete_file` 强烈建议带 `expected_sha256`。

#### safe-delete 规则
安全删除符号默认 dry_run=true，存在代码引用时拒绝：
~~~text
gortex edit safe-delete <id>
gortex edit safe-delete <id> --apply
gortex edit safe-delete <id> --cascade preview
gortex edit safe-delete <id> --cascade apply
gortex edit safe-delete <id> --propagate
gortex edit safe-delete <id> --force
~~~

### 5.3 修改后验证

~~~text
change(operation="detect")
change(operation="tests", target={symbols:["<affected ids>"]})
change(operation="guards", target={symbols:["<affected ids>"]})
change(operation="contract", target={symbols:["<affected ids>"]})
~~~
- `change.tests` 仅返回建议运行的单测目标列表，**并不代表单测已运行**；必须实际执行构建和测试命令（如 `go test`, `npm test` 等）并汇报真实测试输出。

## 6. overlay、memory、session 和 review

### 6.1 overlay（10个）

操作列表：`delete`, `drop`, `drop_branch`, `fork`, `keepalive`, `merge`, `push`, `register`, `simulate`, `switch`。
~~~text
overlay(operation="register")
overlay(operation="push", arguments={branch:"main"})
overlay(operation="merge", arguments={branch:"main"})
overlay(operation="keepalive")
~~~
- `overlay.merge` 固定 to_disk=false（合并至 overlay 缓冲）；若要合并写盘，使用 `edit(operation="apply_overlay")`（固定 to_disk=true）。

### 6.2 recall（8个）& remember（8个）

recall（只读）：`distill`, `memories`, `notebook_find`, `notebook_list`, `notebook_show`, `notes`, `onboarding`, `surface`。
~~~text
recall(operation="distill")
recall(operation="notes", arguments={file:"..."})
recall(operation="memories", arguments={kind:"invariant"})
recall(operation="surface", arguments={task:"..."})
~~~

remember（本地写入）：`edit_memory`, `memory`, `note`, `notebook`, `notebook_used`, `rename_memory`, `risk_ack`, `suppress_finding`。
~~~text
remember(operation="note", arguments={body:"...", file:"...", tags:["decision"]})
remember(operation="memory", arguments={kind:"invariant", title:"...", body:"..."})
remember(operation="risk_ack")
~~~

### 6.3 session（控制会话）

操作列表：`agents`, `cursor`, `planning_mode`, `proxy_disable`, `proxy_enable`, `subscribe`, `unsubscribe`, `workflow`。
~~~text
session(operation="agents", arguments={action:"list|register|heartbeat|lock|unlock|unregister"})
session(operation="planning_mode", arguments={enabled:true})
session(operation="subscribe", channel="diagnostics", arguments={min_severity:1})
session(operation="unsubscribe", channel="diagnostics")
~~~
- `subscribe` / `unsubscribe` **必须提供 channel 字段**，合法 channel 仅限：
  `daemon_health`, `diagnostics`, `graph_invalidated`, `stale_refs`, `workspace_readiness`。

### 6.4 review（7个）& pr（6个）& response（5个）

~~~text
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
~~~

## 7. CLI 边界和命令索引

原生 MCP 可用时直接调用 MCP。CLI 仅用于用户明确要求、只读诊断、真实单测/构建、未索引目录检查、安装配置，或宿主无原生 Gortex 且用户明确允许。

### 7.1 MCP、daemon 和 server

~~~text
gortex mcp [--tools core|full|readonly|edit|nav] [--tools-mode hide|defer] [--server] [--bind <addr>] [--port <port>]
~~~
- `gortex mcp` 默认自动 proxy 到运行中的 daemon。旧版 `--index` 和 `--watch` 已标记为 deprecated no-op 并被静默忽略。

~~~text
gortex daemon start [--detach] [--tools <preset>] [--tools-mode hide|defer]
gortex daemon stop | restart | reload
gortex daemon status [--watch] [--interval 2s]
gortex daemon logs [--tail 50]
gortex daemon install-service | uninstall-service | service-status

gortex daemon server list
gortex daemon server add <slug> --url <url> [--default] [--read-only] [--auth-token-env <env>]
gortex daemon server remove <slug>
~~~

### 7.2 repos、track、workspace

~~~text
gortex repos [--json]
gortex repos families [--family <family|graph|prefix|path>]
gortex repos set-primary <graph|prefix|path> [--confirm]
gortex repos forget <path|prefix> [--confirm]
gortex repos reconcile [family|prefix|path]
gortex repos explain-view <path>

gortex track <path> [--wait] [--wait-timeout 10m] [--as-worktree] [--name <prefix>]
gortex untrack <path> [--confirm]

gortex workspace list [--json]
gortex workspace set <repo> <workspace> [project] [--global]
gortex workspace set-all <workspace> [--root <path>] [--yes] [--global]

gortex workspace deps list [repo]
gortex workspace deps add <repo> <target-workspace> <module>... --mode read-only
gortex workspace deps mode <repo> <target-workspace> read-only
gortex workspace deps remove <repo> <target-workspace> [module]...
~~~

### 7.3 tools、call、query、node 和反馈

~~~text
gortex tools list [--format json] [--preset compact] [--category <cat>] [--mutating]
gortex tools search <query> [--limit 20] [--format json]
gortex tools describe <tool>
gortex tools receipt [--format json]

gortex call <tool> --json '<object>'
gortex call <tool> --json-file <file>
gortex call <tool> --arg key=value [--dry] [--format json|gcx|toon|text]
~~~

#### 符号直查（node）与反馈（feedback）
~~~text
gortex node <symbol-id> [--callers] [--context 3] [--format json|gcx|toon]
gortex feedback record --task "<task>" --useful "<id1>,<id2>" --not-needed "<id3>" --missing "<id4>"
gortex feedback query [--format json]
~~~

#### 图查询与分析
~~~text
gortex analyze kinds
gortex analyze --kind <kind> [--format json] [--limit 50] [--path-prefix <prefix>]

gortex query symbol <name>
gortex query deps <id> [--depth 3]
gortex query dependents <id> [--depth 3]
gortex query callers <func-id>
gortex query calls <func-id>
gortex query implementations <interface-id>
gortex query usages <id>
gortex query stats

gortex trace <from-id> <to-id> [--depth 24] [--k 1] [--include-references]
gortex flow --from <source-id> --to <sink-id> [--max-depth 8] [--max-paths 10]
gortex taint --source "path:handlers/" --sink "exact:Exec" [--limit 20]

gortex files [--format tree|flat|grouped] [--filter <text>] [--pattern <glob>]
gortex affected [files...] [--stdin] [--quiet] [--json]
git diff --name-only | gortex affected --stdin --quiet
~~~
- `gortex affected` 退出码规范：`exit 0` 表示有测试文件受影响；`exit 3` 表示无受影响测试（CI 可直接跳过）；`exit 1` 表示执行出错。

~~~text
gortex context -t "<task>" [-e "<symbol-or-file>"] [-n 5] [--token-budget 2000] [--format markdown|json]
gortex explore "<task>" [-e "<symbol-or-file>"] [-n 5] [--format json|gcx|toon]
gortex wakeup [--path <repo>] [--max-tokens 500] [--top-communities 4] [--top-hotspots 5] [--top-entry-points 5]
~~~

### 7.4 edit、review、PR 和 memory

~~~text
gortex edit context <file> [--detail brief|full] [--compress]
gortex edit verify --change "<id>=<signature>" [--changes-file <file>]
gortex edit plan --ids "<id1>,<id2>" [--depth 3]
gortex edit preview --workspace-edit-file <file>
gortex edit simulate --steps-file <file> [--keep]
gortex edit batch --edits-file <file> [--dry-run]
gortex edit apply <file> --old "<old>" --new "<new>" [--dry-run] [--expected 1] [--replace-all]
gortex edit symbol <id> --old "<source>" --new "<source>" [--dry-run]
gortex edit rename <id> --to <new-name> [--dry-run]
gortex edit guards --ids "<id1>,<id2>"
gortex edit tests --ids "<id1>,<id2>" [--depth 3]
gortex edit contract --source auto|diff|edit|symbols|ranges [--format json]
gortex edit safe-delete <id> [--apply|--force|--propagate|--cascade preview|--cascade apply]

gortex review [--scope unstaged|staged|all|compare] [--base <ref>] [--diff <file>] [--audience human|agent]
gortex review --post --pr <number> [--confirm-public] [--dry-run]
~~~
- `review --post` 发布到 public / fork PR 时，**必须带 `--confirm-public`**，防止意外将敏感内部审查评论外泄。

~~~text
gortex prs [number] [--triage [--use-llm]] [--conflicts [--worktrees]]
gortex prs bundle <number> [--out <file>]

gortex memory note --body "<note>" [--file <file>] [--tags decision,bug]
gortex memory notes [--file <file>] [--symbol <id>] [--limit 50]
gortex memory distill [--session all]
gortex memory store --kind invariant --title "<title>" --body "<body>"
gortex memory recall --kind constraint [--min-importance 3]
gortex memory surface --task "<task>" [--files "<file>"] [--symbols "<id>"]
~~~

### 7.5 db、enrich、docs、export、wiki

~~~text
gortex db schema --postgres "<dsn>" [--schema public] [--out schema.sql]

gortex enrich churn [path] [--branch <branch|tag|sha>]
gortex enrich blame [path]
gortex enrich coverage <profile> [path]
gortex enrich releases [path] [--branch <branch|tag|sha>]
gortex enrich cochange [path]
gortex enrich all [path] [--coverage <profile>] [--no-churn] [--no-blame] [--no-releases] [--no-cochange]

gortex docs [path] [--out <file>] [--since 168h] [--top 20] [--include recent,ownership,stale,blame] [--run-blame]
gortex export [path] [--format cypher|graphml|mermaid] [--out <file>] [--scope architecture|communities|processes|all]
gortex wiki [path] [--output wiki] [--format markdown|html] [--wikilinks] [--enhance] [--force]
~~~

### 7.6 setup、配置和远程连接

~~~text
gortex agents render [--check] [--target <dir>]
gortex instructions list | show <profile> | switch <profile> | regen

gortex init [path] [--dry-run] [--dry-run-intake] [--agents auto] [--hook-mode deny|enrich] [--hooks-only] [--no-hooks] [--no-skills]
gortex install [--dry-run] [--agents auto] [--hook-mode deny|enrich|consult-unlock|nudge] [--start] [--track] [--track-path <repo>] [--no-hooks] [--no-claude-md] [--print-config <agent>]

gortex githook status [post-commit|post-merge|post-checkout]
gortex githook install <hook> [--regen-churn] [--regen-docs] [--regen-mermaid] [--regen-releases] [--regen-wiki]
gortex githook uninstall <hook>

gortex config exclude list
gortex config exclude add <path-or-pattern> [--global|--repo <name>]
gortex config exclude remove <path-or-pattern> [--global|--repo <name>]

gortex provider list | show <name> | remove <name>
gortex provider add <name> --base-url <url> --model <model> [--api-key-env <env>] [--schema-mode json_schema]

gortex proxy list | status
gortex proxy add <slug> <url> [--default] [--read-only] [--auth-token-env <env>]
gortex proxy on <slug> | off <slug> | remove <slug>

gortex cloud login --workspace <slug> --token <token> | list | logout --workspace <slug>
gortex plugin emit --target <dir> [--variant anthropic] [--version <semver>]
gortex telemetry status | on | off
~~~

### 7.7 诊断、评测与内部命令

~~~text
gortex version [--short]
gortex version bump major|minor|patch [--pre <id>]
gortex doctor [--days 7] [--redact] [--json] [--all]
gortex status

gortex upgrade [version] [--run] [--no-migrate]
gortex update
gortex uninstall [--yes|--global|--purge]
gortex clean

gortex audit [--format svg|json|text] [--out <path>]
gortex clones [--dead-only] [--min-similarity 0.85] [--path-prefix <prefix>] [--limit 20]
gortex bench {recall|tokens|tokens-efficiency|embedders|perf|daemon-latency|swebench|all} [--format markdown|json] [--out-dir <dir>]
gortex eval {baselines|embedders|pack|parity|quality|recall|stdbench|swebench|tokens}
gortex eval-server [--bind <addr>] [--port <port>] [--auth-token <token>]
gortex gain [--bench-result <path>] [--responses-per-day 1000] [--model <name>]
gortex savings [--model <name>] [--json] [--verbose]
gortex completion {bash|fish|powershell|zsh}
~~~

## 8. Windows/Codex 和维护规则

Windows/Codex 诊断：

~~~text
gortex version
gortex doctor --days 7 --json
gortex status
gortex tools list --format json
~~~

确认 gortex.exe PATH/绝对路径、~/.codex/config.toml、~/.codex/AGENTS.md、可能存在的 ~/.codex/AGENTS.override.md、MCP command/args、direct_only_tool_namespaces、hooks 信任和同一组 GORTEX_DAEMON_*/XDG_* 环境变量。修改 hooks 后在 Codex 中运行 `/hooks`。

冲突裁决优先级：
1. 当前工具返回的 error、completion、view、guard、effect。
2. 当前 capabilities 的 schema、request_shape、fixed_arguments、available。
3. workspace/index/repository/checkout 真实状态。
4. 当前 Gortex 运行时源码（0.64.1）、CLI --help、gortex tools describe。
5. 本文。
