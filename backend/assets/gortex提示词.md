# Gortex MCP 全生命周期执行规范 (v200 · 对齐 Gortex v0.64.11 / `facade-v1` 1.0.1)

> **核心定位**：本规范定义智能体驱动 Gortex 21 个公开门面（Facade）的精确契约。文档只保留高价值路由规则与强制红线；**任何不确定的操作字段，严禁猜测，一律调用 `capabilities(domain, operation, detail="schema")` 或读取 `gortex://guide` 获取实时权威定义。**

---

## 一、 总则、接入边界与权威裁决

1. **代码操作 100% Gortex 闭环**：对已索引仓库的检索、精读、修改、重构与影响评估，一律使用 Gortex MCP 21 门面，禁止退化为宿主原生 `Read`/`Grep`/`Glob` 或 `cat`/`rg`/`find`。
2. **原生工具豁免边界**：仅允许在以下情形使用宿主原生工具——① 读取索引仓库外的宿主文件或外部系统配置；② 用户明确指令执行原生 Git 提交、外部分支检出、构建或运行测试命令（`cargo test`/`go test`/`npm build` 等）。
3. **集成失败止损红线**：若环境已配置 Gortex MCP 但模型可用工具中缺失这 21 个工具，**立即报告 Gortex MCP 集成故障并停止**；严禁自行启动 daemon、修改配置或回退到 `gortex call`/shell 脚本绕过。
4. **运行时权威裁决位阶**：`工具真实返回（error / completion 状态 / guard 拦截）` > `capabilities(detail="schema")` > `物理文件与 Git 状态` > `本文档`。

---

## 二、 任务首动分流与 Localization 终结状态机

禁止每次任务机械堆叠“多步 `workspace` 探测 + 双通道 `search`”。按真实意图直接分流：

1. **已明确文件路径的首次阅读 / 审阅 / 总结**：
   直接调用 `read(operation="file", target:{file:"<path>"}, options:{new_user_task:true})`（切勿启动 `explore.localize`）。`new_user_task:true` **仅限**当前新用户请求的第一通显式文件读，绝不用于续读或重试。
2. **未知位置、寻找文件 / 符号 / 证据（“X 在哪 / 谁负责 Y”）**：
   调用 `explore(operation="localize", task:"<问题>", options:{new_user_task:true})`，并**绝对服从返回的 `completion.state` 与 `completion.required_action`**：
   * `answer_ready`：立即依据 `completion.final_response` 作答，**禁止再调用任何检索或读文件工具**（服务端会在同会话内硬性拦截多余导航调用）；
   * `needs_exact_read` / `needs_refinement` / `needs_recovery`：仅执行 `completion.instruction` / `ExactSymbol` / `AllowedSymbols` 授权的那 1 次调用后再作答。
3. **代码诊断、缺陷修复或功能开发**：
   调用 `explore(operation="task", task:"<任务>")` 一步获取候选符号、关键源码与调用邻域；如需补全，最多再做 1 次聚焦 `search`/`read`/`relations`/`trace`，立即转入改前评估与写入。
4. **多仓库或陌生环境边界不明时**：
   按需调用 `workspace(operation="repos")` / `workspace(operation="active_project")` / `workspace(operation="index")`（或直接读取 MCP 资源 `gortex://repos`、`gortex://index-health`）。避免滥用重型的 `workspace(operation="graph")`。

---

## 三、 参数容器、路径双通道与响应预算契约

### 1. 容器与判别器规则
* **判别器字段**：除 **`analyze` 必须传 `kind`**（严禁传 `operation`）、**`ask` 直接传 `question`**、**`capabilities` 传 `domain`/`operation`/`detail`** 外，其余 18 个业务门面均在顶层传 **`operation`**。
* **12 个热门面（顶层平铺，严禁包裹 `arguments:{}`）**：
  `explore`, `search`, `read`, `relations`, `trace`, `analyze`, `ask`, `change`, `edit`, `refactor`, `review`, `capabilities` —— 业务参数分别置于顶层、`target`、`to`、`source`、`context`、`guard`、`options`、`output`、`view`。
* **9 个冷门面（业务参数必须置于 `arguments:{}` 内）**：
  `workspace`, `workspace_admin`, `overlay`, `recall`, `remember`, `response`, `pr`, `publish_review`, `session`（`session` 的订阅还需顶层传 `channel`；所有门面顶层均支持 `output` 与 `view`）。
* **`target` / `to` 唯一选择器契约**：`target`（以及仅限 `trace.path`/`flow`/`taint` 使用的 `to`）只能含 **1 个非空键**，合法键仅限：`file` | `symbol` | `symbols` | `query` | `artifact` | `repo`。

### 2. 路径双通道规则
* **全局定位器（`target.file`, `target.symbol`, `source.file`, `edit.batch` 的 `path`/`source`/`destination`/`id`）**：
  在多仓库中**必须/优先携带仓库名前缀**（如 `repo-main/src/api.rs`、`repo-main/src/api.rs::handle`）。
* **仓内范围过滤器（`search.symbols` / `search.files` 的 `options.path`；`search.winnow` / `search.ast` 的 `options.path_prefix`）**：
  **严禁拼接仓库名前缀**；仓库限定统一使用 `options.repo`（`search.text` 虽已支持前缀自动拆分，仍建议保持 `repo` 与仓内相对 `path` 分离）。

### 3. 预算、Wire 格式与内存缓冲隔离 (`output` & `response`)
* **预算与格式**：格式与字节上限属于 **`output` 容器**（如 `output:{format:"json"}` 或 `output:{format:"gcx", max_bytes:16000}`）。服务端默认 `DefaultMaxBytes = 40000` 已可防宿主溢出，**勿再机械强锁 `max_bytes:8400`**；`read.file` 正文体积请用 `options:{offset, limit}`（建议 `limit<=150`）、`context:{compress_bodies:true, max_lines}` 或 `options:{max_chars}` 控制。
* **大输出隔离**：服务端自动将 1 KB–8 MB 的近期大响应存入会话内存环（最多 8 条）。若宿主提示输出被转存到 `output.txt`，**严禁调用宿主 `read_file`/`cat` 读取 `output.txt`**，必须调用 `response(operation="stats"|"peek"|"grep"|"slice")` 从服务端内存环精准提炼。

---

## 四、 21 门面完整能力速查矩阵（v0.64.11）

| 门面 | 容器 | 核心 `operation`（`analyze` 用 `kind`） | 关键参数、服务端约束与避坑要点 |
| :--- | :--- | :--- | :--- |
| **`explore`** | 热 | `localize`, `task`, `context`, `closure`, `outline`, `plan`, `prefetch`, `suggest`, `wakeup` | 首选任务入口；`localize`/`task` 必填顶层 `task`；遵守 `completion` 状态机 |
| **`search`** | 热 | `symbols`, `text`, `files`, `ast`, `winnow`, `artifacts`, `completion` | `symbols` 固定 `assist:"off"`，类/结构体用 `kind:"type", flavor:"struct"/"class"`，组件用 `flavor:"component"`；模板指令/字面量用 `text`；`ast` 的 `query` 传 S-expression 时须配 `options.language`（单语言）或改用 `options.detector` |
| **`read`** | 热 | `file`, `source`, `symbols`, `summary`, `editing_context`, `artifact`, `history` | `symbols` 须传 **`target:{symbols:[...]}`**；支持 `context:{compress_bodies:true}` + `options:{keep}` 读骨架（**修改并发/重试/迁移/关键分支时禁开压缩**）；支持 `if_none_match` 复用 `etag`；`history` 仅查**当前会话修改次数**，非 Git 历史 |
| **`relations`** | 热 | `callers`, `dependencies`, `dependents`, `usages`, `references`, `implementations`, `overrides`, `hierarchy`, `cluster`, `declaration`, `import_path` | 向下依赖法定名为 **`dependencies`**（非 `callees`）；`declaration` 用 `target:{query}`，其余用 `target:{symbol}` |
| **`trace`** | 热 | 双端: `path`, `flow`, `taint`<br>单端: `cfg`, `call_chain`, `walk`<br>DSL: `graph` | 双端须同时传 `target` 与 `to`（`taint` 用 `query`，`path`/`flow` 用 `symbol`）；单端严禁传 `to`；`graph` 传 `options:{query}` |
| **`analyze`** | 热 | 传 **`kind`**：`cycles`, `hotspots`, `dead_code`, `sast`, `impact`, `bottlenecks`, `health_score`, `race_writes`, `ownership`, `churn`, `coupling`, `suggest_boundaries`, `def_use`, `why`, `help` 等（74 个原生只读 kind + 21 个别名操作 + `help`） | **唯一不用 `operation` 的门面**；无 `layers` kind；仅 `def_use`/`co_change`/`impact`/`why` 接受 `target`（`coupling`/`race_writes`/`churn` 须用 `options.path_prefix` 等，传 `target` 会被拒）；`cycles` 的 `scope` 为路径前缀且无 `max_depth`/`limit`；`blame`/`coverage`/`sql_rebuild`/`temporal_verify` 须走 `workspace_admin` |
| **`ask`** | 热 | `research`（或省略） | 顶层传 `question`，需配置 LLM provider |
| **`change`** | 热 | `impact`, `verify`, `preview`, `simulate`, `edit_plan`, `api_impact`, `detect`, `diagnostics`, `tests`, `guards`, `contract`, `receipt`, `compare_overlay`, `compare_branches`, `overlay_state`, `overlay_branches`, `code_actions`, `pattern`, `ranges` | `preview` 必填 `source:{workspace_edit}`（落盘前推演，**非看 Git diff**）；看工作区改动用 `detect` 或 `review.diff_context`；`tests`/`guards`/`edit_plan` 必填 `target:{symbol\|symbols}`；`diagnostics` 刚改完需实时结果时传 `options:{wait:true}`；写入超时后用 `receipt` 查状态 |
| **`edit`** | 热 | `batch`, `symbol`, `file`, `write`, `apply_overlay`, `docs`, `skill`, `wiki`, `scaffold`, `export_graph` | `batch` 的 `changes` 为**扁平对象数组**（`op`: `edit_file`/`edit_symbol`/`move_file`/`delete_file`）；`symbol` 与 `file` 均须传 **`match` 与 `replacement`**；`apply_overlay` 传 `options:{from:"<branch>"}`；默认开启 Pre-write Parse Gate；支持 `guard:{base_sha}` 与 `dry_run:true` |
| **`refactor`** | 热 | `rename`, `fix_all`, `apply_code_action`, `move`, `inline`, `delete` | `move` 目的地字段叫 **`destination`**（非 `destination_file`）；`move` 与 `inline` **仅支持 Go**；`delete` 默认 `dry_run:true`，提交须传 `dry_run:false` |
| **`review`** | 热 | `run`, `pack`, `critique`, `diff_context`, `sibling_context`, `pr_context`, `questions` | 参数入 `source` / `options`；**无 `draft` 操作** |
| **`publish_review`** | 冷 | `post` | 唯一外写 PR 评论门面，**无 `publish` 操作**；`arguments:{number, ...}` |
| **`pr`** | 冷 | `list`, `impact`, `triage`, `conflicts`, `risk`, `reviewers` | 参数全部入 **`arguments`**；`impact` 传 `arguments:{number}`（非 `pr_number`）；`conflicts` 传 `repo`/`limit`/`prs`/`files`（无 `pr_numbers`） |
| **`recall` / `remember`** | 冷 | `recall`: `surface`, `distill`, `notes`, `memories`, `notebook_find/list/show`, `onboarding`<br>`remember`: `note`, `memory`, `edit_memory`, `rename_memory`, `notebook`, `notebook_used`, `risk_ack`, `suppress_finding` | 压缩恢复首调 `recall.distill`；任务中按需 `recall.surface`（服务端固定 `mark_accessed:false`）；跨会话不变量用 `remember.memory`（`kind: invariant\|gotcha\|convention\|decision\|constraint\|incident`）；`risk_ack` 服务端固定 `ack:true`，传入 `arguments:{source:"symbols", symbols:"..."}` |
| **`workspace` / `workspace_admin`** | 冷 | `workspace`: `repos`, `active_project`, `index`, `info`, `graph`, `project`, `scopes`, `checkouts`, `proxy`<br>`workspace_admin`: `track`, `untrack`, `index`, `reindex`, `set_active_project`, `save_scope`, `delete_scope`, `enrich_churn`, `enrich_releases`, `blame`, `coverage`, `sql_rebuild`, `temporal_verify`, `feedback` | `workspace.project` 免切域跨库查符号；`set_active_project` 只认配置里的项目名；多仓改代码只要路径带仓前缀即可直接 `edit.batch`；`reindex` 传仓名或绝对路径（可选 `paths` 数组） |
| **`overlay`** | 冷 | `register`, `push`, `delete`, `drop`, `keepalive`, `fork`, `switch`, `drop_branch`, `simulate`, `merge` | 内存沙箱推演（`simulate` 固定 `keep:true`，`merge` 固定 `to_disk:false`）；真落盘调 `edit.apply_overlay` |
| **`session`** | 冷 | `planning_mode`, `workflow`, `cursor`, `proxy_enable`, `proxy_disable`, `agents`, `subscribe`, `unsubscribe` | `subscribe`/`unsubscribe` 需顶层传 `channel`: `daemon_health\|diagnostics\|graph_invalidated\|stale_refs\|workspace_readiness` |
| **`response`** | 冷 | `stats`, `peek`, `slice`, `grep`, `export_context` | 提取服务端内存缓冲大输出；`response_id` 省略即最近一次 |
| **`capabilities`** | 热 | `domain`, `operation`, `detail` (`"summary"` \| `"schema"`) | 运行时权威 Schema 与 `request_shape` 查询 |

---

## 五、 Worktree 与分支视图路由红线（`view` / `freshness`）

1. **禁止手动 `track` 自动工作树**：当前会话 CWD 自动绑定其 checkout，linked worktree 由 Gortex 自动以 overlay 形式发现；**严禁对普通 worktree 调用 `workspace_admin.track`** 或重启 daemon。
2. **显式视图切换**：所有门面均可在顶层传入 `view`：
   * 指定 worktree：`view: { "kind": "worktree", "checkout_id": "..." }`（ID 来自 `workspace.checkouts`，也可传主机绝对 `path`）；
   * 指定只读提交快照：`view: { "kind": "git_ref", "value": "refs/heads/main", "graph_id": "..." }`（**不可写、无 LSP、无 `search.text`**）。
3. **一致性控制**：响应中 `exact:false` 表示发生只读降级；严格模式可在顶层或 `options` 中附加 `require_exact:true`、`require_fresh:true`、`wait_deadline`（RFC3339，`require_fresh` 仅对自动路由 checkout 生效）。主工作树销毁/遗忘/`set-primary` 须先预览并获用户明确确认。

---

## 六、 改写验证闭环、经核验模板与原生资源

### 1. 改前→改中→改后标准闭环
* **改前**：影响公共符号先调 `change(operation="impact", target:{symbol})`（兼顾上下游调用可用 `relations.callers`/`implementations`）；改函数签名先调 `change(operation="verify", source:{changes:[{symbol_id, new_signature}]})`。
* **改中**：优先将多处修改聚合成单轮 `edit(operation="batch")` 原子提交；若遇行号漂移/冲突报错，立即用 `change(operation="detect")` + `read(operation="file")` 对齐基线后再提交；超时未获回执先查 `change(operation="receipt")`。
* **改后**：`change(operation="detect")` 取变动符号 ID → `change(operation="tests", target:{symbols})` 与 `change(operation="guards", target:{symbols})` → `change(operation="diagnostics", source:{file}, options:{wait:true})` → 运行工程真实构建与测试。

### 2. 标准调用模板（100% 对齐源码校验）
```json
// 1) 未知定位（遵循返回的 completion 状态机，answer_ready 即停）
explore({ "operation": "localize", "task": "连接池超时重试逻辑在哪里处理", "options": { "new_user_task": true } })

// 2) 已知文件的首次阅读 / 压缩骨架阅读
read({ "operation": "file", "target": { "file": "repo-main/src/pool.rs" }, "options": { "offset": 1, "limit": 120, "new_user_task": true } })
read({ "operation": "file", "target": { "file": "repo-main/src/pool.rs" }, "context": { "compress_bodies": true }, "options": { "keep": "acquire_conn" } })

// 3) 精确符号检索（仓库名放 options.repo，path 只写仓内相对路径）
search({ "operation": "symbols", "query": "ConnectionPool", "options": { "repo": "repo-main", "path": "src/net", "kind": "type", "flavor": "struct", "limit": 30 }, "output": { "format": "json" } })

// 4) 多文件/多类型原子批量修改（扁平 changes + op）
edit({
  "operation": "batch",
  "changes": [
    { "op": "edit_file", "path": "repo-main/frontend/User.vue", "old_string": "离线", "new_string": "在线" },
    { "op": "edit_symbol", "id": "repo-main/src/api.rs::handle", "old_source": "let x = 1;", "new_source": "let x = 2;" }
  ],
  "output": { "format": "json" }
})

// 5) 改后核验与图谱分析（注意 analyze 传 kind，change.tests 必须传 target）
change({ "operation": "detect", "source": { "scope": "unstaged" } })
change({ "operation": "tests", "target": { "symbols": ["repo-main/src/api.rs::handle"] } })
analyze({ "kind": "cycles", "options": { "repo": "repo-main", "scope": "src/net" }, "output": { "format": "json" } })
```

### 3. 原生 MCP Resources 与 Prompts
* **按需手册与 Schema**：`gortex://guide`（或 `gortex://guide/{topic}`）、`gortex://schema`。
* **会话与启动状态**：`gortex://session`、`gortex://index-health`、`gortex://workspace`、`gortex://repos`、`gortex://active-project`、`gortex://stats`、`gortex://report`、`gortex://god-nodes`、`gortex://surprises`。
* **内置 Prompts**：`orientation`、`pre_commit(scope, repo)`、`safe_to_change(ids)`。
