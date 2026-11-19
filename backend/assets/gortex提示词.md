
## 【调用外壳、容器归属、通用控制字段与排版语法】
### 1. Antigravity Lazy MCP 调用外壳与三类容器隔离（严禁混写）
在 Antigravity 中，Gortex 的 21 个门面均以 Lazy MCP 工具形式挂载，调用时统一使用：
`call_mcp_tool(ServerName="gortex", ToolName="<门面名>", Arguments={...})`
*(若运行环境已将工具展开为直连工具，则直接调用 `mcp_gortex_<门面名>`，参数对象等价于下述 `Arguments`。)*

务必严格区分 **`call_mcp_tool` 外壳的 `Arguments`** 与 **部分冷门面内部必需的 `"arguments"` 嵌套字段**：

1. **普通平铺门面（10 个，9+1）**：
   * `explore`、`search`、`read`、`edit`、`refactor`、`change`、`relations`、`trace`、`review`（共 9 个）：`Arguments` 顶层直接平铺 `operation`、`target`/`source`/`to`/`context`/`guard`/`changes`/`match`/`replacement`/`content`/`new_name`/`destination`/`dry_run`、`options`、`output`。
   * **`analyze`（1 个）**：`Arguments` 顶层固定平铺 **`kind`**（**严禁写成 `operation`！**），搭配可选 `target`、`options`、`output`。
   * **绝对禁令**：以上 10 个平铺门面**严禁**在 `Arguments` 内再多包一层 `"arguments": {...}` 或 `"params": {...}`！
2. **冷门面（8 个，`arguments` 封装型）**：
   * `workspace`、`workspace_admin`、`overlay`、`remember`、`recall`、`response`、`pr`、`publish_review`：
   * 在 `call_mcp_tool(..., Arguments={...})` 内部，**所有业务参数（及除下列特例外的输出控制参数）必须封装在 `Arguments.arguments: { ... }` 内**，与 `Arguments.operation` 同级；即使无需业务参数也须传 `"arguments": {}`。
   * **4 个混合型特例**：`workspace_admin` 的 `blame`、`coverage`、`sql_rebuild`、`temporal_verify` 除在 `Arguments.arguments` 中传作用域/参数外，其 `compact`/`format`/`max_bytes` **在 `Arguments` 顶层通过 `output: {...}` 传入**（且即使全选默认也必须显式传 `arguments: {}`）。
3. **会话与自省门面（3 个）**：
   * **`session`**：`Arguments` 顶层固定传 `operation`。其中 `subscribe` / `unsubscribe` 的 `channel` 位于 **`Arguments` 顶层**（可选 `arguments: {}`）；其余 6 个操作（`agents` / `cursor` / `planning_mode` / `proxy_enable` / `proxy_disable` / `workflow`）的业务参数位于 **`Arguments.arguments` 内**（顶层另支持 `output`）。
   * **`capabilities`**：`Arguments` 顶层平铺 `domain`（如 `"search"`）、`operation?`、`detail`（**合法值为 `"summary"` 或 `"schema"`**；注意：不要写成 `"operations"`）。
   * **`ask`**：研究代理问答门面；`Arguments` 顶层平铺 `question`（必填）、`options`、`output`。当前未配置研究提供方时 `capabilities` 返回 `operations: []`。

### 2. 跨门面通用视角与实时性控制（Request-Level Knobs）
除 `ask` / `capabilities` 外，其余门面均在 `Arguments` 顶层支持以下通用视角与一致性控制字段（由服务端 `overlay.go` 中间件统一解析，不占用具体操作 `options`）：
1. **`view`（工作树 / Git 版本视角切换）**：不传或 `{"kind":"auto"}` 表示从当前会话工作区自动解析；支持其余 5 种精确模式：
   * `{"kind":"base","graph_id":"<图ID>"}`
   * `{"kind":"worktree","checkout_id":"<checkout_id>"}`（ID 来自 `workspace.checkouts`）
   * `{"kind":"worktree","path":"<工作树根目录绝对路径>"}`
   * `{"kind":"git_ref","value":"refs/heads/main","graph_id?":"..."}`（`value` 须传完整 ref，如 `refs/heads/...`）
   * `{"kind":"commit","value":"<40位小写Git SHA>","graph_id?":"..."}`
2. **新鲜度控制字段**（可选，一般省略）：`require_exact?: bool`、`require_fresh?: bool`、`wait_deadline?: string`（用于要求等待索引追平当前缓冲或强制精确视图）。

### 3. `target` 严格单属性闭集契约（防 Schema 校验闪退）
在 `read`、`edit`、`change`、`refactor` 等带 `target` 的平铺门面中，`tools/list` 静态 Schema 对 `target` 施加了严格约束：**`minProperties: 1, maxProperties: 1, additionalProperties: false`**，且仅允许以下 6 个属性名之一：`file` | `symbol` | `symbols` | `query` | `artifact` | `repo`。
* **严禁**传空对象 `"target": {}`；
* **严禁**把路径键写成 `"target": {"path": "..."}`（必须用 `file`）；
* **严禁**同时传两个键（如 `{"file": "...", "symbol": "..."}`）；
* 对于不接收 `target` 的操作（如 `edit.batch`、`edit.apply_overlay`、`edit.docs`/`export_graph`/`wiki`/`skill`、`change.detect`/`preview`/`simulate`/`diagnostics`/`code_actions`/`receipt`/`api_impact`/`pattern`/`ranges`/`verify`/`overlay_branches`/`overlay_state`、全局 `read.history` 等），**必须完全省略 `target` 字段**。

### 4. 排版符号与响应标记约定
1. **排版符号**：`"|"` 表示候选值多选一；`"~"` 表示推荐闭合整数区间（如 `limit: 25~50`）。**调用时严禁原样照抄 `"|"` 或 `"~"`**，必须选定单个合法值或合理整数。
2. **精确输出标记（严禁向无对应能力的接口硬塞 `format` 或 `max_bytes`）**：
   * **`[JSON+MB]`**：在顶层传 `"output": { "format": "json", "max_bytes": 8400 }`（`search.winnow` 为 `8250`）；部分操作还允许在 `output` 内传 `compact` / `fields` / `limit` / `offset` / `summary_only`（以各节表格为准）。
   * **`[JSON]`**：在顶层**仅**传 `"output": { "format": "json" }`（该操作静态 `output` Schema 不接受 `max_bytes`，强传必报 `unknown field "max_bytes"`！）。
   * **`[冷JSON+MB]`**：在冷门面 `Arguments.arguments` 内传 `"format": "json", "max_bytes": 8400`。
   * **`[冷JSON]`**：在冷门面 `Arguments.arguments` 内**仅**传 `"format": "json"`（不接受 `max_bytes`）。
   * **未标注 `format` 的写操作 / 状态控制 / 特定精读接口**：如 `read.file` 仅接受 `output: { max_bytes: 8400 }`（无 `format`）；多数写与状态操作完全不接受 `format`，严禁硬塞。

---

# 【L0 级：全局统领宪法与多项目防错底线】（最高强制位阶 · 每次必读）

### 铁律 1：外部文档只读豁免、Gortex 闭环与原生跳出边界
1. 位于代码仓库外部系统目录的规范文件（如规则中指定的外部提示词或跨盘文档）属于初始环境法定豁免文件，**必须且仅能使用宿主原生只读工具（`read_file` 等）按规范分片读取**，严禁调用 Gortex MCP 读取外部绝对路径。
2. 预读门禁完成后，受 Gortex 纳管工程内的所有源码、测试代码（含源码内的 `#[cfg(test)]` 与独立测试文件）、配置文件与维护脚本的探测、检索、阅读、新建、修改、验证与审查，**100% 走 Gortex MCP 闭环**。
3. 仅两类情况允许宿主原生工具：
   * ① 用户输入中明确包含“使用原生工具”指令时单次跳出，完成后立即重返 Gortex 闭环；
   * ② **Git / GitHub 自身操作及实际运行 / 调试测试进程**（`git`、`cargo test`、`go test`、`pytest`、`npm test`、调试器下断点/展看故障行）可直接使用原生命令，但**严禁借运行测试或调试之名，用原生工具直接检索、阅读或修改受纳管工程/测试文件**。

### 铁律 2：双通道路径零拼接与 `repo`/`project`/`workspace`/`scope` 严格白名单（防 `search`/`read` 闪退核心）
#### A. 双通道路径规则
* **通道 A（全局定位寻址器：`target.file` / `target.symbol` / `source.file` / `changes[].path` / `arguments.path`）**：
  用于 `read.*`、`edit.*`、`refactor.*`、`relations.*`、`trace.*`、`change.*`、`analyze.co_change`/`why`/`def_use`。所有路径与符号 ID 必须原样复制自 `workspace` / `search` / `explore` 返回值；**多仓库及 `unbound_global` 环境下，必须保留前导 `repo/` 前缀**（例如 `"edit-rust/backend/src/main.rs::main"`）。
* **通道 B（仓内子路径过滤器：`options.path` / `options.path_prefix`）**：
  `search.text`/`symbols`/`files` 用 `options.path`，`search.winnow`/`ast` 及 `analyze` 多数分析项用 `options.path_prefix`。**绝对严禁在 `path` 或 `path_prefix` 中拼接 `repo` 前缀**（底层 `pathMatchesAnyPrefix` 按仓内相对路径比对，拼了 `repo/` 必搜空！）；仓库限定必须单独通过允许字段 `options.repo` 传入。

#### B. `repo` / `project` / `workspace` / `scope` 允许与严格禁传矩阵
> **致命高频坑预警**：在 `unbound_global` 会话中，`workspace.repos` 的引导语提示 `"pass repo on tool calls"`。然而 Gortex 对**未在其 legacy schema 声明这些过滤器的操作**，会在 `validateFacadeRequestFields` 直接拦截并抛出 **`invalid_argument: unknown field "repo"`**，导致 UI 出现“调用闪一下即报错消失”！严格遵守下表：

| 门面 | **允许**传 `repo`/`project`/`workspace`/`scope` 的操作及确切可用键 | **绝对禁止**传 `repo`/`project`/`workspace`/`scope` 的操作 |
| :--- | :--- | :--- |
| **`read`** | **仅 `read.summary`** 允许 `{repo, project, scope, workspace, ref}`（多仓且非 `active_project` 时 **`read.summary` 必传 `options.repo`**） | `read.file`、`read.source`、`read.symbols`、`read.editing_context`、`read.history`、`read.artifact` **全部严禁传 `repo` 等 4 键**（直接用带 `repo/` 前缀的 `target` 寻址） |
| **`search`** | `text`、`symbols`、`files` 允许 `{repo, project, scope, workspace}`；`winnow`、`ast` **仅允许 `{repo, project, ref}`**（**禁 `workspace`/`scope`!**） | `search.artifacts`、`search.completion` **严禁传 `repo` 等 4 键** |
| **`explore`** | `context` 允许 `{project, scope}`（**无 `repo`/`workspace`**）；`closure` 允许 `{project, ref, scope, workspace}`（**无 `repo`**） | `task`、`localize`、`outline`、`wakeup`、`plan`、`prefetch`、`suggest` **均严禁在 `options` 传 `repo`/`project`/`scope`/`workspace`** |
| **`edit`** | `docs` 允许 `{workspace}`；`export_graph` 允许 `{repo, scope}`；`wiki` 允许 `{repo, project, workspace}` | `batch`、`symbol`、`file`、`write`、`apply_overlay`、`scaffold`、`skill` **全部严禁传 `repo` 等 4 键** |
| **`refactor`** | **仅 `fix_all`、`apply_code_action`** 允许 `options.repo` | `rename`、`delete`、`move`、`inline` **全部严禁传 `repo` 等 4 键** |
| **`change`** | 仅 `detect`(`repo`)、`diagnostics`(`repo, project`)、`code_actions`(`repo, project`)、`api_impact`(`repo, project, ref`)、`contract`(`repo`) | **其余全部 14 个操作**（`tests`/`guards`/`impact`/`edit_plan`/`pattern`/`ranges`/`verify`/`receipt`/`preview`/`simulate`/`overlay_*`/`compare_*`）**均严禁传 `repo` 等 4 键** |
| **`relations`** | `callers`、`dependencies`、`usages`、`implementations`、`overrides`、`hierarchy`、`dependents`、`cluster` 均允许 `{repo, project, scope, workspace, ref}` | `references`、`declaration`、`import_path` **严禁传 `repo` 等 4 键** |
| **`trace`** | **仅 `call_chain`、`walk`** 允许 `{repo, project, scope, workspace, ref}` | `path`、`flow`、`taint`、`cfg`、`graph` **均严禁传 `repo` 等 4 键** |
| **`analyze`** | 多数全局分析允许 `{repo, project, scope, workspace}`；注意 `cycles` 的 `scope` 是**目录前缀**非 saved scope；`cross_repo` 的 `repo` 是**跨仓边界过滤** | `agent_config`、`inspection_catalog`、`replay`、`lint`、`citation`、`why`（`why` 的 `scope` 为 `"workspace"\|"global"` 记忆域）、`help` **均严禁传仓库过滤键** |
| **`workspace_admin`** | **仅 `blame`、`coverage`、`sql_rebuild`、`temporal_verify`** 允许 `{repo, project, scope, workspace}` | `feedback`、`index`、`reindex`、`enrich_*`、`track`/`untrack`、`save_scope`/`delete_scope`、`set_active_project` **均严禁乱传 `repo`**（`save_scope` 专用逗号串 `repos`） |
| **其余门面** | `review.*`、`pr.*`、`publish_review.post` 全量允许 `repo`；`remember.risk_ack` 允许 `repo` | `overlay.*`、`response.*`、`recall.*`、`remember.*`（除 `risk_ack`；`recall`/`remember` 的 `scope` 为记忆作用域 `"workspace"\|"global"`）均不传仓库 `repo` |

### 铁律 3：开局四步前置探测状态机 (Probe-Before-Analyze)
接入新任务、跨仓排查或首次分析未知工程前，**必须按序完成探测后才可分析或修改代码**（若同会话刚完成且工作区未变，可复用已缓存结论）：
1. **边界探测**：调 `workspace`（`operation="repos"`, `arguments={"format":"json","max_bytes":8400}`）或 `workspace`（`operation="info"`, `arguments={"format":"json","max_bytes":8400}`），获取已 track 仓库清单、本地根路径 `members[].path` 与绑定模式（`workspace` vs `unbound_global`）。
2. **归属探测**：调 `workspace`（`operation="active_project"`, `arguments={"format":"json","max_bytes":8400}`）确认当前主工程；若处于 `unbound_global` 或与当前工作区路径不符，将当前对话工作目录匹配 `members[].path` 锁定目标 `repo` 前缀与 `project` 名。
3. **健康诊断**：调 `workspace`（`operation="index"`, `arguments={"format":"json","max_bytes":8400}`）核验索引就绪度、解析错误与陈旧状态（或结合只读资源 `gortex://index-health`）。
4. **准入与止损**：严禁未经探测盲搜，严禁臆测“未索引”退回宿主工具；若 `repos`/`members` 均为空或目标目录确未纳管，仅作一次确认并按需通过 `workspace_admin.track` 纳管或如实报告，严禁死循环重试。

### 铁律 4：多项目双穿透与主工程切域法则
1. **只读轻量穿透（免切项目）**：跨仓查找符号可用 `workspace(operation="project", arguments={"project":"<目标工程>","query":"...","limit":20})`；跨仓精读直接将目标文件或符号按 `"<repo>/<相对路径>"` / `"<repo>/<路径>::<符号>"` 传入 `read.file` / `read.symbols` / `read.source`。
2. **深度操作切域**：对非默认 `active_project` 的外部仓库执行依赖全局工程上下文的重构、测试图分析或批量联动修改前，先调 `workspace_admin(operation="set_active_project", arguments={"project":"<目标工程>"})`（注意：值为 `project` 名，不是文件路径，且无 `format`）。若仅在多仓中并发查单仓，对支持 `options.repo` 的操作显式指定 `repo` 即可，严禁向铁律 2 禁传列表中的操作注入 `repo`/`project`。

### 铁律 5：强类型签名、`output` 精确匹配与读取切片硬锁
1. **显式操作名**：顶层必传 `"operation"`（`analyze` 顶层必传 `"kind"`），严禁发送无头请求。
2. **输出容器精确门禁（防 `max_bytes` / `format` 报错）**：
   * 仅在操作标注 **`[JSON+MB]`** 时才传 `"output": {"format":"json","max_bytes":8400}`（`search.winnow` 为 `8250`）；
   * 仅标注 **`[JSON]`** 的操作，**严禁传 `max_bytes`**，只传 `"output": {"format":"json"}`（`analyze.coupling`/`churn` 可附加 `limit`，仍禁 `max_bytes`）；
   * **`read.file` 仅支持 `"output": {"max_bytes":8400}`，严禁传 `format`**；
   * 冷门面在 `arguments` 内按 `[冷JSON+MB]` 或 `[冷JSON]` 传对应字段（4 个 `workspace_admin` 富化读取在顶层 `output`）；
   * 写操作、状态控制、`explore.task`/`localize`/`plan`、`analyze.health`/`lint`/`recent_changes` 等未标 `format` 的接口，**严禁硬塞 `format` 或 `max_bytes`**。
3. **绝对禁把 `max_bytes` 放进 `options`**：`max_bytes` 永远属于 `output`（平铺门面）或 `arguments`（冷门面），放进 `options` 必报未知字段错误。
4. **读取切片与体积硬锁**：
   * `read.file` **严禁无范围盲读整文件**！必传 `options: {"offset": 起始1基行号, "limit": <=150}`（常规推荐 40~100；即使已知小文件也显式传 `offset:1, limit:150`）+ `output: {"max_bytes":8400}`；
   * **关键补遗**：Gortex 响应封包器（`respondJSONOrTOON`）**不会**按 `max_bytes` 自动截断单个长字符串字段（如 `read.file` 的 `content`），因此读取密集代码、长行或含大量多字节中文的文件时，除 `limit <= 150` 外，必要时主动配合 **`options.max_chars`**（按 UTF-8 字节上限安全切分）及 `context.compress_bodies: true` / `context.max_lines`，防止单次返回触发客户端溢出落盘或大载荷拥塞。
   * `explore` 探索优先控制 `options.token_budget: 1600~4000`。

### 铁律 6：分级批量原子交付、大 `edit` 防闪断取消与写故障三步退避
> **专项根治 `edit` 大输出取消 / 闪退现象**：Antigravity 流式生成超长 JSON 参数时易发生转义失配/流式超时中断，且 Gortex 服务端默认单工具超时为 **60 秒（`DefaultToolCallTimeout`，含最长 30 秒的重组合图等待）**，叠加批量任意一处失配导致整批事务全回滚（`aborted`），造成 `edit` 大输出频繁闪退取消。

1. **合理的编辑路由与单批体量上线**：
   * **单文件单处确定性替换** → 直接用 `edit.file`；
   * **新建文件或必须整文件覆写** → **必须用 `edit.write`**（重要：`edit.batch` 内的 `edit_file` 对不存在的文件会直接报 `"file does not exist"` 并触发整批回滚，绝不能用来建新文件！）；
   * **修改函数/类内部代码（支持局部片段！）** → 优先用 `edit.symbol`（只需传入该符号内唯一的前后数行作为 `match` 和 `replacement`，**无需把 200 行函数体全量塞进 `match`/`replacement`**，可大幅压缩请求与响应输出体积！）；
   * **多处关联重构 / 跨文件签名+调用点同步** → 合并入 **`edit.batch`** 原子提交。若单一任务涉及跨十余个文件、改动体量巨大（单次请求文本预计超过 ~10 KB 或 8~12 个大块 `changes`），禁止硬塞进一笔畸大无边 `edit.batch` 赌超时/转义，而应**按内聚模块拆分为少数几次边界清晰、可独立通过编译校验的 `edit.batch`**（禁止退化成逐行切香肠）。
2. **`edit.batch` 内部四类 `op` 严格字段隔离**：`facade_tools.go` 对 `changes[]` 子项未封死静态键名，混写键名会在进入事务解析时被立即拒收！每条 item 仅允许本 `op` 专属键（见第三节），**绝对禁止在 `changes[]` 内部写入 `target`、`match` 或 `replacement`**。
3. **写操作幂等性与故障三步退避**：
   * 关键批改建议带 `edit.batch` `options.transaction_id`（单文件/单符号带 `options.mutation_id`），重发同一 Key+相同内容可安全幂等回放而不会写两遍；高不确定性改动可用 `options.base_sha` 做防漂移校验，或先 `dry_run: true`。
   * 一旦出现 `ambiguous_match`、行号/上下文漂移、或 **请求闪断/超时不确定是否已落盘**：**绝对禁止直接原样重放！** 严格按三步退避：
     ① 若传过 `transaction_id`，可先 `edit.batch(changes:[], options:{transaction_id:"...", status_only:true})`；若传过 `mutation_id` 或需查近期回执，调 `change.receipt(options:{mutation_id|receipt})`；同时调 **`change.detect`** 核实磁盘与未提交差异究竟是否已写入；
     ② 仅对尚未落盘或失配的目标区域，调用 **`read.file(offset, limit <= 150)`** 或 `read.symbols` 重新对齐真实源码上下文基线；
     ③ 更新精确 `match`/`old_string`（或更换新 `mutation_id`）后重新提交。

### 铁律 7：大输出防溢出隔离守卫 (Anti-Spill)
1. 若某次 Gortex 调用仍触发宿主客户端落盘截断（提示结果保存至某个 `output.txt`）或命中 Gortex 缓冲句柄，**绝对严禁使用宿主任何原生工具（`read_file` / `cat` / `Get-Content`）直接读取该内部 `output.txt` 文件或绕过配额**！
2. 正确做法：从源头通过收紧 `output.max_bytes: 8400`、`read.file` 降低 `limit`（40~80）与设 `options.max_chars`、启用 `context.compress_bodies: true` 与 `output.compact: true` 重取；若已获得 Gortex `response_id` 缓冲句柄，仅限调用 **`response(operation="grep"|"slice"|"peek"|"stats", arguments={...})`** 提炼摘要。

### 铁律 8：直调优先、本地 Schema 禁令与 `capabilities` 首错即查防连闪红线
1. **禁用本地 Schema 文件**：严禁使用宿主原生工具读取任何 `mcp/gortex/*.json`。
2. **已知接口严禁事前空转 `capabilities`**：凡本文档已给出明确参数契约的操作，必须直接构造参数调用，**禁止**每次常规读写或编辑前例行调用 `capabilities` 预查。
3. **防连闪“首错即查”兜底机制（三类触发边界）**：
   * **场景 A（报错即刻自愈，禁二次盲猜！）**：按本文档直调后，若服务端返回任何参数校验错误（`invalid_argument`、`unknown field`、缺失必填字段、容器嵌套报错等），**严禁凭猜测修改参数直接连续重试**（连续猜错即表现为 UI 频繁报错闪烁）；必须**立即调用一次 `capabilities`**：`{"domain":"<门面>","operation":"<操作>","detail":"schema"}`，严格对照返回的 `input_schema` 与 `request_shape` 一次修准；
   * **场景 B（未收录盲区）**：需使用本文档未覆盖详细参数的新能力或核对子操作清单时，先调 `{"domain":"<门面>","detail":"summary"}`（或带 `operation` 的 `"schema"`）；
   * **场景 C（同会话强去重）**：同一组 `domain + operation` 在当前会话内通过 `capabilities` 查询过一次 `detail:"schema"` 后，**本会话内严禁再次重复查询**。

### 铁律 9：默认按文档执行与冲突裁决五大位阶
**未发生真实报错或行为冲突时，默认直接以本文档契约为准执行；禁止借高位阶之名在日常调用前常规跑 `capabilities`。** 仅当运行时实际响应与本文档出现分歧或报错时，严格按由高到低五级裁决：
1. **工具真实运行时响应**（报错细节 / `guard` 提示 / `completion` / `not_modified`）；
2. **报错或盲区触发的 `capabilities(detail:"schema")` 即时 Schema**；
3. **磁盘物理文件与 Git 树真实状态**（如 `change.detect` / `read.file(physical_evidence:true)`）；
4. **Gortex 服务端源码实现**（若对应仓库已纳管且确需排查底层逻辑）；
5. **本文档静默规则**。

---

# 【L1 级：日常开发与多项目协同黄金闭环】（高频 · 协-查-读-写-验五大核心中枢）

## 零、 工作区与多项目协同中枢 (`workspace` [9] / `workspace_admin` [14])
> **容器铁律**：此 2 个门面均为**冷门面**，`Arguments` 内必须传 `"operation"` 与 `"arguments": {...}`。仅 `workspace_admin` 后 4 个富化分析（`blame` / `coverage` / `sql_rebuild` / `temporal_verify`）可在 `Arguments` 顶层另附 `output`。

### 1. `workspace` 只读与导航查询（9 个全收录）
| 触发场景 | `operation` | `arguments: { ... }` 参数与精确格式契约 |
| :--- | :--- | :--- |
| 列已纳管仓库、模式(`workspace`\|`unbound_global`)及本地路径映射 | `"repos"` \| `"info"` | `{"format":"json","max_bytes":8400}` **`[冷JSON+MB]`**（严禁传 `repo`） |
| 查当前会话主工程及包含成员列表 | `"active_project"` | `{"format":"json","max_bytes":8400}` **`[冷JSON+MB]`** |
| 查索引健康、解析失败数、陈旧文件与语言统计 / 图谱节点边汇总 | `"index"` \| `"graph"` | `index`: `{"compact?":true,"format":"json","max_bytes":8400}`；`graph`: `{"format":"json","max_bytes":8400}` **`[冷JSON+MB]`** |
| 跨项目免切换检索外部已登记工程符号 | `"project"` | **必填 `project`**（工程名，非路径）+ **`query`**，可选 `limit?: 20`（**无 `format`**） |
| 列出可见工作树家族(`family`)、`checkout_id` 与隔离视角路径 | `"checkouts"` | `{"family?":"...","fields?":"...","max_tokens?":...,"format":"json","max_bytes":8400}` **`[冷JSON+MB]`** |
| 列出已保存的多仓/路径复用作用域 | `"scopes"` | `{}`（**无 `format`**） |
| 查询远程 Gortex 代理连接状态 | `"proxy"` | `{}`（**无 `format`**） |

### 2. `workspace_admin` 工作区治理与配置（日常治理 6 项；底层索引与富化 8 项见第十节，共 14 项）
| 触发场景 | `operation` | `arguments: { ... }` 必填项与注意事项（除标注外均无 `format`） |
| :--- | :--- | :--- |
| 热切换当前会话主工程上下文 | `"set_active_project"` | **必填 `project`**（工程名如 `"edit-rust"`，非文件路径） |
| 纳管注册新仓库 / 工作树 | `"track"` | **必填 `path`**（绝对路径），可选 `name?`, `as_worktree?: bool`, `force?: bool` |
| 从索引取消跟踪某仓库 | `"untrack"` | **必填 `path`**；**未传 `confirm: true` 时仅作影响预览而不执行删除！** |
| 保存 / 删除可复用的多仓+子路径组合作用域 | `"save_scope"` \| `"delete_scope"` | `save_scope`: **必填 `name`, `repos`（注意：必须是逗号分隔字符串 `"repoA,repoB"`，严禁传 JSON 数组！）**，可选 `paths?`（同样为逗号分隔字符串）, `description?`；`delete_scope`: **必填 `name`** |
| 记录或查询智能上下文召回质量反馈 | `"feedback"` | **必填 `action`** (`"record"` \| `"query"`)；`record` 配 `task?`, `useful?`, `not_needed?`, `missing?`（三者皆为逗号分隔符号 ID 串）, `tool_source?` (`"smart_context"` \| `"prefetch_context"`); `query` 配 `top_n?`, `tool_source?` (`"all"`\|...), `compact?: bool`, `format:"json"` **`[冷JSON]`**（禁传 `repo`） |

```json
call_mcp_tool(ServerName="gortex", ToolName="workspace", Arguments={"operation":"repos","arguments":{"format":"json","max_bytes":8400}})
call_mcp_tool(ServerName="gortex", ToolName="workspace", Arguments={"operation":"project","arguments":{"project":"edit-rust","query":"GortexBridge","limit":20}})
call_mcp_tool(ServerName="gortex", ToolName="workspace_admin", Arguments={"operation":"set_active_project","arguments":{"project":"edit-rust"}})
call_mcp_tool(ServerName="gortex", ToolName="workspace_admin", Arguments={"operation":"save_scope","arguments":{"name":"core-svc","repos":"repo-a,repo-b","paths":"src/auth,src/api"}})
```

---

## 一、 检索与探索中枢 (`search` [7] / `explore` [9])
> **容器铁律**：均为平铺门面，`Arguments` 顶层传 `operation`、`options`、`output`，禁止外包 `arguments`。

### 核心门禁（检索前必守）
1. **双探针原则**：排查未知业务逻辑，首动并发 `search.text`（精确字面量）+ `search.winnow`（设 `min_fan_in: 2` 定位主干枢纽），或先行使用 `explore.task` / `explore.context` 获取聚焦上下文。
2. **Vue / React / Svelte SFC 法则**：`.vue`、`.tsx`、`.svelte` 的模板事件绑定、插槽与内部局部变量往往不在全局导出符号索引中，**必须首选 `search.text`** 定位事件与行号，严禁先跑 `search.symbols` 搜空再退回原工具。
3. **拓扑升格死锁**：若 `search.winnow` 首轮未命中，**严禁直接 `read.file` 大范围裸读或 `edit.batch` 盲改**；必须先从 `search.text` 命中行附近提取外层函数/类符号名，二轮调用 `search.winnow(query="真实符号", min_fan_in: 0)` 获取完整符号 ID 与拓扑后，再进入精准阅读与编辑。
4. **禁 `search.files` 投机令（含例外）**：业务逻辑排查严禁首动 `search.files` 猜文件名；**仅当已知目标非源码文件（配置/脚本/文档）、核验文件是否存在、或盘点某目录下文件清单时允许使用 `search.files`**（`query` 禁止 `*`/`?` 通配符，`options.glob` 才支持通配符，且 **`query` 与 `options.glob` 至少传一个**）。
5. **`explore.localize` 会话状态机锁（极高危陷阱警告！）**：
   * Gortex 源码（`localization_terminal.go`）在调用 `explore(operation="localize")` 后会激活 `localizationTerminalState`（定位终结护栏），**强行拦截同一任务后续一切 `explore` / `search` / `read`（非定位文件外） / `relations` / `trace` / `analyze` 调用**，直到读取了返回证据文件或重新声明 `options.new_user_task: true`！
   * **实战硬规**：在任何需要持续诊断、串联链路、读多文件或修改代码的开发任务中，**优先用 `explore.task` 或 `explore.context` 或 `search.*`，严禁随手调用 `explore.localize`**；仅当任务明确是“一句话精准定位并停止解答”时才可用 `localize`。
6. **`explore.*` 中 `path` 被隐式改写为 `repo` 陷阱与 `task` 容器差异**：
   * 经 Gortex 源码（`facade_tools.go:2125-2133`）验证：对 `explore` 门面，若在顶层 `path` 或 `options.path` 传入非空值，`normalizeFacadeAliases` 会执行 `out["repo"] = path; delete(out, "path")`！也就是说 **`options.path` 在当前版本底层实际上当作仓库选择器 `repo` 处理，传入 `"src/auth"` 这类仓内相对子目录极易触发仓库作用域报错或搜空**！在 `explore.context` 中如需按仓内子路径过滤，请直接在 **`task` 文本内附带内联 `path:src/auth`**（官方 Schema 明确支持）或使用预先保存的 **`options.scope`**！
   * 另请注意 `task` 字段位置差异：**`explore.task` 与 `explore.context` 要求 `task` 在顶层（`options` 内禁止写 `task`）**；而 **`explore.localize` 与 `explore.plan` 要求 `task` 在 `options.task` 内（顶层可选冗余）**！
7. **`new_user_task` 守则**：仅在由“一条全新的用户提问”触发的**首次** `explore`（`task`/`localize`）或 `read.file` 调用上设置 `options.new_user_task: true`；同一任务的重试、补充查证或连续工具链中**绝对严禁再传 `new_user_task: true`**。

### 1. `search`（7 个全收录）
| 触发场景 | `operation` | 必填项、`options` 合法字段、`repo` 边界与精确 `output` |
| :--- | :--- | :--- |
| 报错日志、错误码、SQL 文本、Vue/React 模板事件 | `"text"` | **必填顶层 `query`**；`options?: { repo?, path? (无repo/前缀), project?, scope?, workspace?, regexp?: false, limit?: 35~80 }`；**`[JSON]`（无 `max_bytes`！）** |
| 约束链检索与主干入口探测 / 拓扑确认 | `"winnow"` | 可选顶层 `query`（**警告：`query` 与过滤条件不能全空；若未传 `query` 切勿只传 `min_fan_in:0`，至少具备 1 个有效非零筛选或 `query`**）；`options: { repo?, project?, ref?, path_prefix? (无repo/前缀), language?, kind?, community?, min_fan_in?: 0\|2, min_fan_out?, min_churn?, is_test?, test_role? ("test,benchmark,fuzz,example"), limit?: 25~50, paginate?, cursor? }`（**禁传 `workspace`/`scope`!**）；`output: {"format":"json","max_bytes":8250,"compact?":bool,"fields?":"..."}` |
| 按名称/概念/特征查符号与文档节点 | `"symbols"` | **必填顶层 `query`**；`options?: { repo?, project?, scope?, workspace?, ref?, path? (无repo/前缀), kind?, flavor? ("struct\|class\|component\|..."), corpus? ("code\|docs\|content\|all"), expand? ("both\|equivalence\|llm\|off"), query_class? ("auto\|symbol\|concept\|path\|signature"), vocab_anchored?: bool, max_per_file?, limit?: 28~70, paginate?, cursor?, debug?: bool }`（底层自动固定 `assist:"off"`）；**`[JSON+MB]`**（支持 `compact?`, `fields?`） |
| 语法树结构模式或内置安全/卫生检测器扫描 | `"ast"` | `query`（结构模式代码模板）与 `options.detector` **严格互斥**！① 用原生模式：**必传顶层 `query` + `options.language`**（`"go\|python\|javascript\|typescript\|rust\|..."`）；② 用内置规则：`options.detector`（传 **`"help"`** 可列出全部内置规则目录，**不要传 `""`**）；通用参数 `options: { repo?, project?, ref?, path_prefix? (无repo/前缀), exclude_tests?, min_fan_in_of_enclosing_func?, limit?: 28~50 }`（**禁传 `workspace`/`scope`!**）；**`[JSON+MB]`** |
| 验证路径存在或按名称/通配符收集文件列表 | `"files"` | **`query` 与 `options.glob` 至少必填其一**（`query` 为精确/模糊子串，**严禁通配符 `*`/`?`**；通配模式放 `options.glob` 如 `"*.rs"`）；`options?: { repo?, project?, scope?, workspace?, path? (无repo/前缀), glob?, fuzzy?: false, limit?: 70~220 }`；**`[JSON]`（无 `max_bytes`！）** |
| 检索 `.gortex.yaml::artifacts` 注册的非代码知识节点 | `"artifacts"` | 可选顶层 `query`；`options?: { kind? ("schema\|api\|infra\|doc"), limit?: 20 }`（**禁 `repo`/`project`/`scope`/`workspace`**）；**`[JSON]`** |
| 从种子符号执行 1 跳图扩展候选补全 | `"completion"` | **必填顶层 `query`**；`options?: { edge_kinds? ("calls,references"), seed_limit?, max_seed_expansion?, limit?: 20 }`（**禁 `repo`/`project`/`scope`/`workspace`**）；**`[JSON]`** |

### 2. `explore`（9 个全收录）
| 触发场景 | `operation` | 参数结构与精确 `output` |
| :--- | :--- | :--- |
| 非终止邻域搜集（持续排查/改代码首选，不锁后续工具） | `"task"` | **必填顶层 `task`**；`options?: { max_symbols?: 10, token_budget?: 1600, new_user_task?: bool }`（**勿传仓内子目录进 `path`/`options.path`；无 `output`**） |
| 定位即止的终端证据提取（会锁定后续调用，慎用） | `"localize"` | **必填 `options: { task, max_symbols?: 10, token_budget?: 1600, new_user_task?: bool }`**（固定注入 `localize:true`；**无 `output`**） |
| 单次组装最小聚焦上下文（支持分级、预算估算与增量 Delta） | `"context"` | **必填顶层 `task`**（需限子目录时在 `task` 内写 `path:src/foo`）；`options?: { project?, scope?, entry_point?, max_symbols?: 5, fidelity?: "flat"\|"graded", token_budget?: 8000 (仅 `graded` 生效), delta_from?: "<etag>", if_none_match?: "<etag>", estimate?: bool, include_call_paths?: bool, include_flows?: bool, include_confidence?: bool, new_user_task?: bool }`；**`[JSON+MB]`** |
| 从已知多个种子文件或符号构建多跳依赖闭包包 | `"closure"` | `options: { files?: "a.rs,b.rs", symbols?: "id1,id2", edge_kinds?, max_depth?: 6, max_nodes?: 400, rank?: "distance"\|"proximity", token_budget?: 4000, project?, ref?, scope?, workspace? }`（注：不支持 `repo`）；**`[JSON]`（无 `max_bytes`）** |
| 单次全库叙事级概览（语言/社区/热点/高频导入/入口） | `"outline"` | `options?: {}`；**`[JSON+MB]`** |
| 获取 ~500 Token 极精简代码库速览摘要 | `"wakeup"` | `options?: { max_tokens?: 500, top_communities?: 4, top_entry_points?: 5, top_hotspots?: 5 }`；`output?: { format?: "markdown" \| "json" }`（无 `max_bytes`） |
| 开局最佳工具路径路由推荐 | `"plan"` | **必填 `options: { task, max_calls?: 4 }`**（无 `output`） |
| 基于最近操作与任务预测下一步上下文 | `"prefetch"` | 可选顶层 `task`；`options?: { recent_symbols? (逗号串), include_source?: bool, limit?: 10, max_tokens?, paginate?: bool, cursor? }`；**`[JSON+MB]`**（支持 `compact?`, `fields?`） |
| 陌生仓库冷启动探索问题建议（5~10 条） | `"suggest"` | `options?: { limit?: 8 }`；**`[JSON]`（无 `max_bytes`）** |

```json
call_mcp_tool(ServerName="gortex", ToolName="search", Arguments={"operation":"text","query":"handle_save_user","options":{"repo":"edit-rust","path":"frontend/src/views","limit":50},"output":{"format":"json"}})
call_mcp_tool(ServerName="gortex", ToolName="search", Arguments={"operation":"winnow","query":"forward_handler","options":{"repo":"edit-rust","path_prefix":"src/api","min_fan_in":0,"limit":30},"output":{"format":"json","max_bytes":8250}})
call_mcp_tool(ServerName="gortex", ToolName="search", Arguments={"operation":"ast","options":{"repo":"edit-rust","detector":"help"},"output":{"format":"json","max_bytes":8400}})
call_mcp_tool(ServerName="gortex", ToolName="explore", Arguments={"operation":"context","task":"定位登录鉴权失败原因 path:src/auth","options":{"fidelity":"graded","token_budget":4000},"output":{"format":"json","max_bytes":8400}})
```

---

## 二、 源码精读中枢 (`read` [7])
> **容器铁律**：平铺门面。`target` 严格单字段闭集。**仅 `read.summary` 接受 `repo` / `project` / `scope` / `workspace` / `ref`**，其余 6 个操作均**严禁**传入这些过滤键！

### 核心门禁
1. **符号优先**：已通过 `search`/`explore`/`relations` 拿到符号 ID 后，**必须优先使用 `read.symbols`（批量 2~8 个）或 `read.source`（单个）**，严禁退化用 `read.file` 扫读整文件（仅需查看文件周边注释、非导出模板代码、导入区或编辑多行锚点时，方可用局部 `read.file` 补读）。
2. **`read.file` 双重大小锁**：**绝对禁止不传 `offset`+`limit` 盲读整文件！** 必传 `options: {"offset": 1基行号, "limit": <=150}` + `output: {"max_bytes": 8400}`（**`read.file` 仅认 `output.max_bytes`，严禁传 `output.format`！**）。针对注释密集、压缩行或中文 Markdown 文件，强烈建议将 `limit` 收紧到 `40~80` 并追加 `options.max_chars`（如 `8000`~`14000`），避免单次 `content` 过长触发客户端落盘截断。
3. **全局大文件降维阅读**：看全部符号骨架优先 `read.summary`（免读函数体）或 `read.file` 配 `context: {"compress_bodies": true, "max_lines": 120}`（可用 `options.keep` 保全重点符号体、`options.fidelity_globs` 分级控制）。
4. **会话历史认知**：`read.history` 返回的是**当前 Gortex 会话内修改过的符号计次与足迹**，不是 Git 提交历史；查 Git 行级历史或高扰动文件用 `workspace_admin.blame` 或 `analyze(kind="churn")`。

### 全 7 操作契约
| 触发场景 | `operation` | `target`（严格单属性） | 完整参数与精确 `output` |
| :--- | :--- | :--- | :--- |
| 批量精读 2~8 个符号实现（`include_source` 默认即为 `true`） | `"symbols"` | `{ "symbols": ["repo/...::fn1", ...] }` 或 `{ "symbol": "id" }` | `context?: { context_lines?: 3 }`, `options?: { include_source?: true, if_none_match?: "<etag>" }`（禁 `repo`）；**`[JSON+MB]`** |
| 精读单个函数/类完整源码（可折叠体省 Token） | `"source"` | `{ "symbol": "repo/...::fn1" }` | `context?: { compress_bodies?: false, context_lines?: 3, max_lines?: N }`, `options?: { allow_secrets?: false, if_none_match? }`（禁 `repo`）；**`[JSON+MB]`** |
| 局部文件切片读取 / 非代码配置 / 物理摘要取证 | `"file"` | `{ "file": "repo/path/to/file" }` | **必传 `options: { offset, limit (<=150) }`**，可选 `max_chars?`, `allow_secrets?: false`, `keep?: "Sym1,Sym2"`, `fidelity_globs?: "internal/**:full,*_test.go:omit"`, `if_none_match?`, `physical_evidence?: bool`, `digest?: "sha256"`, `new_user_task?: bool`；`context?: { compress_bodies?, max_lines? }`；**`output: { "max_bytes": 8400 }`（绝对禁传 `format`！）** |
| 文件全部符号结构大纲（唯一支持 `repo` 等过滤器的 `read` 操作） | `"summary"` | `{ "file": "repo/path/to/file" }` | `options?: { repo?, project?, scope?, workspace?, ref?, max_tokens?, if_none_match? }`（多仓查非当前 `active_project` 文件时必传 `options.repo`）；`output: { format:"json", max_bytes:8400, compact? }` **`[JSON+MB]`** |
| 修改前单次装配文件/符号与直接上下游依赖上下文 | `"editing_context"` | `{ "file": "..." }` 或 `{ "symbol": "..." }` | `context?: { compress_bodies?: true }`, `options?: { detail?: "brief"\|"full", keep?, fidelity_globs?, max_tokens?, if_none_match? }`（禁 `repo`）；**`[JSON+MB]`** |
| 查询当前会话修改的符号计数/轨迹 | `"history"` | 可省略 `target` 或传 `{ "symbol": "..." }` | `options?: {}`, `output?: { compact?: false }`（无 `format`/`max_bytes`） |
| 读取 `.gortex.yaml::artifacts` 登记的非代码知识资产 | `"artifact"` | `{ "artifact": "name" }` 或改传 `options.path` | `options?: { path? }`（单文件硬上限 64 KiB，禁 `repo`）；**`[JSON]`（无 `max_bytes`）** |

```json
call_mcp_tool(ServerName="gortex", ToolName="read", Arguments={"operation":"symbols","target":{"symbols":["edit-rust/backend/src/main.rs::main","edit-rust/backend/src/lib.rs::init"]},"output":{"format":"json","max_bytes":8400}})
call_mcp_tool(ServerName="gortex", ToolName="read", Arguments={"operation":"file","target":{"file":"edit-rust/backend/assets/gortex提示词.md"},"options":{"offset":1,"limit":80,"max_chars":12000},"output":{"max_bytes":8400}})
call_mcp_tool(ServerName="gortex", ToolName="read", Arguments={"operation":"summary","target":{"file":"edit-rust/backend/src/main.rs"},"options":{"repo":"edit-rust"},"output":{"format":"json","max_bytes":8400}})
```

---
## 三、 修改与重构中枢 (`edit` [10] / `refactor` [6])
> **容器铁律**：均为平铺门面。`edit.batch`/`apply_overlay`/`docs`/`export_graph`/`wiki`/`skill` **不传 `target`**。`edit.batch`/`symbol`/`file`/`write`/`apply_overlay`/`scaffold`/`skill` 与 `refactor.rename`/`delete`/`move`/`inline` **绝对禁止传 `repo` / `project` / `workspace` / `scope`**！

### 核心门禁
1. **分批策略与新文件边界**：跨文件/多处联动修改优先合并为单轮 **`edit.batch`**，但严禁把海量修改硬塞进单个超大型请求导致客户端流式超时取消或服务端 60s 超时回滚（大重构按模块拆分，单批保持体量紧凑）；**创建新文件必须用 `edit.write`**（`edit.batch` 的 `edit_file` 对不存在文件会立即失败）。
2. **`edit.symbol` 片段替换与语法门禁真实边界（修正旧版误传）**：
   * `edit.symbol` 既支持替换整符号源码，也**完全支持仅替换函数体内部或头部文档注释中的精确小片段**，因此应尽量只传入目标符号内唯一的小块 `match` / `replacement`，以大幅缩小调用输出、消除取消风险；
   * `edit.file`、`edit.write` 与 `refactor.rename` 带有 tree-sitter 语法门禁（`checkParseGate`，拒绝引入新语法错误），且提供 **`options.allow_parse_errors: true`** 开关；而 **`edit.symbol` 无 `allow_parse_errors` 选项**（强传会报未知字段错！）；
   * `refactor.fix_all` 与 `refactor.apply_code_action` **无 `dry_run` 参数**，且基于 **0-based** 行/列偏移（与 `change.code_actions` 对齐）；
   * `refactor.move` 与 `refactor.inline` **当前仅支持 Go 语言**；
   * `edit.scaffold` 底层默认 `dry_run: true`，需实际写入磁盘时**务必显式传 `dry_run: false`**！

### 全 16 操作契约（除明确标有 `format` 者外，其余写操作均严禁传 `format`！）
| 触发场景 | 门面与 `operation` | 必填/核心参数与精确容器规则 |
| :--- | :--- | :--- |
| 单/跨文件多处原子批量改删移 | `edit("batch")` | **无 `target`**！必填 **`changes`** 数组，4 类 `op` 字段必须严格隔离：<br>① `{"op":"edit_file","path":"repo/...","old_string":"...","new_string":"...","replace_all?":false}`<br>② `{"op":"edit_symbol","id":"repo/...::fn","old_source":"...","new_source":"..."}`<br>③ `{"op":"move_file","source":"repo/...","destination":"repo/...","expected_sha256?":"..."}`<br>④ `{"op":"delete_file","path":"repo/...","expected_sha256?":"..."}`<br>可选顶层 `dry_run?: false`, `options?: { transaction_id?, status_only?: bool }`, `output?: { compact?: bool }` |
| 按符号 ID 精准替换（整符号或内部小片段） | `edit("symbol")` | 必填 `target: { symbol }` + 顶层 **`match`** + **`replacement`**；可选 `dry_run?: false`, `options?: { base_sha?, mutation_id?, physical_evidence?, digest?: "sha256" }`（**无 `allow_parse_errors`**） |
| 单文件单处或全部精确文本替换 | `edit("file")` | 必填 `target: { file }` + 顶层 **`match`** + **`replacement`**；可选 `dry_run?: false`, `guard?: { expected_occurrences?: N }`, `options?: { replace_all?: false, allow_parse_errors?: false, base_sha?, mutation_id?, physical_evidence?, digest?: "sha256" }` |
| **新建文件**或整文件全量覆写 | `edit("write")` | 必填 `target: { file }` + 顶层 **`content`**；可选 `dry_run?: false`, `options?: { allow_parse_errors?: false, base_sha?, mutation_id?, physical_evidence?, digest?: "sha256" }` |
| 将内存 `overlay` 分支合并并真正落盘 | `edit("apply_overlay")` | **无 `target`**！必填 `options: { from: "<分支名>" }`，可选 `into?: "main"`, `force?: false` |
| 按参考符号生成同模式新实现+测试桩脚手架 | `edit("scaffold")` | 必填 `target: { symbol }` + `options: { new_name }`；`dry_run` 默认常为 `true`，实写须传 `dry_run: false`；`output?: { compact? }` |
| 生成活文档 / 导出图谱 / 生成 Wiki / 打包 Skill | `edit("docs" \| "export_graph" \| "wiki" \| "skill")` | **均无 `target`**！<br>• `docs`: `options?:{include?, output_path?, path_prefix?, run_blame?, since?, top?, workspace?}`, `output?:{format:"markdown"\|"json"}`<br>• `export_graph`: `options?:{kinds?, languages?, repo?, scope?, max_communities?, min_community?, no_synthetic?, output_path?, output_dir?}`, `output?:{format:"cypher"\|"graphml"\|"mermaid"}`<br>• `wiki`: `options?:{output_dir?, repo?, project?, workspace?, max_communities?, min_community?, force?, wikilinks?, no_contracts?, no_docs?, no_processes?}`, `output?:{format:"markdown"\|"html"}`<br>• `skill`: `options?:{directory?, skill_name?, description?, output_dir?, include_patterns?, ignore_patterns?, compress?, max_references?}`, `dry_run?:false`, `output?:{format:"json"\|"gcx"\|"toon"}` |
| 全局拓扑协同重命名（跨引用+跨文件） | `refactor("rename")` | 必填 `target: { symbol }` + 顶层 **`new_name`**；可选 `dry_run?: false`, `options?: { allow_parse_errors?: false }` |
| 循环收敛 LSP 自动修复诊断 | `refactor("fix_all")` | 必填 `target: { file }`；`options?: { repo?, kinds? (默认 `"quickfix,source.organizeImports"`), diagnostic_timeout_ms?: 5000, max_actions_per_iter?: 50, max_iterations?: 5 }`（**无 `dry_run`**） |
| 按索引应用单个 LSP CodeAction | `refactor("apply_code_action")` | 必填 `target: { file }` + 必填 `options: { index (来自change.code_actions), start_line?, start_char?, end_line?, end_char? (均0-based!), only?, repo? }`（**无 `dry_run`**） |
| 搬移符号到新包/文件 / 安全级联删除 / 内联调用点 | `refactor("move" \| "delete" \| "inline")` | • `move`(**仅Go**): 必填 `target:{symbol}` + 顶层 **`destination`**, `dry_run?:false`, `options?:{target_package?}`<br>• `delete`: 必填 `target:{symbol}`, `dry_run?:false`, `options?:{cascade?:"off"\|"preview"\|"apply", cascade_into_tests?, force?, propagate?}`, **`[JSON]`**<br>• `inline`(**仅Go**): 必填 `target:{symbol}`, `dry_run?:false`, `options?:{delete_after?:true}` |

```json
call_mcp_tool(ServerName="gortex", ToolName="edit", Arguments={"operation":"batch","changes":[{"op":"edit_file","path":"edit-rust/src/a.rs","old_string":"old","new_string":"new","replace_all":false},{"op":"edit_symbol","id":"edit-rust/src/b.rs::calc","old_source":"x + 1","new_source":"x + 2"}],"dry_run":false,"options":{"transaction_id":"tx-a1b2-01"}})
call_mcp_tool(ServerName="gortex", ToolName="edit", Arguments={"operation":"write","target":{"file":"edit-rust/src/new_mod.rs"},"content":"pub fn ready() -> bool { true }\n","dry_run":false})
call_mcp_tool(ServerName="gortex", ToolName="refactor", Arguments={"operation":"rename","target":{"symbol":"edit-rust/src/a.rs::old_fn"},"new_name":"new_fn","dry_run":false})
```
---
## 四、 变更核验与质量中枢 (`change` [19])
> **容器铁律**：平铺门面。除表内明确列出 `repo`/`project`/`ref` 的 5 个操作（`detect`、`diagnostics`、`code_actions`、`api_impact`、`contract`）外，其余 14 个操作**严禁传 `repo` / `project` / `workspace` / `scope`**！

### 核心门禁
源码落盘后严禁凭直觉假设正确，必走 **`change.detect` → `change.diagnostics` → `change.tests`** 闭环，再配宿主原生测试命令跑通实际编译与单测；仅修改纯文档/纯提示词/注释时免强跑 `diagnostics`/`tests`，未启动语言服务（LSP）时以原生构建与测试兜底。

### 全 19 操作契约
| 触发场景 | `operation` | 参数归属、`repo` 权限与精确 `output`（注意 `limit`/`summary_only` 归属区别！） |
| :--- | :--- | :--- |
| 盘点未提交变动及图谱受影响符号 | `"detect"` | `source?: { scope?: "unstaged"\|"staged"\|"all"\|"compare" }`, `options?: { repo?, base_ref?, limit?: 50, offset?: 0 }`, `output?: { summary_only?: bool }`（无 `format`/`max_bytes`） |
| 在内存影子区预演单次 / 多步 LSP WorkspaceEdit | `"preview"` \| `"simulate"` | `preview`: **必填 `source: { workspace_edit: "<JSON字符串>" }`**, `options?: { diagnostics?: true, diagnostics_timeout_ms?, inherit_overlay?: bool }`<br>`simulate`: **必填 `source: { steps: "<WorkspaceEdit[] JSON字符串>" }`**, `options?: { diagnostics?: true, diagnostics_timeout_ms?, inherit_overlay?: bool, stop_on_error?: true }` |
| 提取实时 LSP 诊断 / 查询可用 CodeAction（0-based） | `"diagnostics"` \| `"code_actions"` | `diagnostics`: **必填 `source: { file }`**, `options?: { repo?, project?, wait?: false, timeout_ms? }`<br>`code_actions`: **必填 `source: { file }`**, `options?: { start_line? (0-based), start_char?, end_line?, end_char?, only?, repo?, project? }` |
| 查询写操作磁盘落盘回执（写超时的必查口） | `"receipt"` | `options?: { receipt?: "...", mutation_id?: "..." }` 或 `source?: { path?: "repo/..." }` |
| 受影响测试 / 架构规则守卫 / 风险分级爆炸半径 | `"tests"` \| `"guards"` \| `"impact"` | 均**必填 `target: { symbol \| symbols:[...] }`**（仅 `impact` 另支持 `target: { file }`，且均禁传 `repo`！）<br>• `tests`: `options?: { depth?: 3 }`, `output?: { compact?: bool }`（无 `format`/`max_bytes`）<br>• `guards`: `output?: { compact?: bool }`, **`[JSON+MB]`**<br>• `impact`: `output?: { format:"json", max_bytes:8400, limit?: 100, offset?: 0, summary_only?: bool }` **`[JSON+MB]`**（注意 `limit`/`offset`/`summary_only` 放 `output`，不要放 `options`！） |
| 改前全流水线风险合同裁决 (`allow`\|`warn`\|`refuse`) | `"contract"` | `target?: { symbol \| symbols }` 或 `source?: { scope?, base?, path?, start_line?, end_line?, ranges?: "<JSON字符串>", workspace_edit?: "<JSON字符串>" }`, `options?: { risk_gate?, lens?: "api", repo?, start_line?, end_line? }`；**`[JSON]`（无 `max_bytes`）** |
| HTTP/gRPC/主题路由变更影响分析 | `"api_impact"` | `source?: { file }` 或 `options?: { route, repo?, project?, ref? }`；**`[JSON+MB]`** |
| 多符号编辑依赖拓扑排序建议 | `"edit_plan"` | **必填 `target: { symbol \| symbols }`**；`options?: { depth?: 3 }`（无 `output`） |
| 按示例符号提取可复用结构编码模式 | `"pattern"` | **必填 `source: { symbols: ["id1"] }`（底层按单锚点取首项）**, `options?: { max_source_lines?: 40 }`, `output?: { max_bytes?: 8400 }`（无 `format`） |
| 把 `file` 或行区间转换为对应 Gortex 符号清单 | `"ranges"` | **必填 `source: { file \| ranges: [{"file":"...","start_line":1,"end_line":20}] }`（对象数组，区别于 `contract` 的字符串）**, `options?: { start_line?, end_line? }`；**`[JSON]`（无 `max_bytes`）** |
| 验证接口签名变更会破坏哪些调用点 | `"verify"` | **必填 `source: { changes: [{"symbol_id":"...","new_signature":"..."}] }`**, `output?: { compact?: bool }` |
| 列/查会话 overlay 状态，或对比分支/overlay 拓扑影响差异 | `"overlay_branches"` \| `"overlay_state"` \| `"compare_branches"` \| `"compare_overlay"` | `overlay_branches` / `overlay_state`: 仅 `operation`<br>`compare_branches`: 必填 `target:{symbol}` + `options:{a, b, kind:"find_usages\|get_callers\|get_call_chain\|get_dependencies\|get_dependents", depth?, limit?}`<br>`compare_overlay`: 必填 `target:{symbol}` + `options:{kind, depth?, limit?}` |

```json
call_mcp_tool(ServerName="gortex", ToolName="change", Arguments={"operation":"detect","source":{"scope":"unstaged"},"options":{"repo":"edit-rust","limit":50}})
call_mcp_tool(ServerName="gortex", ToolName="change", Arguments={"operation":"diagnostics","source":{"file":"edit-rust/backend/src/main.rs"},"options":{"repo":"edit-rust","wait":false}})
call_mcp_tool(ServerName="gortex", ToolName="change", Arguments={"operation":"impact","target":{"symbol":"edit-rust/backend/src/main.rs::main"},"output":{"format":"json","max_bytes":8400,"summary_only":true}})
```
---
# 【L2 级：深度拓扑与架构审计】（进阶 · 析-联-迹三大中枢）

## 五、 架构分析中枢 (`analyze` [96] — 顶层必传 `kind`，严禁写 `operation`！)
> **容器与重要 Schema 警告**：
> 1. `analyze` 是唯一不用 `operation` 的平铺门面，**顶层必须用 `"kind": "<类型名>"`**。
> 2. 仅 **`co_change`、`why`、`def_use` 必填 `target`**，仅 **`impact` 可选 `target`**；**其余 92 个 `kind` 均严禁传入 `target`！**
> 3. **实机 `capabilities` 关键预警（必读！）**：虽然部分 `kind` 的摘要文字里提到了过滤提示（如 `routes` 提到 `method/path/type`、`models` 提到 `orm/table/model`、`k8s_resources` 提到 `k8s_kind/namespace`、`kustomize` 提到 `dir`、`event_emitters` 提到 `level/name`、`field_writers` 提到 `id`、`annotation_users` 提到 `id/name`、`stale_flags` 提到 `older_than`、`images` 提到 `ref`、`fixes_history` 等），但在当前 **`facade-v1` 静态校验层，这些额外过滤字段并未对这些 `kind` 开放**！若硬传非通用字段，`validateFacadeRequestFields` 会直接报 `unknown field` 拒绝。调用下表分组 2 中未单列特例参数的操作时，**`options` 请仅限使用通用域过滤 `{repo?, project?, scope?, workspace?}`**！

### 核心门禁
大规模重构或动用公共抽象前，优先运行 `cycles` 与 `dead_code`（及 `hotspots`）；排查竞态与状态共享风险用 `race_writes` 与 `field_writers`；注意本门面**不存在 `"layers"` 类型**（层映射建议用 `suggest_boundaries`）。

### 1. 高频与带专属静态参数 `kind` 精要表（共 43 项）
| 分类场景 | `kind` | 必填 / `target` / 专属 `options` / `repo` 权限与精确 `output` |
| :--- | :--- | :--- |
| 模块依赖环 / 高耦合热点 / 死代码 | `"cycles"` \| `"hotspots"` \| `"dead_code"` | **通用域过滤** `{repo?, project?, scope?, workspace?}`（⚠️ **`cycles` 中 `options.scope` 是目录/包路径前缀**，非 saved scope！）；`hotspots`: `options:{threshold?}`; `dead_code`: `options:{include_variables?, include_fields?, include_constants?, include_cgo_exports?, include_linkname_targets?, skip_cross_repo_nodes?}` — **均 `[JSON+MB]`**（支持 `compact?`） |
| 全局架构快照 / 社区探测 / 聚类算法对比 | `"architecture"` \| `"communities"` \| `"clusters"` \| `"louvain"` \| `"concepts"` | • `architecture`: `options:{path_prefix?, resolution?:"file\|package\|service\|system", top_communities?, top_entry_points?, top_hotspots?, top_processes?, ...域过滤}`, **`[JSON]`**（无 `max_bytes`）<br>• `communities`: `options:{id?, ...域过滤}`（无 `output`）<br>• `clusters`: `options:{algorithm?:"leiden\|louvain\|spectral", min_size?:3, resolution?:1.0, ...域过滤}`, **`[JSON+MB]`**<br>• `louvain`: 仅通用域过滤, **`[JSON+MB]`**<br>• **`concepts`**（v601 漏录项，固定 `use_llm:false`）: 仅通用域过滤, **`[JSON+MB]`** |
| 耦合度与不稳定性 / Git 共变耦合 | `"coupling"` \| `"co_change"` | • `coupling`: `options:{unit?:"package\|community", package_depth?:2, min_members?:1, path_prefix?, sort_by?}`, `output:{format:"json", limit?:50}`（**无 `max_bytes`**）<br>• `co_change`: 固定 `refresh:false`; **必填 `target:{file\|symbol}`** + `options:{file_path?, symbol_id?, min_score?}`（无 `repo`），`output:{format:"json", limit?:20}` |
| 综合爆炸半径评分 / 竞态写入 / 提交扰动热度 | `"impact"` \| `"race_writes"` \| `"churn"` | • `impact`: 固定 `refresh_cochange:false`; **可选 `target:{file\|symbol}`** + `options:{id?, ids?, path?, kinds?, min_score?, max_score?, path_prefix?, ...域过滤}`, **`[JSON+MB]`**(`compact?, limit?`)<br>• `race_writes`: 仅通用域过滤, **`[JSON+MB]`**<br>• `churn`: `options:{kinds?:"function,method", min_commits?:1, path_prefix?, sort_by?}`, `output:{format:"json", limit?:100}`（**无 `max_bytes`**） |
| 复杂度评级 / 多轴健康分 / 未测盲区 / 可提取函数推荐 | `"health"` \| `"health_score"` \| `"untested"` \| `"extraction"` | • `health`: 仅通用域过滤（**无 `output`**）<br>• `health_score`: `options:{grade?:"d,f", min_score?, max_score?, min_axes?:1, roll_up?:"file\|repo", ...域过滤}`, **`[JSON+MB]`**(`compact?`)<br>• `untested`: `options:{file_prefix?, min_fan_in?}`（禁 `repo`）, `output:{limit?:50}`（**无 `format`/`max_bytes`**）<br>• `extraction`: `options:{min_callers?, min_fan_out?, min_lines?, path_prefix?}`（禁 `repo`）, `output:{format:"json", limit?:25}` |
| 规则巡检目录与扫描 / 无 LSP 外置 Lint / SAST / 预置安全包 | `"inspection_catalog"` \| `"inspections"` \| `"lint"` \| `"sast"` \| `"hygiene"` \| `"named"` | • `inspection_catalog`: `options:{category?}`（**禁 `repo`**）, **`[JSON]`**<br>• `inspections`: `options:{inspections:"all\|...", severity?, path_prefix?, max_per_inspection?, ...域过滤}`, **`[JSON]`**<br>• `lint`: **必填 `options:{path, language?:"go\|python\|shell", timeout_ms?}`**（**禁 `repo`，无 `output`**）<br>• `sast` / `hygiene`: `options:{cwe?, detector?, language?, severity?, exclude_tests?, kinds_only?, ...域过滤}`, **`[JSON+MB]`**<br>• `named`: `options:{name? ("sql-injection\|xss\|ssrf\|hardcoded-secrets\|..."), ...域过滤}`, **`[JSON+MB]`** |
| 存在动机追溯 / 故障回放 / 预检成环 | `"why"` \| `"replay"` \| `"would_create_cycle"` | • `why`: **必填 `target:{symbol}` + `options:{id, scope?:"workspace"\|"global"}`**（禁 `repo`），`output:{format:"json", limit?:20}`<br>• `replay`: `options:{anchor_symbol?, depth?:3, window_days?:30 (0禁用)}`, `output:{format:"json", limit?:25}`（**禁 `repo`**）<br>• `would_create_cycle`: **必填 `options:{from_id, to_id}`**（+可选域过滤），**`[JSON+MB]`** |
| 覆盖率与所有权 / 陈旧代码与开关 / 克隆与执行流 | `"coverage_gaps"` \| `"coverage_summary"` \| `"ownership"` \| `"stale_code"` \| `"stale_flags"` \| `"clones"` \| `"processes"` | • `coverage_gaps`: `options:{min_pct?:0, max_pct?:100, path_prefix?, ...域过滤}`, **`[JSON+MB]`**<br>• `ownership`: `options:{kinds?:"function,method\|all", min_symbols?:1, path_prefix?, ...域过滤}`, **`[JSON+MB]`**<br>• `stale_code`: `options:{older_than?:365, email?, kinds?, ...域过滤}`, **`[JSON+MB]`**<br>• `stale_flags`: `options:{provider?:"launchdarkly\|growthbook\|unleash\|internal", ...域过滤}`（无 `older_than`）, **`[JSON+MB]`**<br>• `clones`: `options:{min_similarity?, dead_only?, path_prefix?, ...域过滤}`, **`[JSON+MB]`**(`limit?:50`)<br>• `processes`: `options:{id?, ...域过滤}`, **`[JSON+MB]`**<br>• `coverage_summary`: 仅通用域过滤, **`[JSON+MB]`** |
| 代理规则审计 / 引用验真 / 文档盲区 / 异常边 / 增量 / TODO / 契约 / 边界建议 | `"agent_config"` \| `"citation"` \| `"knowledge_gaps"` \| `"surprising_connections"` \| `"recent_changes"` \| `"todos"` \| `"contracts"` \| `"suggest_boundaries"` | • `agent_config`: `options:{files?, root?}`（**禁 `repo`**）, `output:{compact?}`（无 `format`/`max_bytes`）<br>• `citation`: **必填 `options:{file_path, span, sha?}`**（**禁 `repo`**）, **`[JSON]`**<br>• `knowledge_gaps`: `options:{path_prefix?, min_coverage_pct?, hotspot_limit?, thin_community_size?, limit_per_category?}`（禁 `repo`）, **`[JSON]`**<br>• `surprising_connections`: `options:{path_prefix?, hub_threshold?, min_score?, rare_kind_pct?}`（禁 `repo`）, `output:{format:"json", limit?:25}`<br>• `recent_changes`: `options:{since?, ...域过滤}`（watch 模式，**无 `output`**）<br>• `todos`: `options:{tag?, assignee?, has_assignee?, ticket?, ...域过滤}`, **`[JSON+MB]`**<br>• `contracts`: `options:{action?:"list\|check\|validate\|bridge", ref?, ...域过滤}`, **`[JSON+MB]`**(`cursor?, fields?`)<br>• `suggest_boundaries`: 仅通用域过滤, **`[JSON+MB]`** |
| 跨仓边汇总 / 过程内定义-使用链 | `"cross_repo"` \| `"def_use"` | • `cross_repo`: `options:{base_kind?:"calls\|implements\|extends", ...域过滤}`（**注意此处 `options.repo` 作为边界仓过滤**）, `output:{compact?, format:"json", limit?:200, max_bytes:8400}`<br>• `def_use`: **必填 `target:{symbol}`** + `options:{id?, ...域过滤}`, **`[JSON+MB]`**(`compact?`) |

### 2. 全量其余专项 `kind` 目录（其余 53 项，全计 **43 + 53 = 96** 个全收录）
> 除个别特例标明外，本组 `options` 统一支持且**仅支持** `{repo?, project?, scope?, workspace?}`，`output` 统一支持 **`[JSON+MB]`**（配 `compact?`）：

* **图谱拓扑、函数流与诊断（15）**：`bottlenecks`、`error_surface`、`field_writers`、`indirect_mutations`、`kcore`、`pagerank`、`scc`、`wcc`、`connectivity_health`、`edge_audit`、`ref_facts`、`resolution_outcomes`、`retrieval_log`、`speculative`、`synthesizers`。
* **并发、通道与不安全模式（4）**：`channel_ops`、`unclosed_channels`、`goroutine_spawns`、`unsafe_patterns`。
* **框架、路由、数据与事件（15）**：`routes`、`route_frameworks`、`models`、`dbt_models`（**另支持 `options:{framework?:"dbt\|sqlmesh", materialized?}`**）、`orphan_tables`、`unreferenced_tables`、`sql_call_sites`（固定 `materialize:false`）、`config_readers`、`env_var_users`、`event_emitters`、`log_events`、`string_emitters`、`pubsub`、`temporal_orphans`、`domain`。
* **前端 / 移动端 / 语言生态专项（10）**：`components`、`swiftui_views`、`uikit_classes`、`drupal_hooks`、`cgo_users`、`wasm_users`、`annotation_users`、`constructors_missing_fields`、`external_calls`、`tests_as_edges`（**另支持 `options:{group_by?:"symbol\|test"}`**）。
* **基础设施、发布与历史演进（9）**：`images`、`k8s_resources`、`kustomize`、`releases`（**另支持 `options:{tag?}`**）、`fixes_history`、`doc_staleness`、`role`、`review`（本地规则引擎）、`help`（`options:{}`，**禁 `repo`，无 `output`**）。

```json
call_mcp_tool(ServerName="gortex", ToolName="analyze", Arguments={"kind":"cycles","options":{"repo":"edit-rust"},"output":{"format":"json","max_bytes":8400}})
call_mcp_tool(ServerName="gortex", ToolName="analyze", Arguments={"kind":"architecture","options":{"repo":"edit-rust","resolution":"package"},"output":{"format":"json"}})
call_mcp_tool(ServerName="gortex", ToolName="analyze", Arguments={"kind":"lint","options":{"path":"edit-rust/backend/src/main.rs"}})
```
---
## 六、 关系网络中枢 (`relations` [11])
> **容器铁律**：平铺门面。前置 8 项支持通用域过滤 `{repo?, project?, scope?, workspace?, ref?}`；**`references`、`declaration`、`import_path` 绝对严禁传 `repo` / `project` / `scope` / `workspace`**！

### 核心门禁
修改任何公共符号签名、返回值或可见性前，**必先调用 `callers` 或 `usages`** 排查所有引用方；向下查依赖的唯一合法操作名为 **`dependencies`（严禁自创 `callees`）**；查引用用 `usages`/`references`（严禁写 `instantiations`）；查实现类/接口实现用 `implementations`。
* **通用置信度过滤**：`min_tier`（按可信度降序：`lsp_resolved` > `lsp_dispatch` > `ast_resolved` > `ast_inferred` > `text_matched`）与 `include_speculative?: bool`。

| `operation` | `target`（严格单属性） | 核心参数与精确 `output`（注意 `exclude_tests` 权限区别！） |
| :--- | :--- | :--- |
| `"callers"` | `{ "symbol": "id" }` | `options?: { depth?: 2, limit?: 40~50, exclude_tests?: bool, min_tier?, include_speculative?, ...域过滤 }`；**`[JSON+MB]`**(`compact?`) |
| `"dependencies"` | `{ "symbol": "id" }` | `options?: { depth?: 2, limit?: 40~50, min_tier?, include_speculative?, ...域过滤 }`（⚠️ **不支持 `exclude_tests`!**）；**`[JSON+MB]`**(`compact?`) |
| `"usages"` | `{ "symbol": "id" }` | `options?: { context?: "parameter_type\|return_type\|field\|value\|type\|attribute\|generic_arg\|call\|import", flavor?, return_usage?: "discarded\|assigned\|partially_ignored\|returned\|goroutine\|deferred\|argument\|condition", group_by?: "file", exclude_tests?: bool, limit?: 50, max_tokens?, min_tier?, include_speculative?, ...域过滤 }`；**`[JSON+MB]`**(`compact?`) |
| `"implementations"` / `"overrides"` / `"hierarchy"` | `{ "symbol": "id" }` | `overrides`: `options:{direction?:"children\|parents"}`; `hierarchy`: `options:{direction?:"both\|up\|down", depth?:5, include_methods?:bool}`; 三者均支持通用域过滤 + `min_tier` + `include_speculative`；**`[JSON+MB]`** |
| `"dependents"` / `"cluster"` | `{ "symbol": "id" }`（`dependents` 不支持 `target.file`） | `dependents`: `options:{depth?:3, limit?:50, min_tier?, include_speculative?, ...域过滤}`（**不支持 `exclude_tests`!**）; `cluster`: `options:{radius?:2, limit?:50, ...域过滤}`; 均 **`[JSON+MB]`**(`compact?`) |
| `"references"` | 可选 `{ "symbol": "id" }` 或省略改用 `options.name` | `options?: { name?, evidence_limit?: 50, exclude_tests?, min_tier? }`（**禁 `repo`等**）；**`[JSON]`（无 `max_bytes`）** |
| `"declaration"` | `{ "query": "使用点片段" }` | `options?: { kind?, limit?: 20, path_prefix?, regex?: false }`（**禁 `repo`等**）；**`[JSON]`**（支持 `format:"json\|toon"`） |
| `"import_path"` | `{ "symbol": "id" }` | **必填 `options: { path: "目标文件相对路径" }`**（**禁 `repo`等，无 `output`**） |

```json
call_mcp_tool(ServerName="gortex", ToolName="relations", Arguments={"operation":"callers","target":{"symbol":"edit-rust/backend/src/tools/gortex/mod.rs::init"},"options":{"limit":40,"exclude_tests":true},"output":{"format":"json","max_bytes":8400}})
```
---
## 七、 链路追踪中枢 (`trace` [7])
> **容器铁律**：平铺门面。**双端点操作（`path`、`flow`、`taint`）必须同时传 `target` 与 `to`**；单点/查询操作（`cfg`、`call_chain`、`walk`、`graph`）**严禁传 `to`**；**仅 `call_chain` 和 `walk` 支持 `{repo?, project?, scope?, workspace?, ref?}`**，其余 5 项严格禁传！`call_chain` **严禁传 `output.compact: true`**（以防丢失行号调用链路）。

| `operation` | 端点容器与核心参数 | 精确 `output` |
| :--- | :--- | :--- |
| `"path"` \| `"flow"` | `target:{symbol:"A"}` + `to:{symbol:"B"}`；`path`: `options:{max_depth:8~24, k?:1, include_references?:true, max_frontier?, min_tier?}`; `flow`: `options:{max_depth?:8, max_paths?:10, min_tier?}` | **`[JSON+MB]`** |
| `"taint"` | `target:{query:"输入源关键字"}` + `to:{query:"危险点关键字"}`, `options:{max_depth?:8, limit?:20, min_tier?}` | **`[JSON+MB]`** |
| `"cfg"` \| `"call_chain"` | `target:{symbol:"id"}`；`cfg`: `options:{mermaid?:true}`; `call_chain`: `options:{depth?:4, limit?:50, max_tokens?, min_tier?, include_speculative?, ...域过滤}` | **`[JSON+MB]`**（`call_chain` 严禁 `compact:true`） |
| `"walk"` \| `"graph"` | `walk`: 必填 `target:{symbol}` + `options:{edge_kinds?:"calls", direction?:"out\|in\|both", max_depth?:8, token_budget?:6000, community?, ...域过滤}`; `graph`: **无 `target`，必填 `options:{query:"DSL语句", limit?:100}`**（禁 `repo`） | **`[JSON]`（无 `max_bytes`）** |

```json
call_mcp_tool(ServerName="gortex", ToolName="trace", Arguments={"operation":"path","target":{"symbol":"edit-rust/src/a.rs::fn1"},"to":{"symbol":"edit-rust/src/b.rs::fn2"},"options":{"max_depth":16,"k":1},"output":{"format":"json","max_bytes":8400}})
call_mcp_tool(ServerName="gortex", ToolName="trace", Arguments={"operation":"graph","options":{"query":"nodes kind=interface | traverse implements in","limit":100},"output":{"format":"json"}})
```
---
# 【L3 级：协同推演、审查、PR 与空间/会话运维】

## 八、 虚拟推演与认知记忆 (`overlay` [10] / `recall` [8] / `remember` [8] — 均冷门面 `arguments`)
> 注意：`recall`/`remember` 内的 `scope` 指代认知记忆域（`"workspace" | "global" | "both"`），切勿与多仓 `saved-scope` 混淆；除 `remember.risk_ack` 支持 `repo` 外，其余操作均不传仓库级 `repo`。

### 1. 内存虚拟分支推演 (`overlay` — 10 操作，均无 `format`)
| `operation` | `arguments: { ... }` 核心参数与服务端固定行为 |
| :--- | :--- |
| `"register"` / `"drop"` / `"keepalive"` | `register`: `{workspace_id?}`；`drop`: `{}`（销毁整个会话 overlay）；`keepalive`: `{}`（续期会话 TTL） |
| `"fork"` / `"switch"` / `"drop_branch"` | `fork`: **必填 `name`**, 可选 `from?, activate?:bool`；`switch` / `drop_branch`: **必填 `name`** |
| `"push"` / `"delete"` | `push`: **必填 `path`**, 可选 `content?, base_sha?, deleted?:bool`；`delete`: **必填 `path`** |
| `"merge"` / `"simulate"` | `merge`: **必填 `from`**, 可选 `into?:"main", force?:false`（**固定 `to_disk:false`，仅在内存合并**；落盘请用 `edit.apply_overlay`）；`simulate`: **必填 `steps:"<WorkspaceEdit[] JSON字符串>"`**, 可选 `diagnostics?:true, diagnostics_timeout_ms?, inherit_overlay?, stop_on_error?:true`（固定 `keep:true`） |

### 2. 认知记忆检索与召回 (`recall` — 8 操作，均 **`[冷JSON]`** 无 `max_bytes`)
| `operation` | `arguments: { ... }` 核心参数 |
| :--- | :--- |
| `"surface"` | `{task?, symbol_ids?, file_paths?, scope?:"workspace"\|"global"\|"both", limit?:10, min_score?, excerpt_chars?, include_superseded?}`（服务端固定 `mark_accessed:false`） |
| `"distill"` | `{max_symbols?:10, max_files?:10, max_tags?:10, max_recent?:8, excerpt_chars?, session_id?}`（长对话上下文压缩后恢复工作记忆首动） |
| `"memories"` | `{text?, kind?:"invariant"\|"constraint"\|"convention"\|"gotcha"\|"decision"\|"incident"\|"reference", symbol_id?, file_path?, tag?, source?, author?, scope?, min_importance?, pinned_only?, include_superseded?, since?, limit?:50}` |
| `"notes"` | `{text?, symbol_id?, file_path?, tag?, session_id?, pinned_only?, since?, limit?:50}` |
| `"notebook_list"` / `"notebook_find"` / `"notebook_show"` | `notebook_list`: `{limit?:50}`; `notebook_find`: `{query?, limit?:50}`; `notebook_show`: **必填 `{id}`** |
| `"onboarding"` | `{essential_kinds?, min_per_kind?, min_total?}` |

### 3. 记忆写入、笔记归档与风险确认 (`remember` — 8 操作)
| `operation` | `arguments: { ... }` 核心参数与输出 |
| :--- | :--- |
| `"memory"` | `{title?, body?, kind?, symbol_ids?, file_paths?, tags?, importance?:1..5, confidence?, scope?, source?, pinned?, id?, supersedes?, superseded_by?, no_autolink?}` **`[冷JSON]`** |
| `"edit_memory"` / `"rename_memory"` | `edit_memory`: **必填 `{id, pattern, replacement}`**, 可选 `mode?:"regex"\|"literal", scope?` **`[冷JSON]`**；`rename_memory`: **必填 `{id, to_scope}`**, 可选 `from_scope?` **`[冷JSON]`** |
| `"note"` / `"notebook"` / `"notebook_used"` | `note`: **必填 `{body}`**, 可选 `file_path?, symbol_id?, links?, tags?, pinned?, id?, no_autolink?` **`[冷JSON]`**；`notebook`: **必填 `{body}`**, 可选 `title?, tags?, id?` **`[冷JSON]`**；`notebook_used`: **必填 `{id}`** **`[冷JSON]`** |
| `"risk_ack"` | `{source?:"auto"\|"edit"\|"diff"\|"symbols"\|"ranges", symbols?, scope?, base?, path?, start_line?, end_line?, ranges?, workspace_edit?, lens?, risk_gate?, repo?}`（固定注入 `ack:true`） **`[冷JSON]`** |
| `"suppress_finding"` | `{action?:"add"\|"list"\|"remove", identity_key?, rule?, category?, symbol_id?, file?, source_line?, line?, reason?, author?}` **`[冷JSON+MB]`**（仅此项支持 `max_bytes`） |

---
## 九、 协同审查与 PR 雷达 (`review` [7] / `pr` [6] / `publish_review` [1])
> 全部 14 个操作均支持 `repo`（注意：`review` 为平铺门面，而 `pr` 与 `publish_review` 为冷门面须用 `arguments: {}`）。

1. **本地只读审查 (`review` — 平铺门面，7 操作)**：
   * `run`：`source:{scope?:"unstaged"\|"staged"\|"all"\|"compare", base?, diff?}`，`options?:{base_ref?, repo?, use_llm?, max_tokens?}`，**`[JSON+MB]`**；
   * `pack`：同 `run` + `options:{include_pack?, scrub?}`，**`[JSON+MB]`**；
   * `diff_context`：`source:{scope?}`，`options?:{base_ref?, repo?}`，`output?:{compact?}`（**无 `format`/`max_bytes`**）；
   * `pr_context`：`source:{scope?, base?}`，`options?:{base_ref?, ids?, repo?, sections?, verify?, audit_config?, edits?, session_id?, max_tokens?}`，**`[JSON+MB]`**；
   * `questions`：`source?:{base?}`，`options?:{ids?, path_prefix?, repo?, categories?, hub_threshold?, min_betweenness?, min_community_size?, limit?, max_tokens?}`，**`[JSON+MB]`**；
   * `sibling_context`：`source:{scope?, base?}`，`options?:{base_ref?, focus_file?, focus_files?, focus_symbol_id?, repo?, max_tokens?}`，**`[JSON+MB]`**；
   * `critique`：`source:{scope?, base?, diff?}`，`options?:{base_ref?, findings?, prior_review?, repo?, max_tokens?}`，**`[JSON+MB]`**。
2. **PR 雷达分析 (`pr` — 冷门面 `arguments`，6 操作，均 `[冷JSON+MB]`)**：
   * `list`：`{state?:"open"\|"closed"\|"all", limit?:30, prs?, repo?}`；
   * `impact`：**必填 `number`**，可选 `{files?, receipt?, scrub?, repo?}`；
   * `conflicts`：`{limit?:20, prs?, files?, repo?}`（**不存在 `pr_numbers` 字段**）；
   * `reviewers`：`{ids? | base? | number?, limit?:10, repo?}`；
   * `risk`：`{ids? | base?, repo?}`；
   * `triage`：`{limit?:20, prs?, files?, repo?, use_llm?, max_tokens?}`。
3. **对外发布评审 (`publish_review` — 冷门面 `arguments`，1 操作，`[冷JSON+MB]`)**：
   * `post`：**必填 `number`**，可选 `{findings?, summary?, scope?, base?, base_ref?, diff?, dry_run?:true, confirm_public?:false, public?, refuse_on_secret?:true, owner?, repo_name?, provider?:"github", repo?}`。
---
## 十、 工作区底层索引与时空富化 (`workspace_admin` 运维项，冷门面 `arguments`)
1. **索引重建红线**：仅在索引损坏、初次手动入库或大量外部构建变更未感知时使用：
   * `workspace_admin` `operation:"index"`, `arguments: {"path": "<仓库绝对路径>"}`（**注意：`index` 不支持 `format`！**）；
   * `workspace_admin` `operation:"reindex"`, `arguments: {"path?": "...", "paths?": ["..."], "format": "json"}` **`[冷JSON]`**。严禁日常开发频刷全量重建。
2. **Git / 覆盖率 / SQL / 时序富化分析**：
   * **4 个混合型操作（必填 `arguments: {}` + 顶层 `output`）**：`blame`、`coverage`（**必填 `arguments.profile`**，如 Go coverprofile 路径）、`sql_rebuild`、`temporal_verify`。`arguments` 内支持 `{repo?, project?, scope?, workspace?, profile?}`，顶层支持 `output: {"compact?": bool, "format": "json", "max_bytes": 8400}` **`[JSON+MB]`**；
   * **2 个冷 JSON 富化写盘**：`enrich_churn`、`enrich_releases`，`arguments: {"branch?": "...", "path?": "...", "format": "json"}` **`[冷JSON]`**。

```json
call_mcp_tool(ServerName="gortex", ToolName="workspace_admin", Arguments={"operation":"blame","arguments":{"repo":"edit-rust"},"output":{"format":"json","max_bytes":8400}})
call_mcp_tool(ServerName="gortex", ToolName="workspace_admin", Arguments={"operation":"coverage","arguments":{"profile":"coverage.out","repo":"edit-rust"},"output":{"format":"json","max_bytes":8400}})
```
---
## 十一、 缓冲提炼 (`response` [5])、会话治理 (`session` [8]) 与 MCP 原生 Resource / Prompt
1. **大输出缓冲提炼 (`response` — 冷门面 `arguments`，5 操作)**：
   * `grep`：**必填 `pattern`**, 可选 `response_id?, context?, before?, after?, ignore_case?:bool, literal?:bool, max_matches?`；
   * `slice`：`{from?: 1, to?, response_id?}`；
   * `peek`：`{lines?: 20, response_id?}`；
   * `stats`：`{}`（列出近期缓存的 `response_id` 与大小统计）；
   * `export_context`：**必填 `task`**, 可选 `entry_point?, max_symbols?:5, token_budget?:2000, format?:"markdown"\|"json"`（**仅此项支持 `format`**）。
2. **会话状态治理 (`session` — 8 操作，注意双容器形态)**：
   * **顶层 `channel` 型（禁把 `channel` 塞进 `arguments`）**：
     `subscribe` / `unsubscribe` → 顶层必填 `channel`（`"daemon_health"` | `"diagnostics"` | `"graph_invalidated"` | `"stale_refs"` | `"workspace_readiness"`），可选 `arguments:{}`、`output:{}`；
   * **`arguments` 封装型**：
     * `planning_mode`：`arguments:{"mode":"planning"|"editing"}`（切换只读规划态/可写编辑态）；
     * `cursor`：`arguments:{"action":"goto"|"into"|"up"|"sibling"|"back"|"where"|"read", id?, select?, format?:"json"|"toon"}`；
     * `agents`：`arguments:{"action?":"list"|"register"|"heartbeat"|"lock"|"unlock"|"unregister", id?, name?, paths?, cursor?, status?, format?:"json"}`；
     * `workflow`：`arguments:{"action":"start"|"advance"|"status"|"stop", mode?:"block"|"warn"}`；
     * `proxy_enable` / `proxy_disable`：**必填 `arguments:{"slug":"..."}`**。
3. **MCP 原生 Resource（13 个）与 Prompt（3 个）**：
   * **Resource URI 全表**：
     `gortex://workspace`、`gortex://repos`、`gortex://active-project`、`gortex://index-health`、`gortex://stats`、`gortex://schema`、`gortex://report`、`gortex://session`、`gortex://guide`（支持按章读取 `gortex://guide/{topic}`）、`gortex://audit`、`gortex://surprises`、`gortex://god-nodes`、`gortex://questions`。
   * **Prompt 列表**：
     `pre_commit`（`{"scope":"unstaged","repo?":"..."}`）、`safe_to_change`（**必填 `{"ids":"..."}`**）、`orientation`。

```json
call_mcp_tool(ServerName="gortex", ToolName="response", Arguments={"operation":"grep","arguments":{"pattern":"error","response_id":"resp-01","context":2}})
call_mcp_tool(ServerName="gortex", ToolName="session", Arguments={"operation":"planning_mode","arguments":{"mode":"editing"}})
call_mcp_tool(ServerName="gortex", ToolName="session", Arguments={"operation":"subscribe","channel":"diagnostics","arguments":{}})
```
---
# 【执行遵从性与 Antigravity 防闪退自查清单】 (Compliance Checklist)
1. [ ] **【容器与外壳自检】**：`call_mcp_tool` 内 10 个平铺门面（含 `analyze` 用 `kind`）是否未套多余 `arguments`？8 个冷门面是否均封装在 `Arguments.arguments` 内？
2. [ ] **【开局四步探测】**：是否已完成 `workspace.repos`/`info` + `active_project` + `index` 探测并匹配目标 `repo` 前缀？
3. [ ] **【`repo` 白名单与双通道零拼接】**：是否绝未向黑名单操作（`read.file`/`source`/`symbols`/`editing_context`、`edit.batch`/`file`/`symbol`/`write`、`refactor.rename`/`delete`、`change.tests`/`guards`/`impact`、`explore.task`/`context`/`localize`、`trace.path`/`flow`/`taint`/`cfg`/`graph`）乱传 `repo`/`workspace`？`options.path`/`path_prefix` 是否零 `repo/` 拼接？
4. [ ] **【`target` 单属性闭集】**：所有 `target` 是否严格仅含 1 个合法键（禁 `{}`、禁 `path`），且无 `target` 操作已完整省略 `target`？
5. [ ] **【精确 `output` 与切片硬锁】**：`[JSON+MB]` 设 `max_bytes:8400`（`winnow` 8250），`[JSON]` 绝不传 `max_bytes`，`read.file` 仅传 `output:{max_bytes:8400}` + `options:{offset, limit<=150, max_chars?}`（绝不传 `format`），写/状态操作绝不硬塞 `format`？
6. [ ] **【`edit` 防取消与安全落盘】**：新建文件是否已用 `edit.write`（不用 `edit.batch`）？改大函数局部是否优先用了 `edit.symbol` 片段替换？超大规模修改是否已按内聚模块合理控制单批体量？遭遇闪断/失配是否先查 `change.receipt` + `change.detect` + `read.file` 后再提交？
7. [ ] **【核验闭环与防溢出】**：源码改后是否完成 `change.detect` → `change.diagnostics` → `change.tests` 并执行原生测试？若出现 `output.txt` 或大缓冲句柄，是否只用 `response` 提炼而不直接原生读取？
8. [ ] **【首错即查 `capabilities`】**：已知操作是否直调免预检？一旦报任何参数错误是否立即停猜并调用一次 `capabilities(domain, operation, detail:"schema")` 纠偏且同会话不重复查询？
