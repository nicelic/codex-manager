# Gortex 统一架构全景全生命周期规范契约 (Unified Gortex MCP Specification - v601 全量补齐与自省优化版)

> 本规范基于在线 `capabilities` 实测（对齐 Gortex `facade-v1` 全量 **21 个门面、248 个操作、13 个 Resource 与 3 个 Prompt**）修订，完整保留 **L0 ~ L3 四级位阶体系**、补齐 `-600` 遗漏与压缩缺口、修正 `max_bytes` / `format` 容器偏差，并将 `capabilities` 严格收敛为**真兜底机制**。

---

## 【命令排版语法、容器归属与通用缩写】

1. **候选符号 `"|"` 与区间 `"~"`**：`"|"` 表示排版多选一，`"~"` 表示闭区间整数范围（如 `limit: 25~50`）。调用时**严禁原样照抄 `|` 或 `~`**，必须选定单一合法值或充沛整数。
2. **容器归属（严格隔离，严禁混写）**：
   * **普通平铺门面（10 个，9+1）**：`explore`、`search`、`read`、`edit`、`refactor`、`change`、`relations`、`trace`、`review`（共 9 个）顶层平铺 `operation`、`target`/`source`/`to`/`context`/`guard`/`changes`、`options`、`output`；**`analyze` 顶层操作字段固定为 `kind`（不是 `operation`）**；均严禁外包 `arguments`/`params`。
   * **冷门面（8 个）**：`workspace`、`workspace_admin`、`overlay`、`remember`、`recall`、`response`、`pr`、`publish_review` 所有业务参数**必须且仅能封装在 `arguments: { ... }` 内**（仅 `workspace_admin` 的 `blame`/`coverage`/`sql_rebuild`/`temporal_verify` 另可在顶层接受 `output`）。
   * **会话与自省门面（3 个）**：
     * **`session`**：顶层平铺 `operation`，其余参数按操作分为两类——`subscribe`/`unsubscribe` 的 `channel` 位于**顶层**；其余 6 个操作（`agents`/`cursor`/`planning_mode`/`proxy_enable`/`proxy_disable`/`workflow`）参数置于 **`arguments`** 内（详见第零节/第十一节）。
     * **`capabilities`**：顶层平铺 `domain`、`operation`、`detail`（`"operations"` | `"schema"`）。
     * **`ask`**：当前表面 `operations: []`（研究代理问答门面）。
3. **跨门面可选 `view`（工作树 / Git 版本视角切换）**：除 `ask`/`capabilities` 外，所有门面顶层均可选传 `view`，不传或 `{"kind":"auto"}` 表示使用当前会话工作区；支持：
   * `{"kind":"base","graph_id":"..."}` / `{"kind":"worktree","checkout_id":"..."}` / `{"kind":"worktree","path":"绝对路径"}` / `{"kind":"git_ref","value":"refs/heads/main"}` / `{"kind":"commit","value":"<40位小写sha>"}`。
4. **响应标记约定**（下文表格严格区分是否支持 `max_bytes`，严禁向不支持 `max_bytes` 的接口硬塞该字段）：
   * **`[JSON+MB]`**：顶层传 `"output": { "format": "json", "max_bytes": 8400 }`（`search.winnow` 为 `8250`）；
   * **`[JSON]`**：顶层仅传 `"output": { "format": "json" }`（该操作 `output` 不接受 `max_bytes`）；
   * **`[冷JSON+MB]`**：`arguments` 内传 `"format": "json", "max_bytes": 8400`；
   * **`[冷JSON]`**：`arguments` 内仅传 `"format": "json"`（不接受 `max_bytes`）；
   * **未标注 `format` 的写/状态操作**：严禁硬塞 `format`。

---

# 【L0 级：全局统领宪法与多项目底座】（最高强制位阶 · 每次必读）

### 铁律 1：外部文档只读豁免、Gortex 闭环与原生跳出边界
1. 位于代码仓库外部系统目录的规范文件（如本文件）属于初始环境法定豁免文件，**必须且仅能用宿主原生只读工具（`read_file` 等）读取**，严禁调 Gortex 读取。
2. 预读完成后，受纳管工程内所有源码、测试代码与配置文件的探测、检索、阅读、修改、验证与审查，**100% 走 Gortex MCP 闭环**。
3. 仅两类情况允许原生工具：① 用户明确输入“使用原生工具”时单次跳出，完成后立即返回闭环；② **Git/GitHub 运维及实际运行/调试测试进程**（`git`、`cargo test`、`go test`、`pytest`、`npm test` 等）可直接用原生命令，但**严禁借测试之名用原生工具检索、阅读或修改工程/测试文件**。

### 铁律 2：寻址定位 vs 范围过滤【双通道路径与零拼接终极铁律】
* **通道 A（全局定位寻址器：`target.file` / `target.symbol` / `changes[].path`）**：用于 `edit.*`、`read.*`、`relations.*`、`trace.*`、`change.*`。所有路径与符号 ID 必须原样复制自 `workspace`/`search`/`explore` 返回值；**多仓库及 `unbound_global` 环境下，合法且必须携带 `repo` 前缀**（如 `"repo-main/src/api/handler.rs::fn"`）。
* **通道 B（仓内子路径过滤器：`options.path` / `options.path_prefix`）**：`search.text`/`symbols`/`files` 用 `options.path`，`search.winnow`/`ast` 用 **`options.path_prefix`**。**绝对严禁在 `path` / `path_prefix` 中拼接 `repo` 前缀**（底层 `pathMatchesAnyPrefix` 按仓内相对路径比对，拼了必搜空！）；仓库限定必须单独使用 `options.repo`（值取自 `workspace.repos`/`info`）。

### 铁律 3：开局四步前置探测状态机 (Probe-Before-Analyze)
接入新任务或跨仓排查前，**必须按序完成探测后才可分析代码**：
1. **边界探测**：调 `workspace(operation="repos", arguments={"format":"json","max_bytes":8400})`（或 `workspace(operation="info", arguments={"format":"json","max_bytes":8400})`）获取已 track 仓库清单与绑定模式（`workspace` vs `unbound_global`）；
2. **归属探测**：调 `workspace(operation="active_project", arguments={"format":"json","max_bytes":8400})` 确认当前主工程；若为 `unbound_global` 或目标与默认工程不符，按当前目录匹配 `members[].path` 锁定目标 `repo` 前缀并按需切域；
3. **健康诊断**：调 `workspace(operation="index", arguments={"format":"json","max_bytes":8400})` 核验解析就绪度与健康状态；
4. **准入**：严禁未经探测盲搜，严禁臆测“未索引”退回宿主工具；若 `repos`/`members` 均为空或目标确未纳管，只作一次确认并如实报告或按需 `workspace_admin.track`，严禁死循环重试。

### 铁律 4：多项目双穿透与主工程切域法则
* **只读轻量穿透（免切项目）**：跨仓查符号用 `workspace(operation="project", arguments={"project":"repo-core","query":"..."})`；跨仓精读用带外部 `repo/` 前缀的 `read.file`。
* **深度操作切域**：对外部仓执行依赖主工程上下文的深度改动或测试分析前，先调 `workspace_admin(operation="set_active_project", arguments={"project":"repo-core"})`；**严禁向未声明 `repo`/`project` 的门面（`edit.*`、`refactor.rename`/`move`/`delete`、`change.tests`/`guards`）注入 `repo`/`project`**。

### 铁律 5：强类型签名、JSON 强制输出与读取大小硬锁
1. **显式操作名**：顶层必传 `"operation"`（`analyze` 顶层必传 `"kind"`），严禁无头请求。
2. **JSON 强制输出硬锁（严格匹配 Schema，禁止错塞字段）**：
   * 支持 `output.max_bytes` 的查询/分析（`[JSON+MB]`）必传 `output: { format: "json", max_bytes: 8400 }`（`search.winnow` 设为 `8250`）；
   * 仅支持 `output.format` 的查询/分析（`[JSON]`，如 `search.text`/`files`/`artifacts`/`completion`、`explore.closure`/`suggest`、`read.artifact`、`relations.references`/`declaration`、`trace.walk`/`graph`、`change.contract`/`ranges`、`refactor.delete`、`analyze.architecture`/`co_change`/`inspections`/`citation`/`communities`/`extraction`/`inspection_catalog`/`knowledge_gaps`/`replay`/`surprising_connections`）**只传 `output: { format: "json" }`，严禁塞 `max_bytes`**；`analyze.coupling`/`churn` 支持 `output: { format: "json", limit }`（不支持 `max_bytes`）；
   * 支持 `[冷JSON+MB]` 与 `[冷JSON]` 的冷门面将对应字段置于 `arguments` 内（`workspace_admin.blame`/`coverage`/`sql_rebuild`/`temporal_verify` 置于顶层 `output`）；
   * 未声明 `format` 的写/状态操作（`edit.batch`/`symbol`/`file`/`write`/`apply_overlay`/`scaffold`、`refactor.rename`/`move`/`inline`/`fix_all`/`apply_code_action`、`overlay.*`、`change.detect`/`preview`/`simulate`/`diagnostics`/`code_actions`/`receipt`/`tests`/`verify`/`edit_plan`/`pattern`/`overlay_branches`/`overlay_state`/`compare_branches`/`compare_overlay`、`workspace.project`/`proxy`/`scopes`、`workspace_admin.set_active_project`/`track`/`untrack`/`index`/`save_scope`/`delete_scope`、`session.planning_mode`/`proxy_enable`/`proxy_disable`/`subscribe`/`unsubscribe`/`workflow`、`response.grep`/`slice`/`peek`/`stats`、`relations.import_path`、`explore.task`/`localize`/`plan`、`analyze.health`/`lint`/`recent_changes`）**严禁硬塞 `format`**。
3. **读取大小与切片硬锁**：
   * 绝对严禁把 `max_bytes` 放进 `options`；
   * `read.file` **严禁无范围盲读整文件**！必传 `options: { offset: 行号, limit: 行数 }`（**硬锁 `limit <= 150`**，推荐 30~80；即使小文件也须显式传 `offset: 1, limit: 150`）+ `output: { max_bytes: 8400 }`；`explore` 优先设 `options.token_budget: 1600~4000`。

### 铁律 6：单轮批量原子交付与写故障退避自愈
1. **单轮交付**：多处或跨文件代码修改**必须合并入单次 `edit(operation="batch", changes: [...])` 原子提交**，严禁切香肠试探（单文件单处确定性文本替换用 `edit.file`，按符号整体替换用 `edit.symbol`，新建/整写用 `edit.write`）。
2. **写故障三步退避**：遇到 `ambiguous_match`、行号漂移或超时状态未知时严禁盲目重放 → 先 `change.detect`（**超时未收确定回执必先查 `change.receipt`** 核实是否已落盘）→ 再 `read.file(offset, limit <= 150)` 对齐当前上下文基线后重新提交。

### 铁律 7：大输出防溢出隔离守卫 (Anti-Spill)
触发截断落盘提示（如 `output.txt`）时，**绝对严禁调用任何原生工具（`read_file`/`cat`/`Get-Content`）直接读取该 `output.txt`**！源头靠 `max_bytes: 8400`、`limit <= 150` 与 `compress_bodies: true` 预防；命中 Gortex 缓冲句柄时，仅限使用 `response(operation="grep"|"slice"|"peek"|"stats", arguments={...})` 分片提炼（完整参数见第十一节）。

### 铁律 8：直调优先、本地 Schema 禁令与 `capabilities` 真兜底红线
1. **禁用本地 Schema**：严禁使用原生工具读取本地 `mcp/gortex/*.json`。
2. **已知接口严禁前置预检（防高频浪费）**：本文档已列明操作名与参数结构的门面/操作，**必须直接按本文档构造参数发起调用，严禁在首次调用或写操作前先行调用 `capabilities` 预查 Schema**，消除多余轮次与大载荷上下文消耗。
3. **仅限三类真兜底场景触发 `capabilities`（禁止常规滥用）**：
   * **场景 A（报错自愈）**：按本文档直调后，服务端真实返回参数校验报错（`invalid_argument` / 未知字段 / 缺必填项 / 容器错误）时，调用一次 `capabilities({"domain":"<门面>","operation":"<操作>","detail":"schema"})` 获取 `request_shape` 纠偏；
   * **场景 B（未收录盲区）**：需要使用本文档未展开细则的新版本接口，或遇未知子操作名需确认清单时，先用轻量调用 `capabilities({"domain":"<门面>","detail":"operations"})` 查目录，确有需要再查 `detail:"schema"`；
   * **场景 C（同会话去重）**：同一个 `domain + operation` 在当前会话一旦通过 `capabilities` 查过一次，**本会话内严禁再次重复查询**。

### 铁律 9：默认按文档执行与冲突裁决五大位阶
**未发生真实报错或行为冲突时，默认直接以本文档契约为执行准绳，不得借高位阶之名事前常规调用 `capabilities`。** 仅当运行时实际结果与本文档发生分歧或报错时，严格自顶向下裁决：**1. 工具真实运行时响应（报错 / guard / completion）** > **2. 报错或盲区触发的 `capabilities` 即时 Schema** > **3. 物理文件与 Git 树真实状态** > **4. Gortex 服务端源码实现（当工作区已纳管时）** > **5. 本文档规则**。

---

# 【L1 级：日常开发与多项目协同黄金闭环】（高频 · 协-查-读-写-验五大核心中枢）

## 零、 工作区与多项目协同中枢 (`workspace` [9] / `workspace_admin` [14] — 均须 `arguments: {}`)
| 触发场景 | 操作 | 必填参数与容器规则 |
| :--- | :--- | :--- |
| 列已 track 仓库 / 绑定与成员身份 | `workspace(operation="repos"\|"info")` | `[冷JSON+MB]`（严禁传 `repo`） |
| 查当前主工程 / 图谱健康 / 节点边数 | `workspace(operation="active_project"\|"index"\|"graph")` | `[冷JSON+MB]`（`index` 支持 `compact?:true`） |
| 跨项目免切只读检索外部仓库符号 | `workspace(operation="project")` | 必填 `arguments: { project, query, limit?: 20 }`（无 `format`） |
| 列出可见 checkout 家族与工作树路由 | `workspace(operation="checkouts")` | `[冷JSON+MB]`, 可选 `arguments: { family?, fields?, max_tokens? }` |
| 列出已保存仓库作用域 / 远程代理状态 | `workspace(operation="scopes"\|"proxy")` | `arguments: {}`（无 `format`） |
| 热切换当前会话主工程上下文 | `workspace_admin(operation="set_active_project")` | 必填 `arguments: { project }`（非文件路径，无 `format`） |
| 注册 / 注销仓库 | `workspace_admin(operation="track"\|"untrack")` | `track`: `{path, name?, as_worktree?, force?}`; `untrack`: `{path, confirm:true}` |
| 保存 / 删除可复用多仓子作用域 | `workspace_admin(operation="save_scope"\|"delete_scope")` | `save_scope`: `{name, repos, paths?, description?}`; `delete_scope`: `{name}` |
| 记录或查询上下文检索学习反馈 | `workspace_admin(operation="feedback")` | `arguments: { action:"record"\|"query", task?, useful?, not_needed?, missing?, tool_source?, top_n?, compact? }`, `[冷JSON]` |

```json
workspace({"operation":"project","arguments":{"project":"repo-core","query":"ConnectionPool","limit":20}})
workspace_admin({"operation":"set_active_project","arguments":{"project":"repo-core"}})
```

---

## 一、 检索与探索中枢 (`search` [7] / `explore` [9])
**核心门禁**：
1. **双探针原则**：排查未知业务逻辑，首动并发 `search.text`（字面量）+ `search.winnow(min_fan_in: 2)`（拓扑雷达），或先用 `explore.task`/`localize`/`context` 定界。
2. **Vue/React SFC 法则**：`.vue`/`.tsx` 模板事件绑定与内部闭包不在全局导出索引中，**必须首选 `search.text`**，严禁首用 `symbols` 搜空。
3. **拓扑升格死锁**：若 `search.winnow` 未命中，**严禁直接 `read.file` 裸读或 `edit.batch` 盲改**；必须从 `search.text` 命中行提取外层真实符号名，二轮跑 `search.winnow(query="真实符号", min_fan_in: 0)` 获取拓扑后方可进入读写。
4. **禁 `search.files` 投机令（含例外）**：业务逻辑排查禁首动 `search.files`；**仅当确认文件存在、收集清单或用户明确给出非源码配置/文档路径时允许直接 `search.files`**（`query` 禁通配符，`options.glob` 才支持通配符）。
5. **`new_user_task` 守则**：仅在由新用户请求触发的**首次** `explore`（`task`/`localize`）调用设 `options.new_user_task=true`（注：部分门面由服务端自动跟踪），重试或续查时不得再传。

| 触发场景 | 操作 | 关键参数与精确 `output` 门禁 |
| :--- | :--- | :--- |
| 报错日志、字面量、Vue/React 模板事件 | `search(operation="text")` | `query?`, `options: { repo?, path?, project?, scope?, workspace?, regexp?, limit: 35~80 }`, **`[JSON]`（无 `max_bytes`！）** |
| 探索主干入口 / 补齐真实符号依赖拓扑 | `search(operation="winnow")` | `query?`, `options: { repo?, path_prefix?, project?, ref?, language?, kind?, community?, min_fan_in: 0\|2, min_fan_out?, min_churn?, is_test?, test_role?, limit: 25~50, paginate?, cursor? }`, `output: { format:"json", max_bytes:8250, compact?, fields? }` |
| 查符号/文档/内容块定义（`assist` 已固定 `off`） | `search(operation="symbols")` | 必填 `query` + `options: { repo?, path?, project?, scope?, workspace?, ref?, kind?, flavor?: "struct\|class\|component...", corpus?: "code\|docs\|content\|all", expand?: "both\|equivalence\|llm\|off", query_class?: "auto\|symbol\|concept\|path\|signature", vocab_anchored?, max_per_file?, limit: 28~70, paginate?, cursor?, debug? }`, **`[JSON+MB]`** |
| 语法树模式或内置安全/卫生规则扫描 | `search(operation="ast")` | `query?`, `options: { repo?, path_prefix?, project?, ref?, language?: "rust\|ts\|go...", detector?, exclude_tests?, min_fan_in_of_enclosing_func?, limit: 28~50 }`（`detector` 与 `pattern` 互斥；无参数传空 `detector:""` 可列规则目录）, **`[JSON+MB]`** |
| 确认存在 / 按片段或后缀收集文件列表 | `search(operation="files")` | `query?`（禁`*`/`?`）, `options: { repo?, path?, project?, scope?, workspace?, glob?: "*.rs", fuzzy?: false, limit: 70~220 }`, **`[JSON]`** |
| 查 `.gortex.yaml::artifacts` 非代码知识节点 | `search(operation="artifacts")` | `query?`, `options: { kind?: "schema\|api\|infra\|doc", limit: 20 }`, **`[JSON]`** |
| 1 跳图补全扩展候选符号 | `search(operation="completion")` | `query?`, `options: { edge_kinds?: "calls,references", seed_limit?, max_seed_expansion?, limit: 20 }`, **`[JSON]`** |
| 诊断或持续开发用的非终止领域上下文 | `explore(operation="task")` | 必填顶层 `task`, `path?`, `options: { path?, max_symbols?: 10, token_budget: 1600 }`（无 `output`） |
| 定位即止的终端证据查找 | `explore(operation="localize")` | 必填 `options: { task, path?, max_symbols?: 10, token_budget: 1600 }`, 顶层 `task?`, `path?`（无 `output`） |
| 单次组装任务最小上下文（支持分级/增量） | `explore(operation="context")` | 必填顶层 `task`, `path?`, `options: { path?, project?, scope?, entry_point?, max_symbols?: 5, fidelity?: "flat\|graded", token_budget?: 8000, delta_from?, if_none_match?, estimate?, include_call_paths?, include_flows?, include_confidence? }`, **`[JSON+MB]`** |
| 从一组种子文件/符号构建依赖闭包上下文包 | `explore(operation="closure")` | `path?`, `options: { files?: "a.rs,b.rs", symbols?: "id1,id2", edge_kinds?, max_depth?: 6, max_nodes?: 400, rank?: "distance\|proximity", token_budget?: 4000, project?, ref?, scope?, workspace? }`, **`[JSON]`** |
| 单次调用获取代码库全局概览 / 约 500 Token 摘要 | `explore(operation="outline"\|"wakeup")` | `outline`: `options:{}`, **`[JSON+MB]`**; `wakeup`: `options: { max_tokens?: 500, top_communities?: 4, top_entry_points?: 5, top_hotspots?: 5 }`, `output: { format: "json"\|"markdown" }` |
| 开局行动路由 / 预测下一步上下文 / 冷启动推荐 | `explore(operation="plan"\|"prefetch"\|"suggest")` | `plan`: 必填 `options: { task, max_calls?: 4 }`（无 `output`）; `prefetch`: `task?`, `options: { recent_symbols?, include_source?, limit?: 10, max_tokens?, paginate?, cursor? }`, **`[JSON+MB]`**; `suggest`: `options: { limit?: 8 }`, **`[JSON]`** |

```json
search({"operation":"text","query":"handle_save_user","options":{"repo":"repo-main","path":"frontend/src/views","limit":50},"output":{"format":"json"}})
search({"operation":"winnow","query":"forward_handler","options":{"repo":"repo-main","path_prefix":"src/api","min_fan_in":0,"limit":30},"output":{"format":"json","max_bytes":8250}})
search({"operation":"symbols","query":"UserReq","options":{"repo":"repo-main","path":"models","flavor":"struct","limit":40},"output":{"format":"json","max_bytes":8400}})
explore({"operation":"context","task":"定位登录鉴权失败原因","options":{"path":"src/auth","fidelity":"graded","token_budget":4000},"output":{"format":"json","max_bytes":8400}})
```

---

## 二、 源码精读中枢 (`read` [7])
**核心门禁**：① 拿到符号 ID **必须优先 `read.symbols`/`read.source`**，严禁退化读整文件（仅需查看周边注释、非导出模板或编辑所需精确多行锚点时，方可用局部 `read.file` 补读）；② `read.file` **强锁 `options: { offset, limit }`（`limit <= 150`）+ `output: { max_bytes: 8400 }`**（大源码概览优先 `read.summary` 或 `context: { compress_bodies: true }`）；③ `read.history` 返回**当前会话**修改符号计数（非 Git 历史；查 Git 历史/热点用 `workspace_admin.blame` / `analyze(kind="churn")`）。

| 触发场景 | 操作 | 关键参数与精确 `output` 门禁 |
| :--- | :--- | :--- |
| 批量精读 2~8 个候选符号完整实现 | `read(operation="symbols")` | `target: { symbols: ["repo/...::fn1", ...] }`（或 `{symbol}`）, `context?: { context_lines?: 3 }`, `options?: { include_source?: true, if_none_match? }`, **`[JSON+MB]`** |
| 精读单个函数/类源码（支持主体折叠省 Token） | `read(operation="source")` | `target: { symbol: "id" }`, `context?: { compress_bodies?: false, context_lines?: 3, max_lines? }`, `options?: { allow_secrets?: false, if_none_match? }`, **`[JSON+MB]`** |
| 局部非符号文件/配置/锚点切片 | `read(operation="file")` | `target: { file: "repo/..." }`, 必传 `options: { offset: N, limit: <=150, keep?, fidelity_globs?, allow_secrets?, if_none_match?, max_chars?, physical_evidence?, digest? }`, `context?: { compress_bodies?, max_lines? }`, **`output: { max_bytes: 8400 }`（无 `format`）** |
| 文件全部符号大纲（免读实现） | `read(operation="summary")` | `target: { file: "repo/..." }`, `options?: { repo?, project?, scope?, workspace?, ref?, max_tokens?, if_none_match? }`, **`[JSON+MB]`**（支持 `compact?:true`） |
| 改前装配局部符号与直连上下游 | `read(operation="editing_context")` | `target: { file\|symbol }`, `context?: { compress_bodies: true }`, `options: { detail?: "brief"\|"full", keep?: "Sym1,Sym2", fidelity_globs?, max_tokens?, if_none_match? }`, **`[JSON+MB]`** |
| 查当前会话修改足迹 | `read(operation="history")` | `target?: { symbol }`, `options: {}`, `output?: { compact?: false }`（无 `format`/`max_bytes`） |
| 读 `.gortex.yaml::artifacts` 登记的非代码产物 | `read(operation="artifact")` | `target: { artifact }` 或 `options: { path }`, **`[JSON]`（上限 64 KiB，无 `max_bytes`）** |

```json
read({"operation":"symbols","target":{"symbols":["repo-main/src/a.rs::fn1","repo-main/src/b.rs::fn2"]},"output":{"format":"json","max_bytes":8400}})
read({"operation":"source","target":{"symbol":"repo-main/src/a.rs::fn1"},"context":{"compress_bodies":true},"output":{"format":"json","max_bytes":8400}})
read({"operation":"file","target":{"file":"repo-main/frontend/src/views/UserView.vue"},"options":{"offset":120,"limit":40},"output":{"max_bytes":8400}})
```

---

## 三、 修改与重构中枢 (`edit` [10] / `refactor` [6])
**核心门禁**：多处/跨文件必用单轮 `edit.batch`；`edit.symbol` 必填顶层 `match`（须与磁盘精确匹配）；`edit.file`/`write`/`symbol` 均有树解析语法门禁（拒绝引入新语法错，可用 `options.allow_parse_errors:true` 覆写）；`refactor.move` 与 `refactor.inline` **仅支持 Go 语言**；高风险改动先 `dry_run: true`；遇到写入异常严格执行铁律 6 三步退避。

| 触发场景 | 操作 | 必填参数、可选开关与容器细节（**除明确标注外均无 `format`**） |
| :--- | :--- | :--- |
| 单/跨文件多处原子批量编辑 | `edit(operation="batch")` | `changes: [{op:"edit_file", path, old_string, new_string, replace_all?:false} \| {op:"edit_symbol", id, old_source, new_source} \| {op:"move_file", source, destination, expected_sha256?} \| {op:"delete_file", path, expected_sha256?}]`, `dry_run?: false`, `options?: { transaction_id?, status_only? }`, `output?: { compact? }` |
| 按符号 ID 精确替换源码 | `edit(operation="symbol")` | `target: { symbol }`, 顶层 `match` + `replacement`, `dry_run?: false`, `options?: { base_sha?, mutation_id?, physical_evidence?, digest? }` |
| 单文件单处文本替换 | `edit(operation="file")` | `target: { file }`, 顶层 `match` + `replacement`, `dry_run?: false`, `guard?: { expected_occurrences? }`, `options?: { replace_all?, base_sha?, mutation_id?, physical_evidence?, digest?, allow_parse_errors? }` |
| 新建文件或整文件覆写 | `edit(operation="write")` | `target: { file }`, 顶层 `content`, `dry_run?: false`, `options?: { base_sha?, mutation_id?, physical_evidence?, digest?, allow_parse_errors? }` |
| 内存 overlay 分支合并落盘 | `edit(operation="apply_overlay")` | 无 `target`！必填 `options: { from: "分支名", into?: "main", force?: false }` |
| 基于已有符号模式生成脚手架与测试桩 | `edit(operation="scaffold")` | 必填 `target: { symbol }` + `options: { new_name }`, `dry_run?: false`, `output?: { compact? }` |
| 生成活文档 / 导出图谱 / 生成 Wiki / 打包 Skill | `edit(operation="docs"\|"export_graph"\|"wiki"\|"skill")` | `docs`: `options:{include?, output_path?, path_prefix?, run_blame?, since?, top?, workspace?}`, `output:{format:"markdown"\|"json"}`; `export_graph`: `options:{kinds?, languages?, repo?, scope?, max_communities?, min_community?, no_synthetic?, output_path?, output_dir?}`, `output:{format:"cypher"\|"graphml"\|"mermaid"}`; `wiki`: `options:{output_dir?, repo?, project?, workspace?, max_communities?, min_community?, force?, wikilinks?, no_contracts?, no_docs?, no_processes?}`, `output:{format:"markdown"\|"html"}`; `skill`: `options:{directory?, skill_name?, description?, output_dir?, include_patterns?, ignore_patterns?, compress?, max_references?}`, `dry_run?:false`, `output:{format:"json"\|"gcx"\|"toon"}` |
| 全局协同重命名 | `refactor(operation="rename")` | 必填 `target: { symbol }` + 顶层 `new_name`, `dry_run?: false`, `options?: { allow_parse_errors? }` |
| 一键收敛 LSP 诊断 / 应用单个 CodeAction | `refactor(operation="fix_all"\|"apply_code_action")` | `fix_all`: `target:{file}`, `options?: { repo?, kinds?, diagnostic_timeout_ms?, max_actions_per_iter?, max_iterations? }`; `apply_code_action`: `target:{file}`, 必填 `options: { index, start_line?, start_char?, end_line?, end_char?, only?, repo? }` |
| 搬移符号 / 安全删除 / 内联展开 | `refactor(operation="move"\|"delete"\|"inline")` | `move`(仅Go): `target:{symbol}` + 顶层 `destination`, `dry_run?:false`, `options?:{target_package?}`; `delete`: `target:{symbol}`, `dry_run?:false`, `options?:{cascade?:"off"\|"preview"\|"apply", cascade_into_tests?, force?, propagate?}`, **`[JSON]`**; `inline`(仅Go): `target:{symbol}`, `dry_run?:false`, `options?:{delete_after?:true}` |

```json
edit({"operation":"batch","changes":[{"op":"edit_file","path":"repo-main/src/a.rs","old_string":"old","new_string":"new","replace_all":false}],"dry_run":false})
edit({"operation":"file","target":{"file":"repo-main/src/a.rs"},"match":"old_line","replacement":"new_line","dry_run":false})
edit({"operation":"write","target":{"file":"repo-main/src/new_file.rs"},"content":"pub fn hello() {}\n","dry_run":false})
edit({"operation":"symbol","target":{"symbol":"repo-main/src/a.rs::fn1"},"match":"fn fn1() { old }","replacement":"fn fn1() { new }","dry_run":false})
refactor({"operation":"rename","target":{"symbol":"repo-main/src/a.rs::old_fn"},"new_name":"new_fn","dry_run":false})
```

---

## 四、 变更核验与质量中枢 (`change` [19])
**核心门禁**：源码修改落盘后严禁假设正确，必走 **`change.detect` → `change.diagnostics` → `change.tests`** 闭环（并结合原生测试命令验证；仅纯文档/配置修改免强跑 `diagnostics`/`tests`，未配置 LSP 时以实际编译与单测兜底）。

| 触发场景 | 操作 | 必填参数与真实语义 |
| :--- | :--- | :--- |
| 盘点未提交变动与受影响符号 | `change(operation="detect")` | `source?: { scope: "unstaged"\|"staged"\|"all"\|"compare" }`, `options?: { repo?, base_ref?, limit?: 50, offset? }`, `output?: { summary_only? }` |
| 影子沙箱预演单个 / 多步 LSP WorkspaceEdit | `change(operation="preview"\|"simulate")` | `preview`: 必填 `source: { workspace_edit: "<JSON>" }`, `options?: { diagnostics?: true, diagnostics_timeout_ms?, inherit_overlay? }`; `simulate`: 必填 `source: { steps: "<WorkspaceEdit[] JSON>" }`, `options?: { diagnostics?: true, diagnostics_timeout_ms?, inherit_overlay?, stop_on_error?: true }` |
| 提取 LSP 诊断 / 查询可用 CodeAction | `change(operation="diagnostics"\|"code_actions")` | `diagnostics`: 必填 `source: { file }`, `options?: { repo?, project?, wait?: false, timeout_ms? }`; `code_actions`: 必填 `source: { file }`, `options?: { start_line?, start_char?, end_line?, end_char?, only?, repo?, project? }` |
| 查写操作落盘回执 | `change(operation="receipt")` | `options: { receipt \| mutation_id }` 或 `source: { path }` |
| 受影响测试 / 架构守卫 / 爆炸半径 | `change(operation="tests"\|"guards"\|"impact")` | 必填 `target: { symbol \| symbols:[...] }`（`impact` 亦支持 `target:{file}`）；`tests`: `options?:{depth?:3}`, `output?:{compact?}`; `guards`: **`[JSON+MB]`**(`compact?`); `impact`: **`[JSON+MB]`**(`limit?, offset?, summary_only?`) |
| 改前全流水线风险合同裁决 (`allow\|warn\|refuse`) | `change(operation="contract")` | `target: { symbol\|symbols }` 或 `source: { scope?, base?, path?, start_line?, end_line?, ranges?, workspace_edit? }`, `options?: { risk_gate?, lens?: "api", repo?, start_line?, end_line? }`, **`[JSON]`（无 `max_bytes`）** |
| 路由修改影响 / 编辑依赖序 / 代码模式 / 区间转符号 / 签名校验 | `change(operation="api_impact"\|"edit_plan"\|"pattern"\|"ranges"\|"verify")` | `api_impact`: `source:{file}` 或 `options:{route, repo?, project?, ref?}`, **`[JSON+MB]`**; `edit_plan`: `target:{symbol\|symbols}`, `options?:{depth?:3}`; `pattern`: `source:{symbols:[...]}`, `options?:{max_source_lines?:40}`, `output?:{max_bytes?}`; `ranges`: `source:{file\|ranges:[{file,start_line,end_line}]}`, `options?:{start_line?, end_line?}`, **`[JSON]`**; `verify`: 必填 `source:{changes:[{symbol_id, new_signature}]}`, `output?:{compact?}` |
| 查询 / 比对会话 overlay 状态与分支差异 | `change(operation="overlay_branches"\|"overlay_state"\|"compare_branches"\|"compare_overlay")` | `overlay_branches`/`overlay_state`: 仅 `operation`; `compare_branches`: `target:{symbol}` + `options:{a, b, kind:"find_usages\|get_callers\|get_call_chain\|get_dependencies\|get_dependents", depth?, limit?}`; `compare_overlay`: `target:{symbol}` + `options:{kind, depth?, limit?}` |

```json
change({"operation":"detect","source":{"scope":"unstaged"},"options":{"repo":"repo-main","limit":50}})
change({"operation":"diagnostics","source":{"file":"repo-main/src/a.rs"},"options":{"repo":"repo-main","wait":false}})
change({"operation":"tests","target":{"symbols":["repo-main/src/a.rs::fn1"]}})
```

---

# 【L2 级：深度拓扑与架构审计】（进阶 · 析-联-迹三大中枢）

## 五、 架构分析中枢 (`analyze` [96] — 顶层必用 `kind`，严禁写 `operation`！)
**核心门禁**：大规模重构或改公共类前先跑 `cycles` 与 `dead_code`；排查竞态用 `race_writes`。注意 `analyze` **不存在 `layers`**；大部分图谱全局分析不接受 `target`（仅 `co_change`、`why`、`def_use` 必填 `target`，`impact` 可选 `target`），通用作用域过滤用 `options: { repo?, project?, scope?, workspace? }`。

### 1. 高频核心架构分析（带专属参数说明）
| 触发场景 | `kind` 取值 | `options` 与精确 `output` 门禁 |
| :--- | :--- | :--- |
| 模块环路 / 过耦合热点 / 孤儿死代码 | `"cycles"` \| `"hotspots"` \| `"dead_code"` | `cycles`: 通用过滤; `hotspots`: `options:{threshold?}`; `dead_code`: `options:{include_variables?, include_fields?, include_constants?, include_cgo_exports?, include_linkname_targets?, skip_cross_repo_nodes?}` — **均配 `[JSON+MB]`** |
| 一键架构快照 / Leiden 社区 / 聚类算法对比 | `"architecture"` \| `"communities"` \| `"clusters"` \| `"louvain"` | `architecture`: `options:{path_prefix?, resolution?:"file\|package\|service\|system", top_communities?, top_entry_points?, top_hotspots?, top_processes?}`, **`[JSON]`**; `communities`: `options:{id?}` (无 `output`); `clusters`: `options:{algorithm?:"leiden\|louvain\|spectral", min_size?:3, resolution?:1.0}`, **`[JSON+MB]`**; `louvain`: **`[JSON+MB]`** |
| 包/社区耦合度与不稳定性 / Git 逻辑共变耦合 | `"coupling"` \| `"co_change"` | `coupling`: `options:{unit?:"package\|community", package_depth?:2, min_members?:1, path_prefix?, sort_by?}`, `output:{format:"json", limit?:50}`; `co_change`: 必填 `target:{file\|symbol}` + `options:{file_path?, symbol_id?, min_score?}`, `output:{format:"json", limit?:20}` |
| 综合变更影响分 / 多线程竞态写入 / Git 扰动热度 | `"impact"` \| `"race_writes"` \| `"churn"` | `impact`: 可选 `target:{file\|symbol}` + `options:{id?, ids?, path?}`, **`[JSON+MB]`**; `race_writes`: 通用过滤, **`[JSON+MB]`**; `churn`: `options:{kinds?:"function,method", min_commits?:1, path_prefix?, sort_by?}`, `output:{format:"json", limit?:100}` |
| 复杂度健康评级 / 多轴健康分 / 未测热点与重构提取值 | `"health"` \| `"health_score"` \| `"untested"` \| `"extraction"` | `health`: 通用过滤 (无 `output`); `health_score`: `options:{grade?:"d,f", min_score?, max_score?, min_axes?, roll_up?:"file\|repo"}`, **`[JSON+MB]`**; `untested`: `options:{file_prefix?, min_fan_in?}`, `output:{limit?:50}`; `extraction`: `options:{min_callers?, min_fan_out?, min_lines?, path_prefix?}`, `output:{format:"json", limit?:25}` |
| 规则巡检目录与运行 / LSP 外置 Lint / SAST 安全扫描 | `"inspection_catalog"` \| `"inspections"` \| `"lint"` \| `"sast"` \| `"hygiene"` \| `"named"` | `inspection_catalog`: `options:{category?}`, **`[JSON]`**; `inspections`: `options:{inspections:"all\|...", severity?, path_prefix?, max_per_inspection?}`, **`[JSON]`**; `lint`: `options:{path, language?, timeout_ms?}` (无 `output`); `sast`/`hygiene`: `options:{cwe?, detector?, language?, severity?, exclude_tests?, kinds_only?}`, **`[JSON+MB]`**; `named`: `options:{name?}`, **`[JSON+MB]`** |
| 符号存在动机溯源 / 故障回放 / 预检加边是否成环 | `"why"` \| `"replay"` \| `"would_create_cycle"` | `why`: 必填 `target:{symbol}` + `options:{id, scope?}`, `output:{format:"json", limit?:20}`; `replay`: `options:{anchor_symbol?, depth?:3, window_days?:30}`, `output:{format:"json", limit?:25}`; `would_create_cycle`: 必填 `options:{from_id, to_id}`, **`[JSON+MB]`** |
| 覆盖率缺口与目录汇总 / 代码所有权与陈旧度 / 克隆与执行流 | `"coverage_gaps"` \| `"coverage_summary"` \| `"ownership"` \| `"stale_code"` \| `"stale_flags"` \| `"clones"` \| `"processes"` | `coverage_gaps`: `options:{min_pct?:0, max_pct?:100, path_prefix?}`, **`[JSON+MB]`**; `ownership`: `options:{kinds?, min_symbols?, path_prefix?}`, **`[JSON+MB]`**; `stale_code`: `options:{older_than?:365, email?, kinds?}`, **`[JSON+MB]`**; `clones`: `options:{min_similarity?, dead_only?, path_prefix?}`, **`[JSON+MB]`**(`limit?`); `processes`: `options:{id?}`, **`[JSON+MB]`**; 其余均 **`[JSON+MB]`** |
| 代理配置审计 / 引用校验 / 知识盲区 / 异常边 / 近实时变更 / TODO / 契约 / 边界建议 | `"agent_config"` \| `"citation"` \| `"knowledge_gaps"` \| `"surprising_connections"` \| `"recent_changes"` \| `"todos"` \| `"contracts"` \| `"suggest_boundaries"` | `agent_config`: `options:{files?, root?}`, `output:{compact?}`; `citation`: `options:{file_path, span, sha?}`, **`[JSON]`**; `knowledge_gaps`: `options:{path_prefix?, min_coverage_pct?, hotspot_limit?, thin_community_size?, limit_per_category?}`, **`[JSON]`**; `surprising_connections`: `options:{path_prefix?, hub_threshold?, min_score?, rare_kind_pct?}`, `output:{format:"json", limit?:25}`; `recent_changes`: `options:{since?}` (无 `output`); `todos`: `options:{tag?, assignee?, has_assignee?, ticket?}`, **`[JSON+MB]`**; `contracts`: `options:{action?:"list\|check\|validate\|bridge", ref?, repo?}`, **`[JSON+MB]`**(`cursor?, fields?`); `suggest_boundaries`: **`[JSON+MB]`** |

### 2. 全量其余专项 `kind` 目录（共 96 个全收录，默认通用 `options: { repo?, project?, scope?, workspace? }` + `[JSON+MB]`）
* **图谱拓扑、函数流与诊断（17）**：`bottlenecks`、`def_use`（必填 `target:{symbol}`）、`cross_repo`（`options:{base_kind?}`）、`error_surface`、`field_writers`、`indirect_mutations`、`kcore`、`pagerank`、`scc`、`wcc`、`connectivity_health`、`edge_audit`、`ref_facts`、`resolution_outcomes`、`retrieval_log`、`speculative`、`synthesizers`。
* **并发、通道与不安全模式（4）**：`channel_ops`、`unclosed_channels`、`goroutine_spawns`、`unsafe_patterns`。
* **框架、路由、配置与事件（15）**：`routes`、`route_frameworks`、`models`、`dbt_models`、`orphan_tables`、`unreferenced_tables`、`sql_call_sites`、`config_readers`、`env_var_users`、`event_emitters`、`log_events`、`string_emitters`、`pubsub`、`temporal_orphans`、`domain`。
* **前端 / 移动端 / 语言生态专项（10）**：`components`、`swiftui_views`、`uikit_classes`、`drupal_hooks`、`cgo_users`、`wasm_users`、`annotation_users`、`constructors_missing_fields`、`external_calls`、`tests_as_edges`。
* **基础设施、发布与历史演进（9）**：`images`、`k8s_resources`、`kustomize`、`releases`、`fixes_history`、`doc_staleness`、`role`、`review`（底层规则包）、`help`（列出支持类型）。

```json
analyze({"kind":"cycles","options":{"repo":"repo-main"},"output":{"format":"json","max_bytes":8400}})
analyze({"kind":"architecture","options":{"resolution":"package"},"output":{"format":"json"}})
```

---

## 六、 关系网络中枢 (`relations` [11])
**核心门禁**：改任何公共函数签名或返回值前，**必须先调 `callers` 或 `usages`** 向上反查全部调用点；向下查依赖唯一合法名为 `dependencies`（禁写 `callees`），查引用用 `usages`/`references`（禁写 `instantiations`），查接口实现用 `implementations`。支持通用过滤 `min_tier`（`lsp_resolved` > `lsp_dispatch` > `ast_resolved` > `ast_inferred` > `text_matched`）与 `include_speculative`。

| 操作 | `target`（严格单字段） | 关键参数与精确 `output` 门禁 |
| :--- | :--- | :--- |
| `"callers"` / `"dependencies"` | `{ symbol: "id" }` | `options: { repo?, project?, scope?, workspace?, ref?, depth?: 2, limit?: 40~50, exclude_tests?, min_tier?, include_speculative? }`, **`[JSON+MB]`**（支持 `compact?`） |
| `"usages"` | `{ symbol: "id" }` | `options: { repo?, project?, scope?, workspace?, ref?, context?: "parameter_type\|return_type\|field\|value\|type\|attribute\|generic_arg\|call\|import", flavor?, return_usage?: "discarded\|assigned\|returned\|...", group_by?: "file", exclude_tests?, limit?: 50, max_tokens?, min_tier?, include_speculative? }`, **`[JSON+MB]`**（支持 `compact?`） |
| `"implementations"` / `"overrides"` / `"hierarchy"` | `{ symbol: "id" }` | `overrides`: `options:{direction?:"children\|parents"}`; `hierarchy`: `options:{direction?:"both\|up\|down", depth?:5, include_methods?}`; 均支持通用域过滤 + `min_tier` + `include_speculative`, **`[JSON+MB]`** |
| `"dependents"` / `"cluster"` | `{ symbol: "id" }`（`dependents` 不支持 `target.file`） | `dependents`: `options:{depth?:3, limit?:50, min_tier?, include_speculative?}`; `cluster`: `options:{radius?:2, limit?:50}`, 均 **`[JSON+MB]`**（支持 `compact?`） |
| `"references"` | `{ symbol: "id" }`（可省略改用 `options.name`） | `options: { name?, evidence_limit?: 50, exclude_tests?, min_tier? }`, **`[JSON]`（无 `max_bytes`）** |
| `"declaration"` | `{ query: "使用点片段" }` | `options?: { kind?, limit?: 20, path_prefix?, regex?: false }`, **`[JSON]`（支持 `format:"json\|toon"`）** |
| `"import_path"` | `{ symbol: "id" }` | 必填 `options: { path: "目标文件相对路径" }`（无 `output`） |

```json
relations({"operation":"callers","target":{"symbol":"repo-main/src/api/handler.rs::fn1"},"options":{"limit":40},"output":{"format":"json","max_bytes":8400}})
```

---

## 七、 链路追踪中枢 (`trace` [7])
**核心门禁**：双端点操作（`path`/`flow`/`taint`）必须同时传 `target` 与 `to`；单点操作（`cfg`/`call_chain`/`walk`）严禁传 `to`；`call_chain` **严禁传 `output: { compact: true }`** 以保留完整行号链路。

| 操作 | 端点容器与核心参数 | 精确 `output` |
| :--- | :--- | :--- |
| `"path"` \| `"flow"` | `target:{symbol:"A"}`, `to:{symbol:"B"}`, `path`: `options:{max_depth:8~24, k?:1, include_references?:true, max_frontier?, min_tier?}`; `flow`: `options:{max_depth?:8, max_paths?:10, min_tier?}` | **`[JSON+MB]`** |
| `"taint"` | `target:{query:"输入源"}`, `to:{query:"危险点"}`, `options:{max_depth?:8, limit?:20, min_tier?}` | **`[JSON+MB]`** |
| `"cfg"` \| `"call_chain"` | `target:{symbol:"id"}`, `cfg`: `options:{mermaid?:true}`; `call_chain`: `options:{depth?:4, limit?:50, max_tokens?, min_tier?, include_speculative?, repo?, project?, scope?, workspace?, ref?}` | **`[JSON+MB]`**（`call_chain` 禁 `compact:true`） |
| `"walk"` \| `"graph"` | `walk`: `target:{symbol}`, `options:{edge_kinds?:"calls", direction?:"out\|in\|both", max_depth?:8, token_budget?:6000, community?, repo?, project?, scope?, workspace?, ref?}`; `graph`: 必填 **`options:{query:"DSL", limit?:100}`** | **`[JSON]`（无 `max_bytes`）** |

```json
trace({"operation":"path","target":{"symbol":"repo-main/src/a.rs::fn1"},"to":{"symbol":"repo-main/src/b.rs::fn2"},"options":{"max_depth":16,"k":1},"output":{"format":"json","max_bytes":8400}})
trace({"operation":"graph","options":{"query":"nodes kind=interface | traverse implements in","limit":100},"output":{"format":"json"}})
```

---

# 【L3 级：协同推演、审查、PR 与空间/会话运维】

## 八、 虚拟推演与认知记忆 (`overlay` [10] / `recall` [8] / `remember` [8] — 均冷门面 `arguments`)
1. **内存推演 (`overlay` — 10 操作，均无 `format`)**：
   * `register`: `{workspace_id?}`；`fork`: 必填 `{name, from?, activate?}`；`switch`: 必填 `{name}`；
   * `push`: 必填 `{path, content?, base_sha?, deleted?}`；`delete`: 必填 `{path}`；
   * `merge`: 必填 `{from, into?:"main", force?:false}`（服务端固定 `to_disk:false`，合并落盘用 `edit.apply_overlay`）；
   * `simulate`: 必填 `{steps:"<WorkspaceEdit[] JSON>", diagnostics?:true, diagnostics_timeout_ms?, inherit_overlay?, stop_on_error?:true}`（固定 `keep:true`）；
   * `drop`: `{}`（销毁整个会话 overlay）；`drop_branch`: 必填 `{name}`；`keepalive`: `{}`（续期 TTL）。
2. **记忆召回与笔记 (`recall` — 8 操作，均 `[冷JSON]`)**：
   * `surface`: `{task?, symbol_ids?, file_paths?, scope?:"workspace\|global\|both", limit?:10, min_score?, excerpt_chars?, include_superseded?}`（服务端固定 `mark_accessed:false`）；
   * `distill`: `{max_symbols?:10, max_files?:10, max_tags?:10, max_recent?:8, excerpt_chars?, session_id?}`（长对话压缩恢复首动）；
   * `memories`: `{text?, kind?:"invariant\|constraint\|convention\|gotcha\|decision\|incident\|reference", symbol_id?, file_path?, tag?, source?, author?, scope?, min_importance?, pinned_only?, include_superseded?, since?, limit?:50}`；
   * `notes`: `{text?, symbol_id?, file_path?, tag?, session_id?, pinned_only?, since?, limit?:50}`；
   * `notebook_list`: `{limit?:50}`；`notebook_find`: `{query?, limit?:50}`；`notebook_show`: `{id}`；
   * `onboarding`: `{essential_kinds?, min_per_kind?, min_total?}`。
3. **记忆写入与风险确认 (`remember` — 8 操作)**：
   * `memory`: `{title?, body?, kind?, symbol_ids?, file_paths?, tags?, importance?:1..5, confidence?, scope?, source?, pinned?, id?, supersedes?, superseded_by?, no_autolink?}`, `[冷JSON]`；
   * `edit_memory`: `{id, pattern, replacement, mode?:"regex\|literal", scope?}`, `[冷JSON]`；`rename_memory`: `{id, from_scope?, to_scope}`, `[冷JSON]`；
   * `note`: `{body, file_path?, symbol_id?, links?, tags?, pinned?, id?, no_autolink?}`, `[冷JSON]`；
   * `notebook`: `{title?, body, tags?, id?}`, `[冷JSON]`；`notebook_used`: `{id}`, `[冷JSON]`；
   * `risk_ack`: `{source?:"auto\|edit\|diff\|symbols\|ranges", symbols?, scope?, base?, path?, start_line?, end_line?, ranges?, workspace_edit?, lens?, risk_gate?, repo?}`, `[冷JSON]`（服务端固定 `ack:true`）；
   * `suppress_finding`: `{action?:"add\|list\|remove", identity_key?, rule?, category?, symbol_id?, file?, source_line?, line?, reason?, author?}`, **`[冷JSON+MB]`**。

---

## 九、 协同审查与 PR 雷达 (`review` [7] / `pr` [6] / `publish_review` [1])
1. **本地只读审查 (`review` — 普通门面，7 操作)**：
   * `run`: `source:{scope?:"unstaged\|staged\|all\|compare", base?, diff?}`, `options?:{base_ref?, repo?, use_llm?, max_tokens?}`, **`[JSON+MB]`**；
   * `pack`: 同 `run` 参数 + `options:{include_pack?, scrub?}`, **`[JSON+MB]`**；
   * `diff_context`: `source:{scope?}`, `options?:{base_ref?, repo?}`, `output?:{compact?}`（无 `format`/`max_bytes`）；
   * `pr_context`: `source:{scope?, base?}`, `options?:{base_ref?, ids?, repo?, sections?, verify?, audit_config?, edits?, session_id?, max_tokens?}`, **`[JSON+MB]`**；
   * `questions`: `source?:{base?}`, `options?:{ids?, path_prefix?, repo?, categories?, hub_threshold?, min_betweenness?, min_community_size?, limit?, max_tokens?}`, **`[JSON+MB]`**；
   * `sibling_context`: `source:{scope?, base?}`, `options?:{base_ref?, focus_file?, focus_files?, focus_symbol_id?, repo?, max_tokens?}`, **`[JSON+MB]`**；
   * `critique`: `source:{scope?, base?, diff?}`, `options?:{base_ref?, findings?, prior_review?, repo?, max_tokens?}`, **`[JSON+MB]`**。
2. **PR 雷达分析 (`pr` — 冷门面 `arguments`，6 操作，均 `[冷JSON+MB]`)**：
   * `list`: `{state?:"open\|closed\|all", limit?:30, prs?, repo?}`；
   * `impact`: 必填 `{number, files?, receipt?, scrub?, repo?}`；
   * `conflicts`: `{limit?:20, prs?, files?, repo?}`（无 `pr_numbers` 字段）；
   * `reviewers`: `{ids? | base? | number?, limit?:10, repo?}`；
   * `risk`: `{ids? | base?, repo?}`；
   * `triage`: `{limit?:20, prs?, files?, repo?, use_llm?, max_tokens?}`。
3. **对外发布评审 (`publish_review` — 冷门面 `arguments`，1 操作，`[冷JSON+MB]`)**：
   * `post`: 必填 `number`，可选 `{findings?, summary?, scope?, base?, base_ref?, diff?, dry_run?:true, confirm_public?:false, public?, refuse_on_secret?:true, owner?, repo_name?, provider?:"github", repo?}`。

---

## 十、 工作区底层索引与富化 (`workspace_admin` 运维项)
* **索引恢复红线**：仅在索引损坏、断电或大量新增未探测时使用 `workspace_admin(operation="index", arguments:{path:"仓库绝对路径"})` 或 `workspace_admin(operation="reindex", arguments:{path?, paths?:[...], format:"json"})`，严禁常规开发滥用全量索引。
* **富化写盘**：
  * `workspace_admin(operation="blame"|"coverage"|"sql_rebuild"|"temporal_verify", arguments:{repo?, project?, scope?, workspace?, profile?}, output:{format:"json", max_bytes:8400})`（其中 `coverage` 必填 `arguments.profile`；此 4 项在顶层传 `output`）；
  * `workspace_admin(operation="enrich_churn"|"enrich_releases", arguments:{branch?, path?, format:"json"})`（`[冷JSON]`）。

---

## 十一、 缓冲提炼 (`response` [5])、会话治理 (`session` [8]) 与 MCP 原生 Resource / Prompt
1. **大输出缓冲提炼 (`response` — 冷门面 `arguments`，5 操作)**：
   * `grep`: 必填 `arguments:{pattern, response_id?, context?, before?, after?, ignore_case?, literal?, max_matches?}`；
   * `slice`: `arguments:{from?:1, to?, response_id?}`；
   * `peek`: `arguments:{lines?:20, response_id?}`；
   * `stats`: `arguments:{}`（列出最近缓存的 `response_id` 列表）；
   * `export_context`: 必填 `arguments:{task, entry_point?, max_symbols?:5, token_budget?:2000, format?:"markdown\|json"}`。
2. **会话状态控制 (`session` — 8 操作，按容器隔离区分)**：
   * **顶层 `channel` 型**：`subscribe` / `unsubscribe` → 必填顶层 `channel`（`"daemon_health" | "diagnostics" | "graph_invalidated" | "stale_refs" | "workspace_readiness"`），可选 `arguments:{}`；
   * **`arguments` 封装型**：
     * `planning_mode`: `arguments:{mode:"planning"|"editing"}`（切换只读规划/可编辑状态）；
     * `cursor`: `arguments:{action:"goto\|into\|up\|sibling\|back\|where\|read", id?, select?, format?:"json\|toon"}`；
     * `agents`: `arguments:{action?:"list\|register\|heartbeat\|lock\|unlock\|unregister", id?, name?, paths?, cursor?, status?, format?:"json"}`；
     * `workflow`: `arguments:{action:"start\|advance\|status\|stop", mode?:"block\|warn"}`；
     * `proxy_enable` / `proxy_disable`: 必填 `arguments:{slug}`。
3. **MCP 原生 Resource（13 个）与 Prompt（3 个）**：
   * **Resource**：`gortex://workspace`、`gortex://repos`、`gortex://active-project`、`gortex://index-health`、`gortex://stats`、`gortex://schema`、`gortex://report`、`gortex://session`、`gortex://guide`（支持分段 `gortex://guide/{topic}`）、`gortex://audit`、`gortex://surprises`、`gortex://god-nodes`、`gortex://questions`。
   * **Prompt**：`pre_commit`（`arguments:{"scope":"unstaged","repo?":"..."}`）、`safe_to_change`（必填 `arguments:{"ids":"..."}`）、`orientation`。

---

# 【执行遵从性与防踏空检查自查清单】 (Compliance Checklist)
1. [ ] **【开局探测】**：是否首动完成了 `workspace.repos`/`info` + `active_project` + `index` 探测并锁定 `repo`？
2. [ ] **【路径零拼接】**：`options.path`/`options.path_prefix` 中是否彻底去除 `repo` 前缀？`target.*`/`changes[].path` 是否按需携带了准确的 `repo` 前缀？
3. [ ] **【直调免 `capabilities` 空转】**：文档已收录的操作是否直接发请求，未在调用前常规跑 `capabilities` 预查 Schema？（仅限报错自愈或未收录盲区才兜底调用一次且同会话不重复）。
4. [ ] **【JSON 与 `max_bytes` 精确匹配】**：`[JSON+MB]` 是否设 `max_bytes:8400`（`winnow` 8250），`[JSON]` 接口（如 `search.text`/`files`、`trace.walk`/`graph`、`change.contract`）是否绝不硬塞 `max_bytes`，写/状态操作是否绝不硬塞 `format`？
5. [ ] **【精读切片】**：拿到符号 ID 是否优先 `read.symbols`/`source`？使用 `read.file` 是否强锁 `options: { offset, limit <= 150 }` + `output: { max_bytes: 8400 }`，严禁无范围裸读整文件？
6. [ ] **【原子交付与闭环】**：多处改动是否合并为单轮 `edit.batch`？源码改后是否依次完成 `change.detect` → `change.diagnostics` → `change.tests` 并辅以实际测试？
7. [ ] **【防溢出隔离】**：若发生 `output.txt` 截断落盘，是否严禁原生直接读取，并优先用 `response.grep`/`slice`/`peek` 提炼？
