# Gortex 统一架构全景全生命周期规范契约 (v704 · 批量优先编辑路由与分级降级版)

> 对齐 **Gortex `v0.64.11` (`facade-v1`)** 全量能力：共 **21 个门面、248 个标准子操作（含 `analyze` 96 个 `kind`）+ `capabilities` 自省门面 + 条件门面 `ask`（未配置研究提供方时 `operations: []`），以及 13 个 MCP Resource 与 3 个 MCP Prompt**。
> 本规范整合 **L0 ~ L3 全量命令参数契约与零闪断防线**，确立以 **`edit.batch` 为多处修改最高优先级**（下限 `2` 项、推荐 `2~8` 项、单次硬上限 `10` 项且建议 `< 4~5 KB`，超量按批串行拆分）、**`edit.file(match/replacement)` 为单次修改优选**（独享 `checkParseGate` 语法门禁、`guard` 与 `base_sha`），以及**“连续 3 错降批、5 错强制切 `match`”的两级熔断降级机制**。

---

## 【调用外壳、容器归属、通用控制字段与排版语法】

### 1. Antigravity Lazy MCP 调用外壳与三类容器隔离（严禁混写）
在 Antigravity 中，Gortex 的 21 个门面默认以 Lazy MCP 工具形式挂载，标准调用格式为：
`call_mcp_tool(ServerName="gortex", ToolName="<门面名>", Arguments={...})`
*(若当前环境已将工具展开为直连工具，则直接调用 `mcp_gortex_<门面名>`，其入参与下述 **外层 `Arguments` 对象** 等价。)*

必须绝对分清 **“外层 `Arguments`（Antigravity 工具调用壳）”** 与 **“内层 `"arguments"` 键（仅冷门面使用）”**：

1. **普通平铺门面（10 个，9+1）**：
   * `explore`、`search`、`read`、`edit`、`refactor`、`change`、`relations`、`trace`、`review`（共 9 个）：外层 `Arguments` 顶层直接平铺 `operation`、`query`/`task`/`target`/`to`/`source`/`context`/`guard`/`changes`/`match`/`replacement`/`content`/`new_name`/`destination`/`dry_run`、`options`、`output`。
   * **`analyze`（1 个）**：外层 `Arguments` 顶层固定平铺 **`kind`**（**严禁写成 `operation`！**），搭配可选 `target`、`options`、`output`。
   * **绝对禁令**：以上 10 个平铺门面**严禁**在外层 `Arguments` 内再多包一层 `"arguments": {...}` 或 `"params": {...}`！
2. **冷门面（8 个，内层 `"arguments"` 封装型）**：
   * `workspace`、`workspace_admin`、`overlay`、`remember`、`recall`、`response`、`pr`、`publish_review`：
   * 所有业务参数（及除下述 4 个混合特例外的所有输出参数）**必须**封装在外层 `Arguments` 内部的 **`"arguments": {...}`** 对象中，与 `"operation"` 同级；无业务参数时也推荐显式传 `"arguments": {}`（其中 `workspace_admin` 的 `index`/`track`/`untrack`/`save_scope`/`delete_scope`/`set_active_project`/`feedback`/`coverage`/`sql_rebuild`/`temporal_verify` 与 `overlay` 的 `fork`/`switch`/`drop_branch`/`push`/`delete`/`merge`/`simulate`、`response.export_context`、`pr.impact`、`publish_review.post` 等均为 Schema 必填 `"arguments"`）。
   * **4 个混合型特例**：`workspace_admin` 的 `blame`、`coverage`、`sql_rebuild`、`temporal_verify` 除必须传内层 `"arguments": {...}`（`blame`/`sql_rebuild`/`temporal_verify` 无过滤参时传 `{}`，**`coverage` 必填 `profile`**）外，其 `compact`/`format`/`max_bytes` 需放在**外层 `Arguments` 顶层的 `"output": {...}`** 中。
3. **会话与自省门面（3 个）**：
   * **`session`（特别注意：8 个操作均不声明顶层 `output`！）**：
     * `subscribe` / `unsubscribe`：`channel` 放在**外层 `Arguments` 顶层**（与 `operation` 同级），可选 `"arguments": {}`；**不要传顶层 `output`**。
     * 其余 6 个操作（`agents` / `cursor` / `planning_mode` / `proxy_enable` / `proxy_disable` / `workflow`）：顶层仅允许 `{operation, arguments, view}`（`additionalProperties: false`，**严禁传 `require_exact`/`require_fresh`/`wait_deadline`**），所有参数放在**内层 `"arguments": {...}`** 中，**严禁传顶层 `output`**（`cursor`/`agents` 如需 `format` 须写在 `"arguments"` 内）！
   * **`capabilities`**：外层 `Arguments` 顶层平铺 `domain`（可选；全库总览时可省略）、`operation?`、`detail`（**合法值为 `"summary"` 或 `"schema"`**；禁传 `"operations"`）。
   * **`ask`**：外层 `Arguments` 顶层平铺 `question`（必填）、`options`、`output`。当前未配置研究提供方时 `capabilities` 返回 `operations: []`，切勿盲调。

### 2. 跨门面通用视角与实时性控制（Request-Level Knobs）
除 `ask` / `capabilities`（以及仅顶层支持 `view` 的 `session` 6 项）外，其余门面均在外层 `Arguments` 顶层支持以下可选视角与一致性控制字段（冷门面同样放外层顶层，勿塞进内层 `arguments` 或 `options`）：
1. **`view`（工作树 / Git 版本视角）**：省略或 `{"kind":"auto"}` 表示按当前会话工作区解析；支持其余 5 种精确模式：
   * `{"kind":"base","graph_id":"<图ID>"}`
   * `{"kind":"worktree","checkout_id":"<checkout_id>"}`（ID 来自 `workspace.checkouts`，与下述 `path` 模式二选一互斥）
   * `{"kind":"worktree","path":"<工作树根目录绝对路径>"}`
   * `{"kind":"git_ref","value":"refs/heads/main","graph_id?":"..."}`（**`value` 必须为完整 ref，如 `refs/heads/...`，禁简写 `"main"`**）
   * `{"kind":"commit","value":"<40位小写Git SHA>","graph_id?":"..."}`（**必须为完整 40 位小写哈希，短 SHA 会报错**）
2. **新鲜度控制（一般省略；`session` 6 项禁传）**：`require_exact?: bool`、`require_fresh?: bool`、`wait_deadline?: string`。

### 3. `target` 严格单属性闭集契约（防 Schema 校验闪退）
在 `read`、`edit`、`change`、`refactor`、`relations`、`trace`、`analyze` 等带 `target` 的平铺门面中，`capabilities` 静态 Schema 对 `target` 强制约束为 **`minProperties: 1, maxProperties: 1, additionalProperties: false`**，且仅允许各自声明的属性之一（全库 Schema 闭集为：`file` | `symbol` | `symbols` | `query` | `artifact` | `repo`；日常绝大多数仓库限定请走下表允许的 `options.repo`，勿擅自向未声明操作传 `target.repo`）：
* **严禁**传空对象 `"target": {}`；
* **严禁**写成 `"target": {"path": "..."}`（文件路径必须用 `file`，且仅在对应操作允许 `file` 时使用）；
* **严禁**同时传多个键（如 `{"file": "...", "symbol": "..."}`）；
* 不使用 `target` 的操作（如 `edit.batch`/`apply_overlay`/`docs`/`export_graph`/`wiki`/`skill`、`change.detect`/`preview`/`simulate`/`diagnostics`/`code_actions`/`receipt`/`api_impact`/`pattern`/`ranges`/`verify`/`overlay_branches`/`overlay_state`、`trace.graph`、改用 `options.path` 时的 `read.artifact`、无符号时的 `read.history`/`relations.references`、以及 **`analyze` 中除 `co_change`/`why`/`def_use`/`impact` 外的其余 92 个 `kind`**）**必须完全省略 `target`**。

### 4. 排版符号与精确响应契约约定
1. **排版符号**：`"|"` 表示候选多选一；`"~"` 表示闭合整数区间（如 `limit: 25~50`）；`key?: <值>` 表示可选参数及服务端默认值或推荐取值（如 `regexp?: false`、`limit?: 35~80`；写操作中的 `dry_run?: false` 若非已明确默认 `true` 的 `edit.scaffold`，为防未确认默认值导致仅演练未落盘，实写时均建议显式传入 `"dry_run": false`）；**`服务端固定 <k>:<v>`** 表示服务端内部锁定或自动注入该行为，**调用方无需且勿手动重复传入**。**实际传参时严禁原样照抄 `"|"` 或 `"~"`**。
2. **输出预算标记说明（对齐 `capabilities` Schema + 防体积溢出）**：
   * 说明：在 `facade-v1` 运行时中，门面的顶层 `tools/list` 虽放行通用造型键，但**严格按各操作的 `capabilities` Schema 对齐 `output`** 是避免严格客户端/代理校验拒单与参数无效空转的最佳实践，同时真正会立即触发 Gortex 服务端硬拒报错的另有铁律 2/3/5/6 中的**17 个伪仓选键、必填缺失、状态锁与非法类型**。
   * **`[JSON+MB]`**：外层 `Arguments.output` 传 `{ "format": "json", "max_bytes": 8400 }`（`search.winnow` 为 `8250`）；部分支持 `compact` / `fields` / `limit` / `offset` / `summary_only`（见各节表格）。
   * **`[JSON]`**：外层 `Arguments.output` **仅**传 `{ "format": "json" }`（该操作在 `capabilities` 中未声明 `max_bytes`，请勿附带 `max_bytes`）。
   * **`[冷JSON+MB]`**：冷门面内层 `"arguments"` 中传 `"format": "json", "max_bytes": 8400`。
   * **`[冷JSON]`**：冷门面内层 `"arguments"` 中**仅**传 `"format": "json"`（不传 `max_bytes`）。
   * **特殊/无 `format` 接口**：`read.file` **仅声明 `output: { "max_bytes": 8400 }`（勿传 `format`）**；`edit.batch` 默认必传 **`output: { "compact": true }`**（将双数组回执压成逐行摘要，防返回截断；仅排查逐项失败明细时可省略）；`session` 8 个操作及未标 `format` 的多数写操作勿塞入 `output` 或 `format`。

---

# 【L0 级：全局统领宪法与防闪断底线】（最高强制位阶 · 每次必读）

### 铁律 1：外部文档只读豁免、Gortex 闭环与原生跳出边界
1. 位于代码仓库外部系统目录的规范文件（如外部提示词或跨盘文档）属于初始环境法定豁免文件，**必须且仅能使用宿主原生只读工具（`read_file` 等）按规范分片读取**，严禁调 Gortex 读取外部绝对路径。
2. 预读门禁完成后，受 Gortex 纳管工程内所有源码、测试代码（含 `#[cfg(test)]` 与独立测试文件）、配置文件与维护脚本的探测、检索、阅读、新建、修改、验证与审查，**100% 走 Gortex MCP 闭环**。
3. 仅两类情况允许宿主原生工具：① 用户输入明确包含“使用原生工具”指令时单次跳出，完成后立即返回闭环；② **Git / GitHub 运维、项目编译/类型检查/构建、只读格式检查（如 `cargo fmt --check`/`prettier --check`）与实际运行/调试测试进程**（`git`、`cargo check`/`test`、`tsc`、`go build`/`test`、`pytest`、`npm run build`/`test`、调试器断点查看崩溃栈）可直接用原生命令，但**严禁借构建、格式、测试或调试之名，用原生工具检索、阅读或修改工程内受维护的源码/测试/配置文件**。

### 铁律 2：双通道路径零拼接与 17 个仓库/根目录伪选择器严管矩阵（防 `search`/`read` 闪退核心）
#### A. 双通道路径规则
* **通道 A（全局定位寻址器：`target.file` / `target.symbol` / `source.file` / `changes[].path` / `changes[].source` / `changes[].destination` / `changes[].symbol_id` / `arguments.path` 及 `explore.closure` 的 `options.files`/`symbols`、`remember`/`recall` 的 `file_paths`/`symbol_ids`）**：
  用于 `read.*`、`edit.*`、`refactor.*`、`relations.*`、`trace.*`、`change.*`、`analyze.co_change`/`why`/`def_use` 等。所有路径与符号 ID 原样复制自 `workspace` / `search` / `explore` 返回值（下文 `<repo>` / `my-app` 均为占位符，实际必须替换为 `workspace.repos` 返回的真实仓名）；**多仓库及 `unbound_global` 环境下，必须保留前导 `<repo>/` 前缀**（如 `"my-app/src/main.rs::main"`），**裸相对路径会在同名多仓冲突或未命中前缀时直接报错退出！**
* **通道 B（仓内子路径前缀过滤器：`search` / `explore.context` 的 `options.path` 与 `options.path_prefix`）**：
  `search.text`/`symbols`/`files` 与 `explore.context` 用 `options.path`（逗号分隔仓内相对前缀），`search.winnow`/`ast` 及 `analyze` 相关分析用 `options.path_prefix`。**这类前缀过滤器严格按 `repoRelativePath(n)` 匹配——严禁在 `path` 或 `path_prefix` 中拼接 `<repo>/` 前缀**（拼了 `<repo>/` 会搜空或失配！）；仓库限定必须单独通过下表允许的 `options.repo` 传入。
  *(注：`analyze.lint`、`read.artifact`、`relations.import_path` 等**单文件定位型** `options.path` 并非子路径前缀过滤器——其中 `analyze.lint` 示例带 `<repo>/` 前缀；多仓下若遇单文件 `options.path` 定位报错，以 `capabilities` 或运行时报错纠偏。)*

#### B. `repo` 等 17 个伪选择器禁用与严格白名单矩阵
> **最高频闪消根因警示 (`facade_tools.go:facadeRepositorySelectorLike`)**：Gortex 会在校验层拦截 `options` 中的以下 **17 个仓库/目录别名键**：
> **`repo`、`repo_path`、`repository`、`repository_path`、`repo_root`、`repository_root`、`repopath`、`repo-path`、`repo_dir`、`root`、`cwd`、`dir`、`worktree`、`work_tree`、`base_repo`、`workspace`、`project`**！
> （同时保存域键 `scope` 也有严格白名单。）**除 `analyze.agent_config` 允许 `options.root` 覆盖外**，向任何**未在下表显式允许**的操作传入这 17 个键中的任意一个，都会在校验期**立即抛出 `invalid_argument` 并在 UI 上闪一下消失！**

| 门面 | **允许**传 `repo` / `project` / `workspace` / `scope` / `ref` 的操作及确切键名 | **绝对禁止**传上述 17 个仓库/根目录键及 `scope` 的操作 |
| :--- | :--- | :--- |
| **`read`** | **仅 `read.summary`** 允许 `{repo, project, scope, workspace, ref}`（多仓查非当前 `active_project` 时 **`read.summary` 必传 `options.repo`**） | `read.file`、`read.source`、`read.symbols`、`read.editing_context`、`read.history`、`read.artifact` **全部严禁传 `repo`/`root`/`cwd`/`workspace`/`project`/`scope` 等！**（直接用带 `<repo>/` 前缀的 `target` 寻址） |
| **`search`** | `text`、`symbols`、`files` 允许 `{repo, project, scope, workspace}`（`symbols` 另支持 `ref`）；`winnow`、`ast` **仅允许 `{repo, project, ref}`**（**禁 `workspace`/`scope`!**） | `search.artifacts`、`search.completion` **严禁传上述任何键** |
| **`explore`** | `context` 允许 `{project, scope}`（**无 `repo`/`workspace`**）；`closure` 允许 `{project, ref, scope, workspace}`（**无 `repo`**） | `task`、`localize`、`outline`、`wakeup`、`plan`、`prefetch`、`suggest` **均严禁在 `options` 传 `repo`/`root`/`cwd`/`project`/`scope`/`workspace`** |
| **`edit`** | `docs` 允许 `{workspace}`；`export_graph` 允许 `{repo, scope}`；`wiki` 允许 `{repo, project, workspace}` | `batch`、`symbol`、`file`、`write`、`apply_overlay`、`scaffold`、`skill` **全部严禁传 `repo`/`root`/`cwd`/`workspace`/`project`/`scope`** |
| **`refactor`** | **仅 `fix_all`、`apply_code_action`** 允许 `options.repo` | `rename`、`delete`、`move`、`inline` **全部严禁传 `repo` 等** |
| **`change`** | 仅 `detect`(`repo`)、`diagnostics`(`repo, project`)、`code_actions`(`repo, project`)、`api_impact`(`repo, project, ref`)、`contract`(`repo`) | **其余全部 14 个操作**（`tests`/`guards`/`impact`/`edit_plan`/`pattern`/`ranges`/`verify`/`receipt`/`preview`/`simulate`/`overlay_*`/`compare_*`）**均严禁传 `repo` 等** |
| **`relations`** | `callers`、`dependencies`、`usages`、`implementations`、`overrides`、`hierarchy`、`dependents`、`cluster` 均允许 `{repo, project, scope, workspace, ref}` | `references`、`declaration`、`import_path` **严禁传 `repo` 等** |
| **`trace`** | **仅 `call_chain`、`walk`** 允许 `{repo, project, scope, workspace, ref}` | `path`、`flow`、`taint`、`cfg`、`graph` **均严禁传 `repo` 等** |
| **`analyze`** | 多数分析支持 `{repo, project, scope, workspace}`（**⚠️ `cycles` 的 `scope` 也是 `saved-scope slug`，不是目录前缀！**）；`contracts` 仅允许 `{repo, project, ref}`（无 `scope`/`workspace`）；`why` 仅允许记忆域 `scope`（`"workspace"\|"global"`，无 `repo`/`project`/`workspace`）；`cross_repo` 的 `repo` 作跨仓边界过滤 | **`architecture`、`coupling`、`churn`、`co_change`、`untested`、`extraction`、`knowledge_gaps`、`surprising_connections`、`agent_config`（仅允许 `root`）、`inspection_catalog`、`replay`、`lint`、`citation`、`help`** 均**严禁传 `repo`/`project`/`scope`/`workspace`!** |
| **`workspace_admin`** | **仅 `blame`、`coverage`、`sql_rebuild`、`temporal_verify`** 允许 `{repo, project, scope, workspace}` | `feedback`、`index`、`reindex`、`enrich_*`、`track`/`untrack`、`save_scope`/`delete_scope`、`set_active_project` **严禁乱传 `repo`/`root`/`cwd`**（`save_scope` 专用逗号串 `repos`） |
| **其余门面** | `review.*`、`pr.*`、`publish_review.post` 全量允许 `repo`（放平铺 `options` 或冷 `arguments`）；`remember.risk_ack` 允许 `repo` | `overlay.*`、`response.*`、`recall.*`、`remember.*`（除 `risk_ack`；`recall` 记忆域 `scope` 为 `"workspace"\|"global"\|"both"`，`remember` 为 `"workspace"\|"global"`）均禁传仓库 `repo` |

### 铁律 3：开局精简前置探测与零空转准入 (Probe-Before-Analyze)
接入新任务、跨仓排查或首次分析未知工程前，先完成环境确认（同会话已确认且工作区未变时直接复用）：
1. **一跳全局探测**：调 `workspace`（`Arguments={"operation":"repos","arguments":{"format":"json","max_bytes":8400}}`）获取项目归属、已纳管仓清单、`members[].path` 本地路径映射、`members[].state`（`"ready"` 等）及 `unbound_global` 模式；需要验证工作区标记详情时再用 `"info"`。（支持 MCP Resource 的宿主亦可直接读 `gortex://repos` / `gortex://workspace` / `gortex://index-health`。）
2. **按需二次确认**：**仅当**出现 `unbound_global`、当前工作目录需要对齐多仓归属、或 `members[].state != "ready"` 怀疑索引陈旧/解析报错时，再按需补调 `workspace` 的 `"active_project"` 或 `"index"`，避免每次任务机械三连串查浪费轮次。
3. **准入止损**：严禁未经探测盲搜，严禁臆测“未索引”退回宿主工具；若 `repos`/`members` 均为空或目标仓未纳管，仅确认一次并按需 `workspace_admin.track` 或如实报告，严禁死循环重试；若目标路径属于被索引排除的构建产物（如 `target/`、`dist/`）或第三方依赖缓存（如 `.cargo/`、`node_modules/`），不得死循环检索，应直接说明未纳管状态（测试/构建进程本身输出的标准错误与报告可按铁律 1 直接查看）。

### 铁律 4：多项目双穿透与主工程切域法则
1. **只读轻量穿透（免切项目）**：跨仓查符号用 `Call: workspace`，`Arguments={"operation":"project","arguments":{"project":"<目标工程>","query":"...","limit":20}}`；跨仓精读直接用带 `<repo>/` 前缀的 `read.file` / `read.symbols` / `read.source`。
2. **深度操作切域**：对非默认 `active_project` 的外部仓执行依赖主工程上下文的重构、测试图分析或批量改动前，先调 `workspace_admin`，`Arguments={"operation":"set_active_project","arguments":{"project":"<目标工程>"}}`（值为 `project` 名，非路径）。若仅并发查单个外部仓，按铁律 2 白名单显式传 `options.repo` 即可。

### 铁律 5：强类型字段、`capabilities` 对齐与读取切片硬锁
1. **显式动词头**：外层 `Arguments` 必传 `"operation"`（`analyze` 必传 `"kind"`），严禁无头请求。
2. **按标记匹配 `output`（勿把 `max_bytes` 塞进 `options`！）**：
   * 标 **`[JSON+MB]`**：`output: {"format":"json","max_bytes":8400}`（`search.winnow` 用 `8250`）；
   * 标 **`[JSON]`**：`output: {"format":"json"}`（按 `capabilities` Schema 不带 `max_bytes`）；
   * **`read.file`**：**仅传 `output: {"max_bytes":8400}`，严禁传 `format`**；
   * **冷门面**：`format`/`max_bytes` 放在内层 `"arguments"` 内（仅 `workspace_admin` 4 项混合分析放顶层 `output`）；
   * **`session`（8 项全禁顶层 `output`）**：`planning_mode`/`workflow`/`proxy_*`/`subscribe`/`unsubscribe` 全不传 `output`；`cursor`/`agents` 如需 `format` 放在 `"arguments"` 内。
3. **强制分片与单串防溢出硬锁**：
   * `read.file` **严禁不传 `offset`+`limit` 盲读整文件**！必传 `options: {"offset": 1基行号, "limit": <=150}`（推荐 `40~100`，小文件亦须显式传 `offset:1, limit:150`）+ `output: {"max_bytes":8400}`；
   * **防单字符串超长落盘**：Gortex 不按 `output.max_bytes` 截断 `read.file` 的单个 `content` 字符串；遇到长行、高密度代码或含大量中文的文件时，收紧 `limit` 到 `40~80` 并配 **`options.max_chars`**（如 `8000~14000`）或 `context: {"compress_bodies": true, "max_lines": 120}`。
   * `explore` 优先控制 `options.token_budget: 1600~4000`（如 `context` 默认上限为 `8000` 时亦建议收紧至该区间）。

### 铁律 6：批量最高优先编辑路由、单次 `match` 优选与分级降级熔断（v704 核心改版）
> **核心策略原则（基于 Gortex `v0.64.11` 服务端特性）**：
> 1. **`edit.batch`（批量修改）是修改代码的最高优先级默认命令**：服务端具备内存多改叠加（同文件多处改可自动连续合并）、Journal 持久日志、失败全回滚与 `EnqueueFileMutations` 批量刷新图谱能力；配合 `output: {"compact": true}` 后响应极简。**凡存在 `>= 2` 项独立编辑操作时，优先合并为 `edit.batch` 一次性提交，禁止无故拆成一排零碎单发调用。**
> 2. **`match`/`replacement`（`edit.file` / `edit.symbol`）是单次修改的最高优先级首选，也是批量连续出错后的强制兜底通道**：`edit.file` 独享服务端 **`checkParseGate` 语法门禁**、`guard.expected_occurrences` 计数守卫、`base_sha` 防漂移与真实 `dry_run` diff 预览，单点修改综合优势最高。由于 `edit.batch` 底层未接入 `checkParseGate`，**批量改完源码后必须补齐语法核验（有 LSP 时调 `change.diagnostics`，无 LSP 时以原生构建兜底；纯文档/配置免跑）**。

1. **AI 自主编辑决策与路由优先级（必须严格按此判断）**：
   * **优先级 1（最高优先 · 多处修改默认路径）：`edit.batch`（使用 `old_string`/`new_string` 或 `old_source`/`new_source`）**
     * **触发条件**：当前需要执行 **2 项及以上独立编辑操作**（无论是跨多个文件、还是同一个文件内 2 处及以上不同位置的局部修改、或混合 `edit_file`/`edit_symbol`/`move_file`/`delete_file`）。
     * **数量与体积硬契约（2 ~ 8 项推荐，单次硬上限 10 项）**：
       * **下限 `2` 项**：仅 `1` 项编辑操作禁止走 `edit.batch`，直接进入下述优先级 2；
       * **常规规模 `2 ~ 8` 项**：日常默认按每批 `2 ~ 8` 项组织，估算单批 `changes` 文本总长建议 **`< 4~5 KB`**，单条片段保持最小唯一锚点（1~5 行，严禁整文件搬运）；
       * **单次硬上限 `10` 项与串行拆批**：单次 `edit.batch` 的 `changes[]` **绝对不得超过 `10` 项**。仅当总改动 **`> 10` 项**（或 `9~10` 项但文本总长显著超 `4~5 KB`）时，才必须拆分为多批**按批串行**执行（例如 14 处改动先提交 `8` 项，待完成后再发下一批 `6` 项；**严禁同轮并发多个 `edit.batch`**）。
     * **默认必带精简回执**：`edit.batch` 默认必传 **`output: {"compact": true}`**（仅排查逐项失败明细时可省略，并可配 `options: {"transaction_id": "..."}` 支持幂等续查）。
   * **优先级 2（单次修改最高首选）：`edit.file` / `edit.symbol`（优先使用 `match` + `replacement`）**
     * **触发条件**：① 当前仅需修改 **1 处**局部代码/配置；② 单文件仅需 **1 组**同模式相同字面量的统一替换（`edit.file` + `options: {"replace_all": true}`；若已明确精确匹配次数建议加 `guard: {"expected_occurrences": N}`，次数未知时可省略 `guard`；若跨多文件各自 `replace_all` 或同文件还有其他局部修改，则合并进 `edit.batch`）；③ 高风险单点改动，明确需要 `checkParseGate` 语法拦截、`expected_occurrences` 严格计数、`base_sha` 校验或 `dry_run` diff 预览；④ **`edit.batch` 触发第 5 次连续失败（或小批提前降级）后的强制兜底**。
     * **优先 `edit.file(match/replacement)`**：`edit.file` 拥有 `checkParseGate` 与 `guard`，综合安全优势最高；仅在需要严格限定到单一函数/符号作用域且无需 `guard`/`allow_parse_errors`/`replace_all` 时使用 `edit.symbol(match/replacement)`。**单次修改一律优先用 `match`/`replacement`，不为了用 `old_`/`new_` 而对 1 处修改强开 `edit.batch`。**
   * **专用直达通道（`edit.batch` 无法替代的语义/建文件场景）**：
     * **新建文件或必须整文件覆写** → **必用 `edit.write`**（`edit.batch` 内 `edit_file` 对不存在文件会立即失败并回滚整批！新建多个**互不依赖**的文件允许同轮并发多个 `edit.write`；若任务需“新建 1 个文件 + 修改 3 个旧文件”，旧文件不依赖新文件时可按序或分开落盘，若存在依赖或需改新建文件则先 `edit.write` 再执行后续修改）。
     * **纯跨文件/跨引用重命名符号** → 优先 **`refactor.rename`**（仅传 1 个 `new_name` 即可完成全仓拓扑重命名；若除重命名外还伴随多处逻辑体重写，**严禁同轮并发**，必须先执行 `refactor.rename` 落盘，再针对最新源码/符号基线执行 `edit.batch`）。

2. **批量出错的两级强制降级熔断机制（严禁无限死磕同一批）**：
   同一子任务连续推进期间，统计 `edit.batch` 的**连续失败次数**（任意一次 `edit.batch` 成功落盘后失败计数清零，但**已进入一级降级的当前拆批序列应保持 `2~4` 项稳步收口，严禁反弹回 `8~10` 项**；`tool_blocked_by_mode` 与参数校验错误 `invalid_argument` 不计入本计数——后者先按铁律 8 查 `capabilities` 一次纠偏），按连续失败次数严格执行：
   * **第 1 ~ 2 次连续出错（常规对齐修复）**：
     **绝对禁止原样重放！** 按下方退避规则查明失配原因后，修正 `old_string`/`old_source` 重试批量。
   * **到达第 3 次连续出错（含第 3、4 次）→ 一级降级：必须缩减批量规模！**
     **严禁继续按原规模重试！** 必须立即将单批 `changes[]` 条数**降档减半**（原 `6~10` 项降为每批 **`2~4` 项**；原 `4~5` 项降为每批 **`2~3` 项**；**若当前批原本已处于 `2~3` 项最低档仍第 3 次连续出错，直接跳过一级降级，提前切入下述二级 `match` 通道**），切断连坐回滚范围。
   * **到达第 5 次连续出错（`>= 5` 次）→ 二级硬降级：必须降级到 `match` 修改！**
     **立即终止使用 `edit.batch`！** 当前子任务剩余所有未落盘修改**必须强制全部切换为 `edit.file` / `edit.symbol`（`match` + `replacement`）**：**仅跨不同文件时允许轻量并发，同一个文件内的多处修改必须逐项串行依次执行**（防止同文件写入竞态或 `base_sha` 漂移），借助 `checkParseGate` 与 `guard: {"expected_occurrences": 1}` 稳态收口，严禁继续强试 `edit.batch`。

3. **`edit.batch` 字段严防串键（零报错关键）**：
   `edit.batch.changes[]` 在 `tools/list` 下为开放对象（无字段约束），一旦混键会被 `classifyBatchEditItem` 整批拒绝！牢记：
   * **`edit.batch` 的 `changes[]` 内**：`edit_file` 用 **`old_string` / `new_string`**（+`path`）；`edit_symbol` 用 **`old_source` / `new_source`**（+`id`）；`move_file`/`delete_file` 必填 `op`；**绝不允许出现 `match`、`replacement` 或 `target`！**
   * **单发 `edit.file` / `edit.symbol`**：顶层必用 **`match` / `replacement`** + `target: {file|symbol}`；**绝不允许出现 `old_string` / `old_source`！**

4. **写拦截模式锁 (`tool_blocked_by_mode`) 防连闪恢复**：
   * 若 `edit.*` / `refactor.*` 返回 `tool_blocked_by_mode`（处于只读规划态或工作流阶段拦截），**严禁连续重试写操作（也不计入上述批量降级次数）！** 若当前任务确实允许改码，先调 `session(operation="planning_mode", arguments={"mode":"editing"})` 解锁（或检查 `session.workflow` 阶段），然后再执行写入；若当前处于明确只读/只分析阶段，立即停写并报告。

5. **写操作幂等性与故障分流退避**：
   * 批改可带 `options.transaction_id`（单发可带 `options.mutation_id`），重发同 Key + 同内容可幂等回放；单发高风险修改可带 `options.base_sha` 防漂移或先 `dry_run: true`（注：`edit.batch` 的 `dry_run: true` 仅校验路径规划与生命周期冲突，**不校验** `old_string` 是否匹配文件内容）。
   * 发生错误时**绝对禁止直接原样重放**，按错误类型分流：
     * **若明确返回 `ambiguous_match` 或未找到匹配**：`edit.batch` 已全回滚未落盘，直接跳过查状态，调 **`read.file(offset, limit <= 150)`** 或 `read.symbols` 对齐当前磁盘基线，修正最小唯一 `old_string`/`match`（或按连续失败次数降级）后重提；
     * **若发生流式中断 / 超时等不确定是否已落盘的情况**：必须走完整三步退避——① 若传过 `transaction_id`，先调 `edit`（`operation:"batch"`, **完全省略 `changes`**，`options:{"transaction_id":"...", "status_only":true}`；切勿带空 `changes:[]`）；若传过 `mutation_id` 则查 `change.receipt(options:{mutation_id|receipt})`；同时调 **`change.detect`** 确认磁盘真实状态；② 仅对未落盘或失配片段，调 `read.file` / `read.symbols` 重新对齐基线；③ 修正（或执行 **3 错降批 / 5 错切 `match`**）后再提交。

### 铁律 7：大输出防溢出隔离守卫 (Anti-Spill)
1. 触发截断落盘提示（如保存到 `output.txt`）或命中 Gortex 缓冲句柄时，**绝对严禁调用任何宿主原生只读工具（`read_file` / `cat` / `Get-Content`）直接读取该内部 `output.txt`**！
2. 必须通过收紧 `max_bytes`（平铺门面放在 `output.max_bytes: 8400`，`search.winnow` 为 `8250`；冷门面放在 `arguments.max_bytes: 8400`）、`read.file` 降低 `limit`（40~80）与设 `options.max_chars`、开启 `compress_bodies: true` / `output.compact: true` 重取；若持有 `response_id`，仅限用 **`response(operation="grep"|"slice"|"peek"|"stats", arguments={...})`** 提炼摘要。

### 铁律 8：直调优先、本地 Schema 禁令与 `capabilities` 首错即查防连闪红线
1. **禁用本地 Schema 文件**：严禁使用宿主原生工具读取任何 `mcp/gortex/*.json`。
2. **已知接口严禁事前空转 `capabilities`**：凡本文档已给出参数契约的操作直接按契约直调，禁止日常操作前例行预检。
3. **“首错即查”防连闪兜底边界（防连续两次闪消的关键！）**：
   * **场景 A（报错即查，严禁凭猜测二次同参/盲猜重试！）**：一旦任意命令返回参数校验错误（`invalid_argument` / 缺必填项 / 容器错位 / 未知字段），**立即调用一次 `capabilities`**：`{"domain":"<门面>","operation":"<操作>","detail":"schema"}`，严格对照返回的 `input_schema` 与 `request_shape` 一次性纠偏；
   * **场景 B（未收录盲区）**：需使用未收录的新能力或核对操作清单时，先调 `{"domain":"<门面>","detail":"summary"}` 或加 `"operation"` 查 `"schema"`；
   * **场景 C（同会话去重）**：同一个 `domain + operation` 已查过一次 `detail:"schema"` 后，**本会话内严禁重复再查**。

### 铁律 9：默认按文档执行与冲突裁决五大位阶
未发生真实报错或行为冲突时，默认直接以本文档为准执行；仅在出现分歧时自顶向下裁决：**1. 工具真实运行时响应**（报错 / guard / completion / `not_modified` / `recoverable`）> **2. 触发的 `capabilities(detail:"schema")` 即时 Schema** > **3. 磁盘物理文件与 Git 树真实状态** > **4. Gortex 服务端源码实现**（若对应仓库已纳管）> **5. 本文档静默规则**。

---

# 【L1 级：日常开发与多项目协同黄金闭环】（高频 · 协-查-读-写-验五大核心中枢）

## 零、 工作区与多项目协同中枢 (`workspace` [9] / `workspace_admin` [14])
> **容器铁律**：此 2 个门面均为**冷门面**，外层 `Arguments` 内必须传 `"operation"` 与内层 `"arguments": {...}`。仅 `workspace_admin` 后 4 个混合分析（`blame` / `coverage` / `sql_rebuild` / `temporal_verify`）可在外层 `Arguments` 顶层另附 `output`。

### 1. `workspace` 只读与导航查询（9 个全收录）
| 触发场景 | `operation` | 内层 `arguments: { ... }` 参数与精确格式契约 |
| :--- | :--- | :--- |
| 列已纳管项目/仓库、绑定模式、本地路径及 `state` | `"repos"` \| `"info"` | `{"format":"json","max_bytes":8400}` **`[冷JSON+MB]`**（严禁传 `repo`） |
| 查当前会话主工程及包含成员列表 | `"active_project"` | `{"format":"json","max_bytes":8400}` **`[冷JSON+MB]`** |
| 查索引健康、解析失败数、陈旧文件与语言统计 / 图谱汇总 | `"index"` \| `"graph"` | `index`: `{"compact?":true,"format":"json","max_bytes":8400}`；`graph`: `{"format":"json","max_bytes":8400}` **`[冷JSON+MB]`** |
| 跨项目免切换检索外部已登记工程符号 | `"project"` | **必填 `project`**（工程名，非路径）+ **`query`**，可选 `limit?: 20`（**无 `format`**） |
| 列出可见工作树家族(`family`)、`checkout_id` 与隔离视角路径 | `"checkouts"` | `{"family?":"...","fields?":"...","max_tokens?":...,"format":"json","max_bytes":8400}` **`[冷JSON+MB]`** |
| 列出已保存的多仓/路径复用作用域 | `"scopes"` | `{}`（**无 `format`**） |
| 查询远程 Gortex 代理连接状态 | `"proxy"` | `{}`（**无 `format`**） |

### 2. `workspace_admin` 工作区治理与配置（日常治理 6 项；底层索引与富化 8 项见第十节，共 14 项）
| 触发场景 | `operation` | 内层 `arguments: { ... }` 必填项与注意事项（除标注外均无 `format`） |
| :--- | :--- | :--- |
| 热切换当前会话主工程上下文 | `"set_active_project"` | **必填 `project`**（工程名如 `"my-app"`，非文件路径） |
| 纳管注册新仓库 / 工作树 | `"track"` | **必填 `path`**（绝对路径），可选 `name?`, `as_worktree?: bool`, `force?: bool` |
| 从索引取消跟踪某仓库 | `"untrack"` | **必填 `path`**；**未传 `confirm: true` 时仅作影响预览而不执行删除！** |
| 保存 / 删除可复用的多仓+子路径组合作用域 | `"save_scope"` \| `"delete_scope"` | `save_scope`: **必填 `name`, `repos`（必须为逗号分隔字符串 `"repoA,repoB"`，严禁传 JSON 数组！）**，可选 `paths?`（同样逗号分隔字符串）, `description?`；`delete_scope`: **必填 `name`** |
| 记录或查询智能上下文召回质量反馈 | `"feedback"` | **必填 `action`** (`"record"` \| `"query"`)；`record` 配 `task?`, `useful?`, `not_needed?`, `missing?`（逗号分隔符号 ID 串）, `tool_source?` (`"smart_context"` \| `"prefetch_context"`); `query` 配 `top_n?`, `tool_source?` (`"all"`\|...), `compact?: bool`, `format:"json"` **`[冷JSON]`**（禁传 `repo`） |

```json
call_mcp_tool(ServerName="gortex", ToolName="workspace", Arguments={"operation":"repos","arguments":{"format":"json","max_bytes":8400}})
call_mcp_tool(ServerName="gortex", ToolName="workspace", Arguments={"operation":"project","arguments":{"project":"my-app","query":"MyService","limit":20}})
call_mcp_tool(ServerName="gortex", ToolName="workspace_admin", Arguments={"operation":"set_active_project","arguments":{"project":"my-app"}})
call_mcp_tool(ServerName="gortex", ToolName="workspace_admin", Arguments={"operation":"save_scope","arguments":{"name":"core-svc","repos":"repo-a,repo-b","paths":"src/auth,src/api"}})
```

---

## 一、 检索与探索中枢 (`search` [7] / `explore` [9])
> **容器铁律**：均为平铺门面，外层 `Arguments` 顶层传 `operation`、`options`、`output`，严禁外包 `"arguments"`。

### 核心门禁（防 `search` 闪断 / 搜空 / 锁定）
1. **双探针原则**：排查未知业务逻辑，确认未处于 `localize` 锁态后，可并发 `search.text`（精确字面量）+ `search.winnow`（`min_fan_in: 2`），或先用 `explore.task` / `explore.context` 获取聚焦上下文。
2. **Vue / React / Svelte SFC 法则**：`.vue`、`.tsx`、`.svelte` 的模板事件绑定、插槽与内部闭包通常不在全局导出符号索引中，**必须首选 `search.text`** 定位事件与行号，严禁先用 `symbols` 搜空。
3. **拓扑升格死锁**：若 `search.winnow` 未命中，**严禁直接 `read.file` 大范围裸读或盲改**；必须先从 `search.text` 命中行附近提取外层符号名，二轮跑 `search.winnow(query="真实符号", min_fan_in: 0)` 获取完整符号 ID 与拓扑后，再进入精读或编辑（**纯配置、Markdown、脚本等无代码符号索引的非代码文件除外**，可直接用 `read.file` 分片读取）。
4. **禁 `search.files` 投机令（含例外）**：业务逻辑排查禁首动 `search.files` 猜文件；仅确认文件存在、盘点文件清单或查找明确配置/文档路径时允许使用（`query` 禁通配符 `*`/`?`，通配放 `options.glob`，且 **`query` 与 `options.glob` 至少传一个**）。
5. **严禁开发任务随手调用 `explore.localize`**：`localize` 会激活会话终端锁（`localization_complete`），拦截后续一切 `explore` / `search` / `read`（非定位文件外） / `relations` / `trace` / `analyze` 调用并导致 UI 连续报错闪消！**持续诊断、读多文件或改代码任务必须改用 `explore.task`、`explore.context` 或 `search.*`**；仅“一句话定位即止”时才可用 `localize`。
6. **`explore` 两大高频闪消陷阱与子目录边界（源码实证纠偏）**：
   * **`explore.localize` 双写 `task` 防闪消**：`facade_tools.go:1580-1586` 强校验**顶层 `task` 必填**（缺之必报 `invalid_argument: explore.localize requires task` 闪消！），而 `capabilities` Schema 又要求 **`options.task` 必填**——因此若必须调用 `explore.localize`，**务必同时传入顶层 `task` 与 `options.task`**！**`explore.task` 与 `explore.context` 必填顶层 `task`（`options` 禁写 `task`）；`explore.plan` 必填 `options.task`（顶层禁写 `task`）。**
   * **仓内子目录过滤归属**：`explore.context` 底层通过 `resolvePathFilter` 原生支持 **`options.path`**（逗号分隔仓内子路径，无 `repo/` 前缀）、`task` 内联 **`path:src/foo`** 及 **`options.scope`**；而 **`explore.task` / `explore.localize` 底层 `handleExplore` 未接入 `resolvePathFilter`，会忽略 `options.path` 子目录过滤**——需按子目录精搜时请用 **`explore.context`** 或 **`search.*`**，且勿把仓内子目录写进无说明的顶层 `path`。
7. **`options.new_user_task` 精确白名单（`facade_tools.go:484-518` 硬限制）**：
   * **硬限仅 3 个操作合法**：**`explore.task`、`explore.localize`、`read.file`**，且值**必须为布尔 `true`/`false`**！
   * 即使 `capabilities` 在其他 `read.*`/`explore.*` 的描述文本里回显 `"new_user_task=true..."`，向 **`explore.context`、`explore.closure`、`read.source`、`read.symbols`、`read.summary`、`read.editing_context`、`search.*` 传入 `options.new_user_task` 必报 `invalid_argument` 闪消！**
   * 仅在新用户提问触发后的第一次对应调用（若恰好为 `explore.task` / `explore.localize` / `read.file`）上设 `new_user_task: true`（或用以释放误触的 `localize` 锁），同一任务后续调用与重试均严禁再传。

### 1. `search`（7 个全收录）
| 触发场景 | `operation` | 必填项、`options` 合法字段、易错防错与精确 `output` |
| :--- | :--- | :--- |
| 报错日志、错误码、SQL 文本、Vue/React 模板事件 | `"text"` | **必填顶层 `query`**；`options?: { repo?, path? (无`<repo>/`前缀), project?, scope?, workspace?, regexp?: false, limit?: 35~80 }`（⚠️ **`regexp: true` 时若为非法 RE2 正则必报错闪断！**）；**`[JSON]`（不传 `max_bytes`）** |
| 约束链检索与主干入口探测 / 拓扑确认 | `"winnow"` | 可选顶层 `query`（⚠️ **未传 `query` 时严禁只传 `min_fan_in:0` 或仅传 `repo`/`project`/`ref`，必须至少含 1 个有效结构条件否则必报空约束错误！**）；`options: { repo?, project?, ref?, path_prefix? (无`<repo>/`前缀), language?, kind?, community?, min_fan_in?: 0\|2, min_fan_out?, min_churn?, is_test?, test_role? ("test,benchmark,fuzz,example"), limit?: 25~50, paginate?, cursor? }`（**禁传 `workspace`/`scope`!**）；`output: {"format":"json","max_bytes":8250,"compact?":bool,"fields?":"..."}` |
| 按名称/概念/特征查符号与文档节点 | `"symbols"` | **必填顶层 `query`**（⚠️ 会解析内联 `kind:`/`lang:`/`path:`/`repo:`/`project:`；含字面冒号请改 `search.text`，且勿写 `"A OR B"` `keyword_soup`）；`options?: { repo?, project?, scope?, workspace?, ref?, path? (⚠️ 严禁带 `<repo>/` 前缀), kind?, flavor? ("struct\|class\|component\|..."), corpus? ("code\|docs\|content\|all"), expand? ("both\|equivalence\|llm\|off"), query_class? ("auto\|symbol\|concept\|path\|signature"), vocab_anchored?: bool, max_per_file?, limit?: 28~70, paginate?, cursor?, debug?: bool }`（服务端固定 `assist:"off"`，勿传）；**`[JSON+MB]`**（支持 `compact?`, `fields?`） |
| 语法树结构模式或内置安全/卫生检测器扫描 | `"ast"` | `query` 与 `options.detector` **严格互斥**！① 原生模式：**必传顶层 `query` + `options.language`**（`"go\|python\|javascript\|typescript\|ruby\|java\|kotlin\|scala\|rust\|elixir\|php\|c\|cpp\|csharp\|bash"`）；② 内置规则：`options.detector`（查目录务必传 **`"help"`**，**勿传 `""` 且无 `query`**，底层必报错！）；`options: { repo?, project?, ref?, path_prefix? (无`<repo>/`前缀), exclude_tests?, min_fan_in_of_enclosing_func?, limit?: 28~50 }`（**禁 `workspace`/`scope`!**）；**`[JSON+MB]`** |
| 验证路径存在或按名称/通配符收集文件列表 | `"files"` | **`query` 与 `options.glob` 至少必填其一**（`query` 禁 `*`/`?`，通配放 `options.glob`）；`options?: { repo?, project?, scope?, workspace?, path? (无`<repo>/`前缀), glob?, fuzzy?: false, limit?: 70~220 }`（⚠️ 传不存在 `options.scope` 必报错）；**`[JSON]`（不传 `max_bytes`）** |
| 检索 `.gortex.yaml::artifacts` 非代码知识节点 | `"artifacts"` | 可选顶层 `query`；`options?: { kind? ("schema\|api\|infra\|doc"), limit?: 20 }`（**严禁 `repo`/`project`/`scope`/`workspace`**）；**`[JSON]`** |
| 从种子符号执行 1 跳图扩展候选补全 | `"completion"` | **必填顶层 `query`**；`options?: { edge_kinds? ("calls,references\|all"), seed_limit?, max_seed_expansion?, limit?: 25 }`（**严禁 `repo`/`project`/`scope`/`workspace`**）；**`[JSON]`** |

### 2. `explore`（9 个全收录）
| 触发场景 | `operation` | 参数结构与精确 `output` |
| :--- | :--- | :--- |
| 非终止邻域搜集（持续排查/改代码首选，不锁后续工具） | `"task"` | **必填顶层 `task`**；`options?: { max_symbols?: 10, token_budget?: 1600, new_user_task?: bool }`（**勿在 `options` 写 `task`，也勿靠 `path` 过滤子目录；无 `output`**） |
| 定位即止的终端证据提取（会激活锁，慎用） | `"localize"` | **务必同时传顶层 `task` 与 `options: { task, max_symbols?: 10, token_budget?: 1600, new_user_task?: bool }`**（服务端固定注入 `localize:true`，勿传；**无 `output`**） |
| 单次组装最小聚焦上下文（支持子目录、分级、估算与增量 Delta） | `"context"` | **必填顶层 `task`**（可在 `task` 内写 `path:src/foo` 或传 `options.path`）；`options?: { path?, project?, scope?, entry_point?, max_symbols?: 5, fidelity?: "flat"\|"graded", token_budget?: 8000 (默认值，仅 `graded` 生效；日常推荐 `1600~4000`), delta_from?: "<etag>", if_none_match?: "<etag>", estimate?: bool, include_call_paths?: bool, include_flows?: bool, include_confidence?: bool }`（⚠️ **禁 `new_user_task` / `repo` / `workspace`!**）；**`[JSON+MB]`** |
| 从种子文件/符号构建多跳依赖闭包包 | `"closure"` | `options` **须至少包含 `files` 或 `symbols`**：`{ files?: "<repo>/src/a.rs,<repo>/src/b.rs", symbols?: "id1,id2", edge_kinds?, max_depth?: 6, max_nodes?: 400, rank?: "distance"\|"proximity", token_budget?: 4000, project?, ref?, scope?, workspace? }`（注：不支持 `repo`）；**`[JSON]`** |
| 单次全库叙事级概览（语言/社区/热点/高频导入/入口） | `"outline"` | `options?: {}`；**`[JSON+MB]`** |
| 获取 ~500 Token 极精简代码库速览摘要 | `"wakeup"` | `options?: { max_tokens?: 500, top_communities?: 4, top_entry_points?: 5, top_hotspots?: 5 }`；`output?: { format?: "markdown" \| "json" }`（无 `max_bytes`） |
| 开局最佳工具路径路由推荐 | `"plan"` | **必填 `options: { task, max_calls?: 4 }`**（**顶层禁写 `task`**，无 `output`） |
| 基于最近操作与任务预测下一步上下文 | `"prefetch"` | 可选顶层 `task`；`options?: { recent_symbols? (逗号串), include_source?: bool, limit?: 10, max_tokens?, paginate?: bool, cursor? }`；**`[JSON+MB]`**（支持 `compact?`, `fields?`） |
| 陌生仓库冷启动探索问题建议（5~10 条） | `"suggest"` | `options?: { limit?: 8 }`；**`[JSON]`** |

```json
call_mcp_tool(ServerName="gortex", ToolName="search", Arguments={"operation":"text","query":"handle_save_user","options":{"repo":"my-app","path":"frontend/src/views","limit":50},"output":{"format":"json"}})
call_mcp_tool(ServerName="gortex", ToolName="search", Arguments={"operation":"winnow","query":"forward_handler","options":{"repo":"my-app","path_prefix":"src/api","min_fan_in":0,"limit":30},"output":{"format":"json","max_bytes":8250}})
call_mcp_tool(ServerName="gortex", ToolName="search", Arguments={"operation":"ast","options":{"repo":"my-app","detector":"help"},"output":{"format":"json","max_bytes":8400}})
call_mcp_tool(ServerName="gortex", ToolName="explore", Arguments={"operation":"context","task":"定位登录鉴权失败原因","options":{"path":"src/auth","fidelity":"graded","token_budget":4000},"output":{"format":"json","max_bytes":8400}})
```

---

## 二、 源码精读中枢 (`read` [7])
> **容器铁律**：平铺门面。`target` 严格单字段闭集。**仅 `read.summary` 接受 `repo` / `project` / `scope` / `workspace` / `ref`**，其余 6 个操作均**严禁**传入那 17 个仓库/根目录伪选键！

### 核心门禁（防 `read` 闪断 / 歧义 / 大串溢出）
1. **符号优先**：已拿到完整符号 ID（`<repo>/path::Name`）后**优先使用 `read.symbols`（批量 2~8 个）或 `read.source`（单个）**，严禁退化读整文件。
2. **禁止裸符号歧义与空列表**：`read.source`、`read.symbols`、`read.editing_context` 尽量使用完整 `<repo>/path::Symbol` ID；仅传短名（如 `"init"`）时若多仓或多文件存在同名符号，服务端必报 `invalid_argument: symbol shorthand "..." is ambiguous` 并闪消！`read.symbols` 的 `target.symbols` 严禁传空数组 `[]`。
3. **`read.file` 三大防闪消硬规**：
   * **分段硬锁**：必传 `options: {"offset": 1基行号, "limit": <=150}` + `output: {"max_bytes": 8400}`（**仅认 `output.max_bytes`，严禁传 `output.format`**）；长行/高密度中文文件建议 `limit: 40~80` + `options.max_chars: 8000~14000`。
   * **`physical_evidence` 安全硬锁 (`tools_fileops.go:1756`)**：对 `.env`、`*.yaml`、`*.toml`、`*.properties` 等含敏感键值脱敏的文件开启 `physical_evidence: true` 时，**若未同时设 `allow_secrets: true`，必报 `"physical_evidence for redacted content requires allow_secrets=true"`！** 且 `physical_evidence: true` 不兼容历史 Git `view`（`git_ref` / `commit`）。
   * **多仓路径前缀**：`unbound_global` 模式下 `target.file` 必须带 `<repo>/` 前缀；裸相对路径会在多仓并存或新文件时直接报前缀错误！`options.new_user_task`（布尔）在 `read` 中仅允许用于 `read.file`，禁传于其他 6 个操作。
4. **`read.editing_context` 默认非压缩**：其 `context.compress_bodies` 默认为 **`false`**，读大文件前请显式传入 `context: {"compress_bodies": true}` + `options.keep`。
5. **会话历史认知**：`read.history` 返回**当前 Gortex 会话内修改符号计次与轨迹**，查 Git 行级历史或提交热度用 `workspace_admin.blame` / `analyze(kind="churn")`。

### 全 7 操作契约
| 触发场景 | `operation` | `target`（严格单属性） | 完整参数与精确 `output` |
| :--- | :--- | :--- | :--- |
| 批量精读 2~8 个符号实现（`include_source` 默认 `true`） | `"symbols"` | `{ "symbols": ["<repo>/...::fn1", ...] }` 或 `{ "symbol": "id" }` | `context?: { context_lines?: 3 }`, `options?: { include_source?: true, if_none_match?: "<etag>" }`（禁 `repo` / `new_user_task`）；**`[JSON+MB]`** |
| 精读单个函数/类完整源码（可折叠体省 Token） | `"source"` | `{ "symbol": "<repo>/...::fn1" }` | `context?: { compress_bodies?: false, context_lines?: 3, max_lines?: N }`, `options?: { allow_secrets?: false, if_none_match? }`（禁 `repo` / `new_user_task`）；**`[JSON+MB]`** |
| 局部文件切片读取 / 非代码配置 / 物理摘要取证 | `"file"` | `{ "file": "<repo>/path/to/file" }` | **必传 `options: { offset, limit (<=150) }`**，可选 `max_chars?`, `allow_secrets?: false`, `keep?: "Sym1,Sym2"`, `fidelity_globs?: "internal/**:full,*_test.go:omit"`, `if_none_match?`, `physical_evidence?: bool`, `digest?: "sha256"`, `new_user_task?: bool`；`context?: { compress_bodies?: false, max_lines? }`；**`output: { "max_bytes": 8400 }`（绝对禁传 `format`！）** |
| 文件全部符号结构大纲（唯一支持 `repo` 等过滤器的 `read` 操作） | `"summary"` | `{ "file": "<repo>/path/to/file" }` | `options?: { repo?, project?, scope?, workspace?, ref?, max_tokens?, if_none_match? }`（多仓查非当前 `active_project` 文件时必传 `options.repo`）；`output: { format:"json", max_bytes:8400, compact? }` **`[JSON+MB]`** |
| 修改前单次装配文件/符号与直接上下游依赖上下文 | `"editing_context"` | `{ "file": "..." }` 或 `{ "symbol": "..." }`（禁 `symbols`） | `context?: { compress_bodies?: false }`（**默认 `false`，大文件请显式设 `true`**）, `options?: { detail?: "brief"\|"full", keep?, fidelity_globs?, max_tokens?, if_none_match? }`（禁 `repo` / `new_user_task`）；**`[JSON+MB]`** |
| 查询当前会话修改的符号计数/轨迹 | `"history"` | 可省略 `target` 或传 `{ "symbol": "..." }` | `options?: {}`, `output?: { compact?: false }`（无 `format`/`max_bytes`） |
| 读取 `.gortex.yaml::artifacts` 登记的非代码知识资产 | `"artifact"` | `{ "artifact": "name" }`（或省略 `target` 改传 `options.path`） | `options?: { path? }`（单文件上限 64 KiB，禁 `repo`）；**`[JSON]`** |

```json
call_mcp_tool(ServerName="gortex", ToolName="read", Arguments={"operation":"symbols","target":{"symbols":["my-app/backend/src/main.rs::main","my-app/backend/src/lib.rs::init"]},"output":{"format":"json","max_bytes":8400}})
call_mcp_tool(ServerName="gortex", ToolName="read", Arguments={"operation":"file","target":{"file":"my-app/docs/spec.md"},"options":{"offset":1,"limit":80,"max_chars":12000},"output":{"max_bytes":8400}})
call_mcp_tool(ServerName="gortex", ToolName="read", Arguments={"operation":"summary","target":{"file":"my-app/backend/src/main.rs"},"options":{"repo":"my-app"},"output":{"format":"json","max_bytes":8400}})
```

---

## 三、 修改与重构中枢 (`edit` [10] / `refactor` [6])
> **容器与键名铁律**：
> 1. 均为平铺门面。`edit.batch`/`apply_overlay`/`docs`/`export_graph`/`wiki`/`skill` **不传 `target`**。`edit.batch`/`symbol`/`file`/`write`/`apply_overlay`/`scaffold`/`skill` 与 `refactor.rename`/`delete`/`move`/`inline` **绝对禁止传 `repo` / `project` / `workspace` / `scope` 等 17 个伪选键**！
> 2. **键名防串记口诀**：进入 **`edit.batch.changes[]` 必用 `old_string`/`new_string`（`edit_file`）或 `old_source`/`new_source`（`edit_symbol`），严禁写 `match`/`replacement`/`target`**；单发 **`edit.file` / `edit.symbol` 必用 `match` + `replacement`**，严禁写 `old_`/`new_`！
> 3. **写拦截模式锁提醒**：若任意写入返回 `tool_blocked_by_mode`，先核对是否为只读规则；若在可执行态，调 `session(operation="planning_mode", arguments={"mode":"editing"})` 解锁后再写，严禁连闪重试。

### 核心门禁
1. **批量最高优先 · 单次优选 `match` 决策表**：
   * **多处修改（`>= 2` 项独立编辑操作，跨文件 / 同文件多位置 / 混合删移）→ 最高优先级必用 `edit.batch`**（默认必带 `output: {"compact": true}`）：
     * **数量与体积契约**：**下限 `2` 项，常规推荐 `2~8` 项，单次硬上限 `10` 项且建议估算总文本 `< 4~5 KB`**；
     * **超量串行拆批**：仅当总改动 **`> 10` 项**（或 `9~10` 项但文本总长显著超 `4~5 KB`）时，**必须按批串行拆分调用**，严禁单批塞超 `10` 项，也严禁同轮并发多个 `edit.batch`；
     * **补跑语法诊断**：`edit.batch` 内部未接 `checkParseGate`，批改源码后**必须补齐语法核验（有 LSP 时按修改后的源码文件逐一或并发调 `change.diagnostics`，多批次协同改动待整组落盘后统一检查；纯文档/配置免跑，无 LSP 时以原生构建兜底）**。
   * **单次修改（仅 `1` 处，或单文件仅 `1` 组同模式全量替换 `replace_all: true`）→ 优先用 `match`（首选 `edit.file`，次选 `edit.symbol`）**：
     * `edit.file(match/replacement)` 拥有 `checkParseGate` 语法拦截、`guard: {"expected_occurrences": N}`、`options.base_sha` 防漂移与真实 `dry_run` diff 预览，单点稳态优势极高；**仅 1 处修改禁止强开 `edit.batch`**（`replace_all` 时若已知精确次数建议传 `guard`，未知可省略）。
   * **专用直达通道**：
     * **新建文件或整文件覆写** → **必用 `edit.write`**（`edit.batch` 内 `edit_file` 对不存在文件会失败并整批回滚！新建多个互不依赖的文件允许同轮并发多个 `edit.write`；有依赖或同文件操作须串行）；
     * **纯跨文件/跨引用重命名符号** → **`refactor.rename`**（伴随多处逻辑体改写时，**必须先执行 `refactor.rename` 再执行 `edit.batch`**，严禁同轮并发）。
2. **批量出错两级强制降级铁律（按连续失败次数，成功一次即清零，但已进入一级降级的当前拆批序列维持 `2~4` 项收口不反弹）**：
   * **第 1~2 次连续出错**：按错误类型分流（明确失配直接 `read.file` 对齐，超时/中断先查回执与 `change.detect`）修正重提；
   * **到达第 3~4 次连续出错**：**必须立即降级批量规模**（原 `6~10` 项降为每批 `2~4` 项，原 `4~5` 项降为 `2~3` 项；**若当前批已处于 `2~3` 项最低档仍第 3 次连续出错，直接跳过一级降级切入下述 `match` 通道**）；
   * **到达第 5 次连续出错（`>= 5` 次）**：**必须立即终止 `edit.batch`，强制降级改用 `match`（`edit.file` / `edit.symbol`）完成剩余修改（仅跨不同文件可并发，同文件内多处修改必须逐项串行）！**
3. **最小唯一锚点原则**：无论是 `old_`/`new_` 还是 `match`/`replacement`，只截取**能在目标作用域内唯一识别的最小上下文窗口（通常 1~5 行）**，禁止整段搬运无关模板或整函数源码。
4. **语法门禁与语言/默认值差异**：
   * `edit.file`、`edit.write` 与 `refactor.rename` 带有 `checkParseGate` 语法门禁，支持 `options.allow_parse_errors: true`；**`edit.symbol` 无 `allow_parse_errors`、`guard` 与 `replace_all` 选项**（强传必报错）；`edit.batch` 无 `checkParseGate` 与 `expected_occurrences`；
   * `refactor.fix_all` 与 `refactor.apply_code_action` **无 `dry_run` 参数**（直接修改磁盘，且依赖已运行 LSP），且行列均为 **0-based**（注意 `apply_code_action` 必填 `options.index` 来自 `change.code_actions`）；
   * `refactor.move` 与 `refactor.inline` **当前仅支持 Go 语言**；
   * `edit.scaffold` 默认 `dry_run: true`，实写落盘须显式传 **`dry_run: false`**（其余写操作建议实写时均显式传 `dry_run: false` 以防默认值误判）。

### 全 16 操作契约（除明确标有 `format` 者外，其余写操作均严禁传 `format`！）
| 触发场景 | 门面与 `operation` | 必填/核心参数与精确容器规则 |
| :--- | :--- | :--- |
| **【批量首选】** 跨/单文件 `>=2` 项修改、混批增删改移事务 | `edit("batch")` | **无 `target`**！必填 **`changes`** 数组（**推荐 `2~8` 项、硬上限 `10` 项且 `<4~5 KB`**，超量串行拆批；**默认必带 `output: { "compact": true }`**），4 类 `op` 严格隔离：<br>① `{"op":"edit_file","path":"<repo>/...","old_string":"...","new_string":"...","replace_all?":false}`<br>② `{"op":"edit_symbol","id":"<repo>/...::fn","old_source":"...","new_source":"..."}`<br>③ `{"op":"move_file","source":"<repo>/...","destination":"<repo>/...","expected_sha256?":"..."}`<br>④ `{"op":"delete_file","path":"<repo>/...","expected_sha256?":"..."}`<br>可选顶层 `dry_run?: false`（仅校验规划，不匹配内容）, `options?: { transaction_id?, status_only?: bool }`（查状态省略 `changes`） |
| **【单次首选】** 单文件 1 处精准替换 / 单组 `replace_all` 全量替换 | `edit("file")` | 必填 `target: { file }` + 顶层 **`match`** + **`replacement`**；可选 `dry_run?: false`, `guard?: { expected_occurrences?: N }`, `options?: { replace_all?: false, allow_parse_errors?: false, base_sha?, mutation_id?, physical_evidence?, digest?: "sha256" }` |
| **【单次首选】** 按符号 ID 精准替换 1 处（含内部片段或注释） | `edit("symbol")` | 必填 `target: { symbol }` + 顶层 **`match`** + **`replacement`**；可选 `dry_run?: false`, `options?: { base_sha?, mutation_id?, physical_evidence?, digest?: "sha256" }`（**无 `allow_parse_errors` / `guard` / `replace_all`**） |
| **新建文件**或整文件全量覆写 | `edit("write")` | 必填 `target: { file }` + 顶层 **`content`**；可选 `dry_run?: false`, `options?: { allow_parse_errors?: false, base_sha?, mutation_id?, physical_evidence?, digest?: "sha256" }` |
| 将内存 `overlay` 分支合并并真正落盘 | `edit("apply_overlay")` | **无 `target`**！必填 `options: { from: "<分支名>" }`，可选 `into?: "main"`, `force?: false` |
| 按参考符号生成同模式新实现+测试桩脚手架 | `edit("scaffold")` | 必填 `target: { symbol }` + `options: { new_name }`；实写须传 `dry_run: false`；`output?: { compact? }` |
| 生成活文档 / 导出图谱 / 生成 Wiki / 打包 Skill | `edit("docs" \| "export_graph" \| "wiki" \| "skill")` | **均无 `target`**！<br>• `docs`: `options?:{include?, output_path?, path_prefix?, run_blame?, since?, top?, workspace?}`, `output?:{format:"markdown"\|"json"}`<br>• `export_graph`: `options?:{kinds?, languages?, repo?, scope?, max_communities?, min_community?, no_synthetic?, output_path?, output_dir?}`, `output?:{format:"cypher"\|"graphml"\|"mermaid"}`<br>• `wiki`: `options?:{output_dir?, repo?, project?, workspace?, max_communities?, min_community?, force?, wikilinks?, no_contracts?, no_docs?, no_processes?}`, `output?:{format:"markdown"\|"html"}`<br>• `skill`: `options?:{directory?, skill_name?, description?, output_dir?, include_patterns?, ignore_patterns?, compress?, max_references?}`, `dry_run?:false`, `output?:{format:"json"\|"gcx"\|"toon"}` |
| 全局拓扑协同重命名（跨引用+跨文件） | `refactor("rename")` | 必填 `target: { symbol }` + 顶层 **`new_name`**；可选 `dry_run?: false`, `options?: { allow_parse_errors?: false }` |
| 循环收敛 LSP 自动修复诊断 | `refactor("fix_all")` | 必填 `target: { file }`；`options?: { repo?, kinds? (默认 `"quickfix,source.organizeImports"`), diagnostic_timeout_ms?: 5000, max_actions_per_iter?: 50, max_iterations?: 5 }`（**无 `dry_run`**） |
| 按索引应用单个 LSP CodeAction | `refactor("apply_code_action")` | 必填 `target: { file }` + 必填 `options: { index (来自change.code_actions), start_line?, start_char?, end_line?, end_char? (均0-based!), only?, repo? }`（**无 `dry_run`**） |
| 搬移符号到新包/文件 / 安全级联删除 / 内联调用点 | `refactor("move" \| "delete" \| "inline")` | • `move`(**仅Go**): 必填 `target:{symbol}` + 顶层 **`destination`**, `dry_run?:false`, `options?:{target_package?}`<br>• `delete`: 必填 `target:{symbol}`, `dry_run?:false` (实删传 `false`), `options?:{cascade?:"off"\|"preview"\|"apply", cascade_into_tests?, force?, propagate?}`, **`[JSON]`**<br>• `inline`(**仅Go**): 必填 `target:{symbol}`, `dry_run?:false`, `options?:{delete_after?:true}` |

```json
call_mcp_tool(ServerName="gortex", ToolName="edit", Arguments={"operation":"batch","changes":[{"op":"edit_file","path":"my-app/src/a.rs","old_string":"sig_v1","new_string":"sig_v2"},{"op":"edit_symbol","id":"my-app/src/b.rs::caller","old_source":"sig_v1()","new_source":"sig_v2(ctx)"}],"dry_run":false,"options":{"transaction_id":"tx-sig-01"},"output":{"compact":true}})
call_mcp_tool(ServerName="gortex", ToolName="edit", Arguments={"operation":"file","target":{"file":"my-app/src/a.rs"},"match":"old_call()","replacement":"new_call()","guard":{"expected_occurrences":1},"dry_run":false})
call_mcp_tool(ServerName="gortex", ToolName="edit", Arguments={"operation":"symbol","target":{"symbol":"my-app/src/b.rs::calc"},"match":"x + 1","replacement":"x + 2","dry_run":false})
call_mcp_tool(ServerName="gortex", ToolName="edit", Arguments={"operation":"write","target":{"file":"my-app/src/new_mod.rs"},"content":"pub fn ready() -> bool { true }\n","dry_run":false})
call_mcp_tool(ServerName="gortex", ToolName="refactor", Arguments={"operation":"rename","target":{"symbol":"my-app/src/a.rs::old_fn"},"new_name":"new_fn","dry_run":false})
```

---

## 四、 变更核验与质量中枢 (`change` [19])
> **容器铁律**：平铺门面。除表内明确列出 `repo`/`project`/`ref` 的 5 个操作（`detect`、`diagnostics`、`code_actions`、`api_impact`、`contract`）外，其余 14 个操作**严禁传 `repo` / `project` / `workspace` / `scope`**！

### 核心门禁
1. 源码全部落盘后严禁凭直觉假设正确（尤其 `edit.batch` 内部不运行 `checkParseGate`），必走 **`change.detect` → `change.diagnostics`（按已修改源码文件逐一或并发调用）→ `change.tests`** 闭环，再配宿主原生构建/测试命令跑通实际编译与单测；仅修改纯文档/纯提示词/注释（或无代码逻辑的纯配置）时免强跑 `diagnostics`/`tests`，未启动语言服务（LSP）时以原生构建与测试兜底。
2. **`file_not_indexed` 边界**：`change.impact` 传入无代码符号的文件（Markdown、纯配置、新建文件或被排除文件）会返回 `recoverable: true, condition: "file_not_indexed"`，勿重复调用 `impact`。
3. **JSON 串 vs JSON 对象类型区别（高频校验闪断）**：`change.contract` / `remember.risk_ack` 的 `ranges` 与 `workspace_edit`、以及 `change.preview` / `change.simulate` / `overlay.simulate` 的 `workspace_edit` / `steps` **必须为 JSON 字符串**（且 `steps` 禁传 `"[]"`）；而 **`change.ranges` 的 `source.ranges` 必须为对象数组 `[...]`**！

### 全 19 操作契约
| 触发场景 | `operation` | 参数归属、`repo` 权限与精确 `output`（注意 `limit`/`summary_only` 及 `start_line` 归属！） |
| :--- | :--- | :--- |
| 盘点未提交变动及图谱受影响符号 | `"detect"` | `source?: { scope?: "unstaged"\|"staged"\|"all"\|"compare" }`, `options?: { repo?, base_ref?, limit?: 50, offset?: 0 }`, `output?: { summary_only?: bool }`（无 `format`/`max_bytes`） |
| 在内存影子区预演单次 / 多步 LSP WorkspaceEdit | `"preview"` \| `"simulate"` | `preview`: **必填 `source: { workspace_edit: "<JSON字符串>" }`**, `options?: { diagnostics?: true, diagnostics_timeout_ms?, inherit_overlay?: bool }`<br>`simulate`: **必填 `source: { steps: "<WorkspaceEdit[] JSON字符串，禁\"[]\">" }`**, `options?: { diagnostics?: true, diagnostics_timeout_ms?, inherit_overlay?: bool, stop_on_error?: true }`（服务端固定 `keep:false`，勿传） |
| 提取实时 LSP 诊断 / 查询可用 CodeAction（0-based） | `"diagnostics"` \| `"code_actions"` | `diagnostics`: **必填 `source: { file }`**, `options?: { repo?, project?, wait?: false, timeout_ms? }`<br>`code_actions`: **必填 `source: { file }`**, `options?: { start_line? (0-based), start_char?, end_line?, end_char?, only?, repo?, project? }` |
| 查询写操作磁盘落盘回执（写超时的必查口） | `"receipt"` | `options?: { receipt?: "...", mutation_id?: "..." }` 或 `source?: { path?: "<repo>/..." }`（建议至少传其一；无参查询遇报错查 `capabilities`） |
| 受影响测试 / 架构规则守卫 / 风险分级爆炸半径 | `"tests"` \| `"guards"` \| `"impact"` | 均**必填 `target: { symbol \| symbols:[...] }`**（仅 `impact` 另支持 `target: { file }`，且均禁传 `repo`！）<br>• `tests`: `options?: { depth?: 3 }`, `output?: { compact?: bool }`（无 `format`/`max_bytes`）<br>• `guards`: `output?: { compact?: bool }`, **`[JSON+MB]`**<br>• `impact`: `output?: { format:"json", max_bytes:8400, limit?: 100, offset?: 0, summary_only?: bool }` **`[JSON+MB]`**（注意 `limit`/`offset`/`summary_only` 放 `output`，不要放 `options`！） |
| 改前全流水线风险合同裁决 (`allow`\|`warn`\|`refuse`) | `"contract"` | 必含 `target: { symbol \| symbols:[...] }` **或** 显式非符号 `source: { scope?, base?, path?, ranges?: "<JSON字符串>", workspace_edit?: "<JSON字符串>" }`（⚠️ **`start_line?`, `end_line?` 必须放 `options` 内，严禁塞进 `source`！**行号基准若遇边界偏差以 `capabilities` 为准；缺两者必报 `requires target.symbol/target.symbols or an explicit non-symbol source`）；`options?: { risk_gate?, lens?: "api", repo?, start_line?, end_line? }`；**`[JSON]`** |
| HTTP/gRPC/主题路由变更影响分析 | `"api_impact"` | `source?: { file }` 或 `options?: { route, repo?, project?, ref? }`（`route` 与 `file` 至少其一）；**`[JSON+MB]`** |
| 多符号编辑依赖拓扑排序建议 | `"edit_plan"` | **必填 `target: { symbol \| symbols:[...] }`**；`options?: { depth?: 3 }`（无 `output`） |
| 按示例符号提取可复用结构编码模式 | `"pattern"` | **必填 `source: { symbols: ["id1"] }`（底层按单锚点取首项）**, `options?: { max_source_lines?: 40 }`, `output?: { max_bytes?: 8400 }`（无 `format`） |
| 把 `file` 或行区间转换为对应 Gortex 符号清单 | `"ranges"` | **必填 `source: { file \| ranges: [{"file":"...","start_line":1,"end_line":20}] }`（对象数组，区别于 `contract` 的字符串；1-based 示例）**, `options?: { start_line?, end_line? }`；**`[JSON]`** |
| 验证接口签名变更会破坏哪些调用点 | `"verify"` | **必填 `source: { changes: [{"symbol_id":"...","new_signature":"..."}] }`**, `output?: { compact?: bool }` |
| 列/查会话 overlay 状态，或对比分支/overlay 拓扑影响差异 | `"overlay_branches"` \| `"overlay_state"` \| `"compare_branches"` \| `"compare_overlay"` | `overlay_branches` / `overlay_state`: 仅 `operation`<br>`compare_branches`: **必填** `target:{symbol}` + `options:{a, b, kind:"find_usages\|get_callers\|get_call_chain\|get_dependencies\|get_dependents", depth?, limit?}`<br>`compare_overlay`: **必填** `target:{symbol}` + `options:{kind, depth?, limit?}` |

```json
call_mcp_tool(ServerName="gortex", ToolName="change", Arguments={"operation":"detect","source":{"scope":"unstaged"},"options":{"repo":"my-app","limit":50}})
call_mcp_tool(ServerName="gortex", ToolName="change", Arguments={"operation":"diagnostics","source":{"file":"my-app/backend/src/main.rs"},"options":{"repo":"my-app","wait":false}})
call_mcp_tool(ServerName="gortex", ToolName="change", Arguments={"operation":"contract","source":{"scope":"unstaged"},"options":{"repo":"my-app","lens":"api"},"output":{"format":"json"}})
call_mcp_tool(ServerName="gortex", ToolName="change", Arguments={"operation":"impact","target":{"symbol":"my-app/backend/src/main.rs::main"},"output":{"format":"json","max_bytes":8400,"summary_only":true}})
```

---

# 【L2 级：深度拓扑与架构审计】（进阶 · 析-联-迹三大中枢）

## 五、 架构分析中枢 (`analyze` [96] — 顶层必传 `kind`，严禁写 `operation`！)
> **容器与 Schema 纠偏警告**：
> 1. `analyze` 外层 `Arguments` 顶层固定用 **`"kind": "<类型名>"`**（严禁写 `"operation"`）。
> 2. 仅 **`co_change`、`why`、`def_use` 必填 `target`**，仅 **`impact` 可选 `target`**；**其余 92 个 `kind` 严禁传入 `target`！**
> 3. **14 个全禁域项 + 2 个部分限域项（`contracts`、`why`）纠偏**：下表中未标注 `...域过滤` 的 14 个 `kind`（`architecture`、`coupling`、`churn`、`co_change`、`untested`、`extraction`、`knowledge_gaps`、`surprising_connections`、`agent_config`、`inspection_catalog`、`replay`、`lint`、`citation`、`help`）在 `capabilities` 中已开启 **`additionalProperties: false` 且不含 `repo`/`workspace`/`scope`/`project`**，混传必报闪消错误！**`contracts` 仅允许 `{repo, project, ref}`（禁 `scope`/`workspace`）；`why` 仅允许记忆域 `scope`（禁 `repo`/`project`/`workspace`）**。
> 4. **分组 2 参数边界**：诸多专项分析（如 `routes`、`models`、`k8s_resources`、`kustomize`、`event_emitters`、`field_writers`、`annotation_users`、`stale_flags.older_than`、`images` 等）并未在静态 `options` 中开放其说明里提及的细粒度过滤键！调用分组 2 未单列特例字段的 `kind` 时，**`options` 请严守通用域过滤 `{repo?, project?, scope?, workspace?}`**，禁止臆测传参。

### 核心门禁
大规模重构或动用公共抽象前，优先运行 `cycles` 与 `dead_code`（及 `hotspots`）；排查竞态与状态共享风险用 `race_writes` 与 `field_writers`；注意本门面**不存在 `"layers"` 类型**（层映射建议用 `suggest_boundaries`）。

### 1. 高频与带专属静态参数 `kind` 精要表（共 43 项）
| 分类场景 | `kind` | 必填 / `target` / 专属 `options` / `repo` 权限与精确 `output` |
| :--- | :--- | :--- |
| 模块依赖环 / 高耦合热点 / 死代码 | `"cycles"` \| `"hotspots"` \| `"dead_code"` | **通用域过滤** `{repo?, project?, scope?, workspace?}`（⚠️ **`cycles` 中 `options.scope` 同样是 Saved-Scope slug，传目录前缀如 `"src/auth"` 会报错！**）；`hotspots`: `options:{threshold?}`; `dead_code`: `options:{include_variables?, include_fields?, include_constants?, include_cgo_exports?, include_linkname_targets?, skip_cross_repo_nodes?}` — **均 `[JSON+MB]`**（支持 `compact?`） |
| 全局架构快照 / 社区探测 / 聚类算法对比 | `"architecture"` \| `"communities"` \| `"clusters"` \| `"louvain"` \| `"concepts"` | • `architecture`: `options:{path_prefix?, resolution?:"file\|package\|service\|system", top_communities?, top_entry_points?, top_hotspots?, top_processes?}`（⚠️ **无 `repo`/`project`/`scope`/`workspace`！**），**`[JSON]`**<br>• `communities`: `options:{id?, ...域过滤}`（**无 `output`**）<br>• `clusters`: `options:{algorithm?:"leiden\|louvain\|spectral", min_size?:3, resolution?:1.0, ...域过滤}`, **`[JSON+MB]`**<br>• `louvain`: 仅通用域过滤, **`[JSON+MB]`**<br>• **`concepts`**（服务端固定 `use_llm:false`，勿传）: 仅通用域过滤, **`[JSON+MB]`** |
| 耦合度与不稳定性 / Git 共变耦合 | `"coupling"` \| `"co_change"` | • `coupling`: `options:{unit?:"package\|community", package_depth?:2, min_members?:1, path_prefix?, sort_by?}`（⚠️ **无 `repo`/`project`/`scope`/`workspace`！**），`output:{format:"json", limit?:50}`（**无 `max_bytes`**）<br>• `co_change`: 服务端固定 `refresh:false`（勿传）; **必填 `target:{file\|symbol}`** + `options:{file_path?, symbol_id?, min_score?}`（⚠️ **无 `repo`！**），`output:{format:"json", limit?:20}` |
| 综合爆炸半径评分 / 竞态写入 / 提交扰动热度 | `"impact"` \| `"race_writes"` \| `"churn"` | • `impact`: 服务端固定 `refresh_cochange:false`（勿传）; **可选 `target:{file\|symbol}`** + `options:{id?, ids?, path?, kinds?, min_score?, max_score?, path_prefix?, ...域过滤}`, **`[JSON+MB]`**(`compact?, limit?`)<br>• `race_writes`: 仅通用域过滤, **`[JSON+MB]`**<br>• `churn`: `options:{kinds?:"function,method\|all", min_commits?:1, path_prefix?, sort_by?}`（⚠️ **无 `repo`/`project`/`scope`/`workspace`！**），`output:{format:"json", limit?:100}`（**无 `max_bytes`**） |
| 复杂度评级 / 多轴健康分 / 未测盲区 / 可提取函数推荐 | `"health"` \| `"health_score"` \| `"untested"` \| `"extraction"` | • `health`: 仅通用域过滤（**无 `output`**）<br>• `health_score`: `options:{grade?:"d,f", min_score?, max_score?, min_axes?:1, roll_up?:"file\|repo", ...域过滤}`, **`[JSON+MB]`**(`compact?`)<br>• `untested`: `options:{file_prefix?, min_fan_in?}`（⚠️ **禁 `repo`**）, `output:{limit?:50}`（**无 `format`/`max_bytes`**）<br>• `extraction`: `options:{min_callers?, min_fan_out?, min_lines?, path_prefix?}`（⚠️ **禁 `repo`**）, `output:{format:"json", limit?:25}` |
| 规则巡检目录与扫描 / 无 LSP 外置 Lint / SAST / 预置安全包 | `"inspection_catalog"` \| `"inspections"` \| `"lint"` \| `"sast"` \| `"hygiene"` \| `"named"` | • `inspection_catalog`: `options:{category?}`（**禁 `repo`**）, **`[JSON]`**<br>• `inspections`: `options:{inspections:"all\|...", severity?, path_prefix?, max_per_inspection?, ...域过滤}`, **`[JSON]`**<br>• `lint`: **必填 `options:{path, language?:"go\|python\|shell", timeout_ms?}`**（**禁 `repo`，无 `output`**）<br>• `sast` / `hygiene`: `options:{cwe?, detector?, language?, severity?, exclude_tests?, kinds_only?, ...域过滤}`, **`[JSON+MB]`**<br>• `named`: `options:{name? ("sql-injection\|xss\|ssrf\|hardcoded-secrets\|..."), ...域过滤}`, **`[JSON+MB]`** |
| 存在动机追溯 / 故障回放 / 预检成环 | `"why"` \| `"replay"` \| `"would_create_cycle"` | • `why`: **必填 `target:{symbol}` + `options:{id, scope?:"workspace"\|"global"}`**（禁 `repo`/`project`/`workspace`；若不确定 `id` 与 `symbol` 配合要求遇报错查 `capabilities`），`output:{format:"json", limit?:20}`<br>• `replay`: `options:{anchor_symbol?, depth?:3, window_days?:30 (0禁用)}`, `output:{format:"json", limit?:25}`（**禁 `repo`**）<br>• `would_create_cycle`: **必填 `options:{from_id, to_id}`**（+可选域过滤），**`[JSON+MB]`** |
| 覆盖率与所有权 / 陈旧代码与开关 / 克隆与执行流 | `"coverage_gaps"` \| `"coverage_summary"` \| `"ownership"` \| `"stale_code"` \| `"stale_flags"` \| `"clones"` \| `"processes"` | • `coverage_gaps`: `options:{min_pct?:0, max_pct?:100, path_prefix?, ...域过滤}`, **`[JSON+MB]`**<br>• `ownership`: `options:{kinds?:"function,method\|all", min_symbols?:1, path_prefix?, ...域过滤}`, **`[JSON+MB]`**<br>• `stale_code`: `options:{older_than?:365, email?, kinds?, ...域过滤}`, **`[JSON+MB]`**<br>• `stale_flags`: `options:{provider?:"launchdarkly\|growthbook\|unleash\|internal", ...域过滤}`（无 `older_than`）, **`[JSON+MB]`**<br>• `clones`: `options:{min_similarity?, dead_only?, path_prefix?, ...域过滤}`, **`[JSON+MB]`**(`limit?:50`)<br>• `processes`: `options:{id?, ...域过滤}`, **`[JSON+MB]`**<br>• `coverage_summary`: 仅通用域过滤, **`[JSON+MB]`** |
| 代理规则审计 / 引用验真 / 文档盲区 / 异常边 / 增量 / TODO / 契约 / 边界建议 | `"agent_config"` \| `"citation"` \| `"knowledge_gaps"` \| `"surprising_connections"` \| `"recent_changes"` \| `"todos"` \| `"contracts"` \| `"suggest_boundaries"` | • `agent_config`: `options:{files?, root?}`（**仅此处合法传 `root`；禁 `repo`**）, `output:{compact?}`（无 `format`/`max_bytes`）<br>• `citation`: **必填 `options:{file_path, span, sha?}`**（**禁 `repo`**；`span` 格式不确定时可首错查 `capabilities`）, **`[JSON]`**<br>• `knowledge_gaps`: `options:{path_prefix?, min_coverage_pct?, hotspot_limit?, thin_community_size?, limit_per_category?}`（⚠️ **禁 `repo`**）, **`[JSON]`**<br>• `surprising_connections`: `options:{path_prefix?, hub_threshold?, min_score?, rare_kind_pct?}`（⚠️ **禁 `repo`**）, `output:{format:"json", limit?:25}`<br>• `recent_changes`: `options:{since?, ...域过滤}`（watch 模式，**无 `output`**）<br>• `todos`: `options:{tag?, assignee?, has_assignee?, ticket?, ...域过滤}`, **`[JSON+MB]`**<br>• `contracts`: `options:{action?:"list\|check\|validate\|bridge", repo?, project?, ref?}`（⚠️ **无 `scope`/`workspace`!**）, **`[JSON+MB]`**（支持 `fields?`，分页参数 `cursor?` 归属若不确定以 `capabilities` 为准，其余分页接口 `cursor` 均在 `options`）<br>• `suggest_boundaries`: 仅通用域过滤, **`[JSON+MB]`** |
| 跨仓边汇总 / 过程内定义-使用链 | `"cross_repo"` \| `"def_use"` | • `cross_repo`: `options:{base_kind?:"calls\|implements\|extends", ...域过滤}`（`options.repo` 作边界仓过滤）, `output:{compact?, format:"json", limit?:200, max_bytes:8400}`<br>• `def_use`: **必填 `target:{symbol}`** + `options:{id?, ...域过滤}`, **`[JSON+MB]`**(`compact?`) |

### 2. 全量其余专项 `kind` 目录（其余 53 项，全计 **43 + 53 = 96** 个全收录）
> 除个别特例标明外，本组 `options` 统一支持且**仅支持** `{repo?, project?, scope?, workspace?}`，`output` 统一支持 **`[JSON+MB]`**（配 `compact?`）：

* **图谱拓扑、函数流与诊断（15）**：`bottlenecks`、`error_surface`（支持 `output.limit`）、`field_writers`、`indirect_mutations`、`kcore`、`pagerank`、`scc`、`wcc`、`connectivity_health`、`edge_audit`、`ref_facts`、`resolution_outcomes`、`retrieval_log`、`speculative`、`synthesizers`。
* **并发、通道与不安全模式（4）**：`channel_ops`、`unclosed_channels`、`goroutine_spawns`、`unsafe_patterns`（特例 `options:{detector?, language?, severity?, exclude_tests?}` 与 `output.limit`；若需叠加 `repo` 等域过滤且遇校验报错，按铁律 8 查 `capabilities`）。
* **框架、路由、数据与事件（15）**：`routes`、`route_frameworks`、`models`、`dbt_models`（**另支持 `options:{framework?:"dbt\|sqlmesh", materialized?}`**）、`orphan_tables`、`unreferenced_tables`、`sql_call_sites`（服务端固定 `materialize:false`，勿传）、`config_readers`、`env_var_users`、`event_emitters`、`log_events`、`string_emitters`、`pubsub`、`temporal_orphans`、`domain`。
* **前端 / 移动端 / 语言生态专项（10）**：`components`、`swiftui_views`、`uikit_classes`、`drupal_hooks`、`cgo_users`、`wasm_users`、`annotation_users`、`constructors_missing_fields`、`external_calls`、`tests_as_edges`（**另支持 `options:{group_by?:"symbol\|test"}`**）。
* **基础设施、发布与历史演进（9）**：`images`、`k8s_resources`、`kustomize`、`releases`（**另支持 `options:{tag?}`**）、`fixes_history`、`doc_staleness`、`role`、`review`（本地规则引擎）、`help`（`options:{}`，**禁 `repo`，无 `output`**）。

```json
call_mcp_tool(ServerName="gortex", ToolName="analyze", Arguments={"kind":"cycles","options":{"repo":"my-app"},"output":{"format":"json","max_bytes":8400}})
call_mcp_tool(ServerName="gortex", ToolName="analyze", Arguments={"kind":"architecture","options":{"path_prefix":"backend/src","resolution":"package"},"output":{"format":"json"}})
call_mcp_tool(ServerName="gortex", ToolName="analyze", Arguments={"kind":"lint","options":{"path":"my-app/backend/src/main.rs"}})
```

---

## 六、 关系网络中枢 (`relations` [11])
> **容器铁律**：平铺门面。前置 8 项支持通用域过滤 `{repo?, project?, scope?, workspace?, ref?}`；**`references`、`declaration`、`import_path` 绝对严禁传 `repo` / `project` / `scope` / `workspace` 等 17 个伪选键**！

### 核心门禁
修改任何公共符号签名、返回值或可见性前，**必先调用 `callers` 或 `usages`** 排查所有引用方；向下查依赖的唯一合法操作名为 **`dependencies`（严禁自创 `callees`）**；查引用用 `usages`/`references`（严禁写 `instantiations`）；查实现类/接口实现用 `implementations`。
* **通用置信度过滤**：`min_tier`（按可信度降序：`lsp_resolved` > `lsp_dispatch` > `ast_resolved` > `ast_inferred` > `text_matched`）与 `include_speculative?: bool`。

| `operation` | `target`（严格单属性） | 核心参数与精确 `output`（注意 `exclude_tests` 权限区别！） |
| :--- | :--- | :--- |
| `"callers"` | `{ "symbol": "id" }` | `options?: { depth?: 2, limit?: 40~50, exclude_tests?: bool, min_tier?, include_speculative?, ...域过滤 }`；**`[JSON+MB]`**(`compact?`) |
| `"dependencies"` | `{ "symbol": "id" }` | `options?: { depth?: 2, limit?: 40~50, min_tier?, include_speculative?, ...域过滤 }`（⚠️ **不支持 `exclude_tests`!**）；**`[JSON+MB]`**(`compact?`) |
| `"usages"` | `{ "symbol": "id" }` | `options?: { context?: "parameter_type\|return_type\|field\|value\|type\|attribute\|generic_arg\|call\|import", flavor?, return_usage?: "discarded\|assigned\|partially_ignored\|returned\|goroutine\|deferred\|argument\|condition", group_by?: "file", exclude_tests?: bool, limit?: 50, max_tokens?, min_tier?, include_speculative?, ...域过滤 }`；**`[JSON+MB]`**(`compact?`) |
| `"implementations"` / `"overrides"` / `"hierarchy"` | `{ "symbol": "id" }` | `overrides`: `options:{direction?:"children\|parents"}`; `hierarchy`: `options:{direction?:"both\|up\|down", depth?:5, include_methods?:bool}`; 三者均支持通用域过滤 + `min_tier` + `include_speculative`；**`[JSON+MB]`** |
| `"dependents"` | `{ "symbol": "id" }`（不支持 `target.file`） | `options?: { depth?: 3, limit?: 50, min_tier?, include_speculative?, ...域过滤 }`（**不支持 `exclude_tests`!**）；**`[JSON+MB]`**(`compact?`) |
| `"cluster"` | `{ "symbol": "id" }` | `options?: { radius?: 2, limit?: 50, ...域过滤 }`；**`[JSON+MB]`**(`compact?`) |
| `"references"` | 可选 `{ "symbol": "id" }` 或省略改用 `options.name` | `options?: { name?, evidence_limit?: 50, exclude_tests?, min_tier? }`（**禁 `repo`等**）；**`[JSON]`** |
| `"declaration"` | `{ "query": "使用点片段" }` | `options?: { kind?, limit?: 20, path_prefix?, regex?: false }`（**禁 `repo`等**）；**`[JSON]`**（支持 `format:"json\|toon"`） |
| `"import_path"` | `{ "symbol": "id" }` | **必填 `options: { path: "目标文件相对路径" }`**（**禁 `repo`等，无 `output`**；多仓若报错查 `capabilities` 核对是否需 `<repo>/` 前缀） |

```json
call_mcp_tool(ServerName="gortex", ToolName="relations", Arguments={"operation":"callers","target":{"symbol":"my-app/backend/src/mod.rs::init"},"options":{"limit":40,"exclude_tests":true},"output":{"format":"json","max_bytes":8400}})
```

---

## 七、 链路追踪中枢 (`trace` [7])
> **容器铁律**：平铺门面。**双端点操作（`path`、`flow`、`taint`）必须同时传 `target` 与 `to`**；单点/查询操作（`cfg`、`call_chain`、`walk`、`graph`）**严禁传 `to`**；**仅 `call_chain` 和 `walk` 支持 `{repo?, project?, scope?, workspace?, ref?}`**，其余 5 项严格禁传！`call_chain` **严禁传 `output.compact: true`**（以防丢失行号调用链路）。

| `operation` | 端点容器与核心参数 | 精确 `output` |
| :--- | :--- | :--- |
| `"path"` \| `"flow"` | `target:{symbol:"A"}` + `to:{symbol:"B"}`；`path`: `options:{max_depth:8~24, k?:1, include_references?:true, max_frontier?, min_tier?}`; `flow`: `options:{max_depth?:8, max_paths?:10, min_tier?}` | **`[JSON+MB]`** |
| `"taint"` | `target:{query:"输入源关键字"}` + `to:{query:"危险点关键字"}`（⚠️ 用 `query`，勿用 `symbol`）, `options:{max_depth?:8, limit?:20, min_tier?}` | **`[JSON+MB]`** |
| `"cfg"` \| `"call_chain"` | `target:{symbol:"id"}`；`cfg`: `options:{mermaid?:false}`; `call_chain`: `options:{depth?:4, limit?:50, max_tokens?, min_tier?, include_speculative?, ...域过滤}` | **`[JSON+MB]`**（`call_chain` 严禁 `compact:true`） |
| `"walk"` \| `"graph"` | `walk`: 必填 `target:{symbol}` + `options:{edge_kinds?:"calls", direction?:"out\|in\|both", max_depth?:8, token_budget?:6000, community?, ...域过滤}`; `graph`: **无 `target`，必填 `options:{query:"DSL语句", limit?:100}`**（禁 `repo`） | **`[JSON]`** |

```json
call_mcp_tool(ServerName="gortex", ToolName="trace", Arguments={"operation":"path","target":{"symbol":"my-app/src/a.rs::fn1"},"to":{"symbol":"my-app/src/b.rs::fn2"},"options":{"max_depth":16,"k":1},"output":{"format":"json","max_bytes":8400}})
call_mcp_tool(ServerName="gortex", ToolName="trace", Arguments={"operation":"graph","options":{"query":"nodes kind=interface | traverse implements in","limit":100},"output":{"format":"json"}})
```

---

# 【L3 级：协同推演、审查、PR 与空间/会话运维】

## 八、 虚拟推演与认知记忆 (`overlay` [10] / `recall` [8] / `remember` [8] — 均冷门面，用内层 `arguments`)
> **类型防闪断警报**：
> 1. `recall` 内的 `scope` 指代认知检索域（`"workspace" | "global" | "both"`），`remember` 内的 `scope` 指代认知写入域（`"workspace" | "global"`），切勿与多仓 `saved-scope` 混淆；除 `remember.risk_ack` 支持 `repo` 外，其余操作均禁传仓库 `repo`。
> 2. **数组 vs 逗号串类型边界**：`remember.memory` 的 `symbol_ids`/`file_paths`/`tags`/`supersedes`、`recall.surface` 的 `symbol_ids`/`file_paths`、`remember.note`/`notebook` 的 `tags` 与 `remember.risk_ack` 的 `symbols` **全部为逗号分隔字符串**（如 `"a,b"`），**严禁传 JSON 数组 `[...]`！** 且 `risk_ack` 的 `ranges` / `workspace_edit` 必须为 JSON 字符串。

### 1. 内存虚拟分支推演 (`overlay` — 10 操作，均无 `format`)
| `operation` | 内层 `arguments: { ... }` 核心参数与服务端固定行为 |
| :--- | :--- |
| `"register"` / `"drop"` / `"keepalive"` | `register`: `{workspace_id?}`；`drop`: `{}`（销毁整个会话 overlay）；`keepalive`: `{}`（续期会话 TTL） |
| `"fork"` / `"switch"` / `"drop_branch"` | `fork`: **必填 `name`**（字母数字/短横/下划线，<=64 字符，**勿含 `/`!**）, 可选 `from?, activate?:bool`；`switch` / `drop_branch`: **必填 `name`** |
| `"push"` / `"delete"` | `push`: **必填 `path`**, 可选 `content?, base_sha?, deleted?:bool`；`delete`: **必填 `path`** |
| `"merge"` / `"simulate"` | `merge`: **必填 `from`**, 可选 `into?:"main", force?:false`（**服务端固定 `to_disk:false`，仅内存合并**；落盘请用 `edit.apply_overlay`）；`simulate`: **必填 `steps:"<WorkspaceEdit[] JSON字符串，禁\"[]\">"`**, 可选 `diagnostics?:true, diagnostics_timeout_ms?, inherit_overlay?, stop_on_error?:true`（服务端固定 `keep:true`，勿传） |

### 2. 认知记忆检索与召回 (`recall` — 8 操作，均 **`[冷JSON]`**)
| `operation` | 内层 `arguments: { ... }` 核心参数 |
| :--- | :--- |
| `"surface"` | `{task?, symbol_ids? (逗号串), file_paths? (逗号串), scope?:"workspace"\|"global"\|"both", limit?:10, min_score?, excerpt_chars?:320, include_superseded?:false}`（服务端固定 `mark_accessed:false`，勿传） |
| `"distill"` | `{max_symbols?:10, max_files?:10, max_tags?:10, max_recent?:8, excerpt_chars?:240, session_id? ("all"查全部)}`（长对话压缩后恢复工作记忆首动） |
| `"memories"` | `{text?, kind?:"invariant"\|"constraint"\|"convention"\|"gotcha"\|"decision"\|"incident"\|"reference", symbol_id?, file_path?, tag?, source?, author?, scope?:"workspace"\|"global"\|"both", min_importance?, pinned_only?, include_superseded?, since?, limit?:50}` |
| `"notes"` | `{text?, symbol_id?, file_path?, tag?, session_id?, pinned_only?, since?, limit?:50}` |
| `"notebook_list"` / `"notebook_find"` / `"notebook_show"` | `notebook_list`: `{limit?:50}`; `notebook_find`: `{query?, limit?:50}`; `notebook_show`: `{id}` |
| `"onboarding"` | `{essential_kinds?, min_per_kind?, min_total?}` |

### 3. 记忆写入、笔记归档与风险确认 (`remember` — 8 操作)
| `operation` | 内层 `arguments: { ... }` 核心参数与输出 |
| :--- | :--- |
| `"memory"` | `{title?, body?, kind?, symbol_ids? (逗号串), file_paths? (逗号串), tags? (逗号串), importance?:1..5, confidence?, scope?:"workspace"\|"global", source?, pinned?, id?, supersedes? (逗号串), superseded_by?, no_autolink?}` **`[冷JSON]`** |
| `"edit_memory"` / `"rename_memory"` | `edit_memory`: `{id, pattern, replacement, mode?:"regex"\|"literal", scope?:"workspace"\|"global"}` **`[冷JSON]`**；`rename_memory`: `{id, to_scope, from_scope?}` **`[冷JSON]`** |
| `"note"` / `"notebook"` / `"notebook_used"` | `note`: `{body, file_path?, symbol_id?, links?, tags? (逗号串), pinned?, id?, no_autolink?}` **`[冷JSON]`**；`notebook`: `{body, title?, tags? (逗号串), id?}` **`[冷JSON]`**；`notebook_used`: `{id}` **`[冷JSON]`** |
| `"risk_ack"` | `{source?:"auto"\|"edit"\|"diff"\|"symbols"\|"ranges" (字符串枚举，非对象), symbols? (逗号串!), scope?, base?, path?, start_line?, end_line?, ranges? (JSON字符串!), workspace_edit? (JSON字符串!), lens?, risk_gate?, repo?}`（服务端固定注入 `ack:true`，勿传；行号基准未确认时查 `capabilities`） **`[冷JSON]`** |
| `"suppress_finding"` | `{action?:"add"\|"list"\|"remove", identity_key?, rule?, category?, symbol_id?, file?, source_line?, line?, reason?, author?}`（行号基准未确认时查 `capabilities`） **`[冷JSON+MB]`**（`remember` 内唯一支持 `max_bytes`） |

---

## 九、 协同审查与 PR 雷达 (`review` [7] / `pr` [6] / `publish_review` [1])
> 全部 14 个操作均支持 `repo`（`review` 为平铺门面，参数放 `source`/`options`/`output`；`pr` 与 `publish_review` 为冷门面，须封装在内层 `Arguments.arguments: {...}`；如 `edits`/`findings`/`prior_review` 等复杂载荷结构不确定，遇错按铁律 8 查 `capabilities`）。

1. **本地只读审查 (`review` — 平铺门面，7 操作)**：
   * `run`：`source:{scope?:"unstaged"\|"staged"\|"all"\|"compare", base?, diff?}`，`options?:{base_ref?, repo?, use_llm?:false, max_tokens?}`，**`[JSON+MB]`**；
   * `pack`：同 `run` + `options:{include_pack?:false, scrub?:false}`，**`[JSON+MB]`**；
   * `diff_context`：`source:{scope?}`，`options?:{base_ref?, repo?}`，`output?:{compact?}`（**无 `format`/`max_bytes`**）；
   * `pr_context`：`source:{scope?, base?}`，`options?:{base_ref?, ids?, repo?, sections?, verify?:true, audit_config?:true, edits?, session_id?, max_tokens?}`（⚠️ 仅同时传入 `session_id` 时才执行 `edits` 推演），**`[JSON+MB]`**；
   * `questions`：`source?:{base?}`，`options?:{ids?, path_prefix?, repo?, categories?, hub_threshold?:8, min_betweenness?:20, min_community_size?:3, limit?:20, max_tokens?}`，**`[JSON+MB]`**；
   * `sibling_context`：`source:{scope?, base?}`，`options?:{base_ref?, focus_file?, focus_files?, focus_symbol_id?, repo?, max_tokens?}`，**`[JSON+MB]`**；
   * `critique`：`source:{scope?, base?, diff?}`，`options?:{base_ref?, findings?, prior_review?, repo?, max_tokens?:1500}`，**`[JSON+MB]`**。
2. **PR 雷达分析 (`pr` — 冷门面内层 `arguments`，6 操作，均 `[冷JSON+MB]`)**：
   * `list`：`{state?:"open"\|"closed"\|"all", limit?:30, prs?, repo?}`；
   * `impact`：**必填 `number`**，可选 `{files?, receipt?, scrub?, repo?}`；
   * `conflicts`：`{limit?:20, prs?, files?, repo?}`（**不存在 `pr_numbers` 字段**）；
   * `reviewers`：`{ids? | base? | number?, limit?:10, repo?}`（三选一）；
   * `risk`：`{ids? | base?, repo?}`（二选一）；
   * `triage`：`{limit?:20, prs?, files?, repo?, use_llm?:false, max_tokens?:512}`。
3. **对外发布评审 (`publish_review` — 冷门面内层 `arguments`，1 操作，`[冷JSON+MB]`)**：
   * `post`：**必填 `number`**，可选 `{findings?, summary?, scope?, base?, base_ref?, diff?, dry_run?:false, confirm_public?:false, public?:false, refuse_on_secret?:true, owner?, repo_name?, provider?:"github", repo?}`。
   * **高危安全警报**：`dry_run` 默认为 **`false`**（非 `true`！），仅为演练预览时**必须显式传入 `"dry_run": true`**；目标为公开/Fork PR（`public: true`）时须显式传 `"confirm_public": true` 才会放行。

---

## 十、 工作区底层索引与时空富化 (`workspace_admin` 运维项，冷门面内层 `arguments`)
1. **索引重建红线**：仅在索引损坏、初次手动入库或大量外部构建变更未感知时使用：
   * `workspace_admin`：`Arguments={"operation":"index","arguments":{"path":"<仓库绝对路径>"}}`（**`index` 不支持 `format`**）；
   * `workspace_admin`：`Arguments={"operation":"reindex","arguments":{"path?":"<仓库绝对路径或仓名>","paths?":["..."],"format":"json"}}` **`[冷JSON]`**。严禁日常开发频刷全量重建。
2. **Git / 覆盖率 / SQL / 时序富化分析**：
   * **4 个混合型操作（内层必填 `"arguments": {}` + 外层顶层 `output`）**：`blame`、`coverage`（**内层必填 `arguments.profile`**，如 `"coverage.out"`）、`sql_rebuild`、`temporal_verify`。内层 `arguments` 支持 `{repo?, project?, scope?, workspace?, profile?}`，外层顶层支持 `output: {"compact?": bool, "format": "json", "max_bytes": 8400}` **`[JSON+MB]`**；
   * **2 个冷 JSON 富化写盘**：`enrich_churn`、`enrich_releases`，`arguments: {"branch?": "...", "path?": "...", "format": "json"}` **`[冷JSON]`**。

```json
call_mcp_tool(ServerName="gortex", ToolName="workspace_admin", Arguments={"operation":"blame","arguments":{"repo":"my-app"},"output":{"format":"json","max_bytes":8400}})
call_mcp_tool(ServerName="gortex", ToolName="workspace_admin", Arguments={"operation":"coverage","arguments":{"profile":"coverage.out","repo":"my-app"},"output":{"format":"json","max_bytes":8400}})
```

---

## 十一、 缓冲提炼 (`response` [5])、会话治理 (`session` [8]) 与 MCP 原生 Resource / Prompt
1. **大输出缓冲提炼 (`response` — 冷门面内层 `arguments`，5 操作)**：
   * `grep`：**必填 `pattern`**, 可选 `response_id?, context?, before?, after?, ignore_case?:bool, literal?:bool, max_matches?`；
   * `slice`：`{from?: 1, to?, response_id?}`；
   * `peek`：`{lines?: 20, response_id?}`；
   * `stats`：`{}`（列出近期缓存的 `response_id` 与大小统计）；
   * `export_context`：**必填 `task`**, 可选 `entry_point?, max_symbols?:5, token_budget?:2000, format?:"markdown"\|"json"`（**仅此项支持 `format`**）。
2. **会话状态治理 (`session` — 8 操作，⚠️ 严禁传顶层 `output`！)**：
   * **顶层 `channel` 型（禁把 `channel` 塞进内层 `arguments`）**：
     `subscribe` / `unsubscribe` → 外层 `Arguments` 顶层必填 `channel`（`"daemon_health"` | `"diagnostics"` | `"graph_invalidated"` | `"stale_refs"` | `"workspace_readiness"`），可选 `"arguments": {}`（**无 `output` 字段**）；
   * **内层 `arguments` 封装型（顶层仅 `{operation, arguments, view}`，严禁传顶层 `output` 或 `require_exact`/`require_fresh`/`wait_deadline`！）**：
     * `planning_mode`：`arguments:{"mode":"planning"|"editing"}`（切换只读规划态/可写编辑态，解 `tool_blocked_by_mode`）；
     * `cursor`：`arguments:{"action":"goto"|"into"|"up"|"sibling"|"back"|"where"|"read", id? ("goto"必填), select?, format?:"json"|"toon"}`；
     * `agents`：`arguments:{"action?":"list"|"register"|"heartbeat"|"lock"|"unlock"|"unregister", id?, name?, paths? (逗号串), cursor?, status?, format?:"json"}`；
     * `workflow`：`arguments:{"action":"start"|"advance"|"status"|"stop", mode?:"block"|"warn"}`；
     * `proxy_enable` / `proxy_disable`：**必填 `arguments:{"slug":"..."}`**。
3. **MCP 原生 Resource（13 个）与 Prompt（3 个）**：
   * **Resource URI 全表**：
     `gortex://workspace`、`gortex://repos`、`gortex://active-project`、`gortex://index-health`、`gortex://stats`、`gortex://schema`、`gortex://report`、`gortex://session`、`gortex://guide`（支持按章读取 `gortex://guide/{topic}`）、`gortex://audit`、`gortex://surprises`、`gortex://god-nodes`、`gortex://questions`。
   * **Prompt 列表**：
     `pre_commit`（`{"scope":"unstaged","repo?":"..."}`）、`safe_to_change`（**必填 `{"ids":"..."}`**，按字符串传入如逗号串）、`orientation`。

```json
call_mcp_tool(ServerName="gortex", ToolName="response", Arguments={"operation":"grep","arguments":{"pattern":"error","response_id":"resp-01","context":2}})
call_mcp_tool(ServerName="gortex", ToolName="session", Arguments={"operation":"planning_mode","arguments":{"mode":"editing"}})
call_mcp_tool(ServerName="gortex", ToolName="session", Arguments={"operation":"subscribe","channel":"diagnostics","arguments":{}})
```

---

# 【执行遵从性与 Antigravity 零闪断自查清单】 (Compliance Checklist)
1. [ ] **【内外层容器与 `session` 隔离】**：10 个平铺门面（含 `analyze` 用 `kind`）外层 `Arguments` 是否未套 `"arguments"`？8 个冷门面是否均封装在内层 `"arguments"` 中？`session` 是否绝未传顶层 `output`（且 6 项内层操作未传 `require_exact`/`require_fresh`/`wait_deadline`）？
2. [ ] **【精简开局探测】**：是否已通过 `workspace.repos`/`info`（及按需 `active_project`/`index`）确认工作区模式、仓库状态与目标 `<repo>` 前缀？
3. [ ] **【17 伪选键白名单与双通道】**：是否绝未向黑名单操作（`read.file`/`source`/`symbols`/`editing_context`、`edit.file`/`symbol`/`write`/`batch`、`refactor.rename`/`delete`、`change.tests`/`guards`/`impact`、`explore.task`/`context`/`localize`、`trace.path`/`flow`/`taint`/`cfg`/`graph`、`analyze.architecture`/`coupling`/`churn`/`co_change` 等）传入 `repo`/`root`/`cwd`/`dir`/`workspace`/`project` 等 17 个键？子路径前缀 `options.path`/`path_prefix` 是否零 `<repo>/` 拼接？
4. [ ] **【`explore.localize` / `new_user_task` / `read.file` 三大防闪锁】**：若调用 `explore.localize`，是否同时传了顶层 `task` 与 `options.task`？`options.new_user_task` 是否仅在新提问首次对应调用 `explore.task`/`localize` 或 `read.file` 使用布尔值？`read.file` 对脱敏配置文件开 `physical_evidence` 时是否同时传了 `allow_secrets: true`？读取符号 ID 时是否用了完整 `<repo>/file::Symbol` 避免短名歧义？
5. [ ] **【`target` 单属性、`view` 与字符串类型】**：所有 `target` 是否严格仅含 1 个合法键（禁 `{}`、禁 `path`），无 `target` 操作（含 92 个无 `target` 的 `analyze` 与用 `options.path` 的 `read.artifact`）是否完全省略？`change.contract` 的 `start_line`/`end_line` 是否放在 `options`（非 `source`）？`remember`/`recall` 的 `symbol_ids`/`file_paths`/`tags`/`symbols` 是否传逗号分隔字符串而非数组？`publish_review.post` 仅预演时是否显式传了 `"dry_run": true`？
6. [ ] **【精确 `output` 与切片硬锁】**：`[JSON+MB]` 是否设 `max_bytes:8400`（`winnow` 8250），`[JSON]` 是否未附带 `max_bytes`，`read.file` 是否仅传 `output:{max_bytes:8400}` + `options:{offset, limit<=150, max_chars?}`（无 `format`），`read.editing_context` 需要折叠函数体时是否显式设 `compress_bodies: true`？
7. [ ] **【批量优先路由、单次 `match` 与分级降级】**：涉及 `>=2` 项独立编辑时是否最高优先走 `edit.batch`（下限 `2` 项、推荐 `2~8` 项、硬上限 `10` 项且 `<4~5 KB`，超量按批串行拆分，默认必带 `output:{"compact":true}` 且不混入 `match`/`target`）？仅 `1` 处局部修改或单文件单组 `replace_all` 时是否优先走 `match`（`edit.file`/`edit.symbol`）？新建文件用 `edit.write`、重命名与批改共存时先 `refactor.rename` 后 `edit.batch`？批量连续出错第 3 次是否立即缩减批量且不反弹（`2~3` 项最低档则提前切 `match`），连续第 5 次是否已强制切换至 `match`（同文件串行）？遇 `tool_blocked_by_mode` 且当前任务允许改码时才用 `session.planning_mode` 解锁（只读任务立即停写）？
8. [ ] **【核验闭环、防溢出与 `capabilities` 首错停猜】**：源码改后（尤其 `edit.batch` 无内建语法门禁）是否完成 `change.detect` → `change.diagnostics`（按修改源码文件；无 LSP 时原生构建兜底；纯文档/配置免跑）→ `change.tests` 并跑原生测试？若出现 `output.txt` 是否只用 `response` 提炼？一旦发生任何参数错误是否立即停止猜参数并查一次 `capabilities(domain, operation, detail:"schema")`（同会话不重查）？
