# Gortex MCP 统一架构规范契约 (v707 · 精简核心版)
## 第一部分：容器结构、选择器与全局输出约定
### 1. 工具调用与四类门面容器
直接调用对应 Gortex MCP 门面工具（入参即外层 JSON 对象，勿套伪函数壳）。
普通平铺（9 个：`explore`、`search`、`read`、`edit`、`refactor`、`change`、`relations`、`trace`、`review`）：顶层传 `operation` 并平铺语义载荷（`query`/`task`/`target`/`to`/`source`/`context`/`guard`/`match`/`replacement`/`content`/`changes`/`new_name`/`destination`/`dry_run`/`options`/`output`），严禁外包 `"arguments"` 或 `"params"`。
分析平铺（1 个：`analyze`）：顶层鉴别键固定为 `kind`（严禁写 `operation`），仅 `co_change`/`why`/`def_use` 必填 `target`、`impact` 可选 `target`（其余 92 个禁传 `target`），严禁外包 `"arguments"`。
冷封装（6 个：`workspace`、`workspace_admin`、`overlay`、`remember`、`recall`、`response`）：顶层传 `operation`，业务参数（及除下述特例外的 `format`/`max_bytes`）封装进 `"arguments": {...}`（无参传 `{}`）。4 个混合特例（`workspace_admin` 的 `blame`/`coverage`/`sql_rebuild`/`temporal_verify`）：过滤参数放内层 `arguments`（`coverage` 必填 `profile`），`output` 放外层顶层。
会话与自省（3 个：`session`、`capabilities`、`ask`）：
  `session` 全 8 项严禁传顶层 `output`；`subscribe`/`unsubscribe` 顶层 `{operation, channel, arguments:{}}`；其余 6 项顶层只许 `{operation, arguments, view?}`（禁 `require_exact`/`require_fresh`/`wait_deadline`），`cursor`/`agents` 的 `"format":"json"` 放内层 `arguments`。
  `capabilities`：`{domain?, operation?, detail:"summary"|"schema"}`；`ask`：`{question, options?, output?}`（未配研究提供方时 `capabilities(domain="ask")` 返回 `operations: []`，勿盲调）。
### 2. `target`/`to` 单属性闭集与 `view` 视角
`target` / `to`（严禁 `{}`、`null`、`{"path":...}` 或多键共存）：每对象仅限 1 键，合法键为 `file`、`symbol`、`symbols`（非空数组）、`query`（仅 `relations.declaration`/`trace.taint`）、`artifact`（仅 `read.artifact`）、`repo`（日常限仓勿擅用 `target.repo`）；不收 `target` 的操作严禁传入。
顶层 `view`（仅需切换工作树/分支/提交视角时补充，按需省略）：`{"kind":"auto"}` | `{"kind":"base","graph_id":"..."}` | `{"kind":"worktree","checkout_id":"..." | "path":"<绝对路径>"}`（二选一互斥） | `{"kind":"git_ref","value":"refs/heads/..."}`（须完整 ref，禁简写 `"main"`） | `{"kind":"commit","value":"<40位小写SHA>"}`（禁短 SHA）。
### 3. 全局 JSON、无折叠与预算硬锁
显式动词头：外层必传 `operation`（`analyze` 必传 `kind`）。
强制 `"json"` 与禁传例外：支持 `format` 的查询/分析统一传 `"format": "json"`（禁 `"toon"`/`"gcx"`/`"markdown"`；`edit.docs`/`wiki`/`skill`/`export_graph` 及 `explore.wakeup` 按需选格式）；`read.file` 及未声明 `format` 的写操作严禁传 `format`。
绝对无折叠：`read.file`/`read.source`/`read.editing_context` 必须显式传 `"compress_bodies": false`；全库禁用 `compact: true`、`summary_only: true` 及改码前 `fidelity_globs`（如 `:omit`）/`max_lines` 截略；`read.symbols` 保持 `include_source: true`。
分片与预算：`read.file` 严禁盲读整文件，必传 `options: {"offset": 1基行号, "limit": 50~80}`（硬上限 `<=150`，高密度中文/长行收紧至 `35~60`）+ `output: {"max_bytes": 8400}`；其余查询支持 `max_bytes` 时默认 `8400`（`search.winnow` 为 `8250`）；`explore` 控 `options.token_budget: 1600~4000`。
---
## 第二部分：【L0 全局执行铁律】
### 铁律 1：外部只读豁免、Gortex 闭环与原生跳出边界
1. 代码仓库外部系统目录的规范文档仅限宿主原生只读工具分片读取，严禁调 Gortex 读外部路径。
2. 纳管工程内所有源码、测试代码（含 `#[cfg(test)]` 与独立测试文件）、配置与维护脚本的探测、阅读、新建、修改与审查 100% 走 Gortex 闭环。
3. 仅两类情况允许宿主原生工具：① 用户明确要求“使用原生工具”（单次跳出后即返闭环）；② Git/GitHub 运维、编译/类型检查/构建、只读格式检查（如 `cargo fmt --check`/`prettier --check`）与实际运行/调试测试进程（严禁借此检索、阅读或修改工程内受维护文件）。
### 铁律 2：双通道路径与 17 伪仓选键拦截
通道 A（全局定位寻址器：`target`/`source.file`/`changes`/`explore.closure` 及记忆路径/符号 ID）：原样复制自 `workspace`/`search`/`explore`，多仓及 `unbound_global` 下必须保留前导 `<repo>/` 前缀。
通道 B（仓内子路径前缀：`search` 的 `path`/`path_prefix`、`analyze` 的 `path_prefix`/`cycles.scope`、`explore.context` 的 `task` 内联 `path:src/foo`）：严禁拼接 `<repo>/` 前缀；限仓另传允许的 `options.repo`。
17 个伪仓选键拦截：`repo`、`repo_path`、`repository`、`repository_path`、`repo_root`、`repository_root`、`repopath`、`repo-path`、`repo_dir`、`root`、`cwd`、`dir`、`worktree`、`work_tree`、`base_repo`、`workspace`、`project`（及 `scope`）只准传给第三部分明确允许的操作；未声明消费的接口传入任意一个即报 `invalid_argument` 闪断。
### 铁律 3：开局探测、跨仓与准入止损
未知环境先一跳 `workspace.repos`（同会话工作区未变直接复用；仅遇 `unbound_global`、归属需对齐或 `state != "ready"` 时按需补调 `active_project`/`index`）。若目标属于被索引排除的构建产物/依赖缓存（`target/`、`dist/`、`.cargo/`、`node_modules/`），直接说明未纳管，严禁死循环重试。
跨仓只读免切项目（用 `workspace.project` 或带 `<repo>/` 前缀的 `read`）；对外部仓执行依赖主工程上下文的重构、测试图分析或批量改动前，先 `workspace_admin.set_active_project`（单仓并发查询可按允许传 `options.repo`）。
### 铁律 4：稳态编辑路由、`old_`/`new_` 隔离与快速熔断
未读原文禁盲改 · 已读未变禁重读：修改前上下文必须有未折叠逐字符原文（`.env`/`.yaml`/`.toml`/`.properties` 须带 `allow_secrets: true`）；仅未直读、曾读脱敏符或失配漂移时才补读。
路由优先级与键名隔离：
  1. 单点 / 跨 2~6 个不同文件独立修改：首选 `edit.file` / `edit.symbol`（`match` + `replacement`）（独享 `checkParseGate` 与 `guard`；跨文件可并发，同文件严禁并发，且禁写 `old_string`/`old_source`）。
  2. 原子事务 / 同文件 `>=2` 处叠加 / 含 `move_file`/`delete_file`：用 `edit.batch`（`old_string`/`new_string` 或 `old_source`/`new_source`），控量 `2~6` 项、硬上限 `8` 项且 `<4 KB`、严禁并发批改、禁 `changes:[null]` 与 `"output":null`（省略 `output` 或 `{"compact":false}`）、禁写 `match`/`replacement`/`target`；内部无 `checkParseGate`，源码批改后必补 `change.diagnostics`（无 LSP 时原生构建兜底）。
  3. 专用通道：新建/整文件覆写用 `edit.write`；纯跨引用重命名用 `refactor.rename`。
扩窗与两级熔断：`match` 取 `3~8` 行唯一原文，报 `ambiguous_match` 立即扩窗至 `8~12` 行（含函数头/唯一邻接行）。同一子任务内所有 `edit.batch` 失败均计入连续失败次数（仅 `tool_blocked_by_mode` 不计，成功清零）：第 1 次失败禁原样重放（参数错查 `capabilities`，未匹配先重读对齐）；第 2 次连续失败批规模 `>=4` 拆为 `2~3` 项（若原本已 `2~3` 项或连续 `changes:[null]`/`invalid_argument` 则直切 `match`）；第 3 次连续失败永久终止本子任务 `edit.batch`，全改 `edit.file`/`symbol`（`match`）收口。
模式锁与超时退避：遇 `tool_blocked_by_mode` 且允许改码时先调 `session.planning_mode(mode:"editing")` 解锁；流式超时不确定落盘时，调 `edit.batch(options:{transaction_id, status_only:true})`（省略 `changes`）或 `change.receipt` + `change.detect` 核实后，仅对未落盘片段重读并修正或降级。
### 铁律 5：防溢出隔离与 `capabilities` 首错停猜
触发 `output.txt` 截断或缓冲句柄时，严禁宿主原生工具读取 `output.txt`，改用收紧 `max_bytes`/`limit` 分片重取或 `response` 门面（`grep`/`slice`/`peek`/`stats`）提炼。
严禁原生读取本地 `mcp/gortex/*.json`；已知接口直接调用不空转 `capabilities`；一旦返回任何参数校验错误，立即停猜并调用一次 `capabilities(domain, operation, detail:"schema")` 对照纠偏（同 `domain+operation` 本会话禁重复再查）。
---
## 第三部分：19 个本地开发门面命令与逐位置参数拆解（L1 ~ L3 全收录）
---
## 零、 工作区与多项目协同中枢 (`workspace` [9] / `workspace_admin` [14])
### 1. `workspace` — 工作区只读与导航（冷门面，共 9 个操作）
完整基准调用模板：
  查仓库清单：`workspace`（`Arguments={"operation":"repos","arguments":{"format":"json","max_bytes":8400}}`）
  跨项目查符号：`workspace`（`Arguments={"operation":"project","arguments":{"project":"my-app","query":"MyService","limit":20}}`）
  查工作树检查点：`workspace`（`Arguments={"operation":"checkouts","arguments":{"family":"...","format":"json","max_bytes":8400}}`）
`operation` 位置可选参数（全部 9 个）：
  `"repos"`：列出当前工作区所有已纳管项目/仓库、绑定模式（如 `unbound_global`）、本地路径映射 `members[].path` 与就绪状态 `members[].state`（开局全局探测首选）。
  `"info"`：返回工作区完整身份信息（绑定模式、根目录、`.gortex.yaml` 标记内容、自动发现成员及未知标记键）。
  `"active_project"`：返回当前会话的主工程名称（`active_project`）及其包含的成员仓库列表。
  `"index"`：检查索引健康状况（解析失败文件数、陈旧文件数、语言覆盖与健康评分）。
  `"graph"`：返回图谱节点与边数量按类型和语言的统计汇总（`per_repo_counts` 为估算值）。
  `"project"`：跨项目轻量只读符号查询（免切主工程，直接检索指定外部工程或已纳管仓中的符号；不改变当前 `active_project`）。
  `"checkouts"`：列出当前会话可见的工作树家族（`family`）、`checkout_id`（供顶层 `view` 切换视角用）与隔离路径状态。
  `"scopes"`：列出所有通过 `workspace_admin.save_scope` 保存的多仓/子路径复用作用域。
  `"proxy"`：查询远程 Gortex 代理（federation/proxy）花名册及其全局与会话生效状态。
内层 `arguments` 位置其他参数逐项说明：
  `"format"`（字符串，强制 `"json"`）：仅 `repos`、`info`、`active_project`、`index`、`graph`、`checkouts` 支持；`project`、`scopes`、`proxy` 无此参数，严禁传入。
  `"max_bytes"`（整数，推荐 `8400`）：仅 `repos`、`info`、`active_project`、`index`、`graph`、`checkouts` 支持；`project`、`scopes`、`proxy` 无此参数。
  `"compact"`（布尔）：仅 `index` 声明此字段；按强制无折叠原则严禁传 `true`（省略或 `false`）。
  `"project"`（字符串，`project` 操作必填）：目标工程名、每仓 project 标签或已纳管仓前缀（非文件路径）。
  `"query"`（字符串，`project` 操作必填）：要查询的符号关键词。
  `"limit"`（整数，可选，默认 `20`）：仅 `project` 操作支持，限制返回符号条数。
  `"family"`（字符串，可选）：仅 `checkouts` 支持，按 family ID、graph ID、仓前缀或路径过滤工作树家族。
  `"fields"`（字符串，可选）：仅 `checkouts` 支持，逗号分隔保留字段（如 `"family_id,primary_graph_id,checkouts"`）。
  `"max_tokens"`（整数，可选）：仅 `checkouts` 支持，响应 Token 预算上限。
  严禁传 `repo` / `workspace` / `root` / `cwd`：`workspace` 全量 9 个操作均在工作区级运行（仅 `project` 操作专设 `arguments.project`），严禁乱传 `repo` 等伪选键（`tools_workspace.go:51` 对 `repos`/`info` 传 `repo` 会直接拒绝）。
---
### 2. `workspace_admin` — 工作区治理、索引与富化（冷门面，共 14 个操作）
完整基准调用模板：
  普通冷封装型（10 个治理/索引/富化操作）：
    `workspace_admin`（`Arguments={"operation":"set_active_project","arguments":{"project":"my-app"}}`）
    `workspace_admin`（`Arguments={"operation":"save_scope","arguments":{"name":"core-svc","repos":"repo-a,repo-b","paths":"src/auth,src/api"}}`）
    `workspace_admin`（`Arguments={"operation":"reindex","arguments":{"path":"my-app","format":"json"}}`）
  混合型特例（`blame`/`coverage`/`sql_rebuild`/`temporal_verify` 4 个操作，`output` 放外层顶层）：
    `workspace_admin`（`Arguments={"operation":"blame","arguments":{"repo":"my-app"},"output":{"format":"json","max_bytes":8400}}`）
    `workspace_admin`（`Arguments={"operation":"coverage","arguments":{"profile":"coverage.out","repo":"my-app"},"output":{"format":"json","max_bytes":8400}}`）
`operation` 位置可选参数（全部 14 个）：
  A. 日常工作区治理（6 个）：
    `"set_active_project"`：热切换当前会话主工程上下文并持久化，影响后续默认查询作用域。
    `"track"`：运行时将新仓库或关联 Git worktree 注册纳管进工作区并立即建索引。
    `"untrack"`：从索引与配置取消跟踪某仓库并清理节点（注意：当操作会删除图谱行时，未传 `confirm: true` 仅作影响预览，不实际移除！）。
    `"save_scope"`：保存可复用的多仓+仓内子路径组合作用域（供后续 `search`、`analyze` 等通过 `scope` 复用）。
    `"delete_scope"`：按名称删除已保存的作用域。
    `"feedback"`：记录（`action:"record"`）或查询（`action:"query"`）上下文召回质量反馈（用于优化 `explore.context` / `explore.prefetch`）。
  B. 底层索引与 Git 富化写盘（4 个）：
    `"index"`：全量建索或重建指定绝对路径仓库的图谱（仅在索引损坏、首次手动入库或大面积外部变更未感知时用，严禁日常高频刷）。
    `"reindex"`：增量重索引（仅重解析变化文件、驱逐已删文件；支持按 `paths` 局部范围重索引，开销远低于 `index`）。
    `"enrich_churn"`：预计算并写入各文件与符号的 Git 变更热度（`meta.churn`），供 `analyze(kind="churn")` 极速读取。
    `"enrich_releases"`：预计算并写入发布版本 Tag 时间线（`meta.added_in` 及 `KindRelease` 节点），供 `analyze(kind="releases")` 读取。
  C. 修改图谱状态的混合型分析（4 个，源码 `facade_registry.go:246-254` 因修改持久化状态而归入本门面）：
    `"blame"`：结合 Git Blame 富化作者归属分析。
    `"coverage"`：导入 Go `cover.out` 覆盖率文件并写回图谱节点（内层 `arguments.profile` 必填）。
    `"sql_rebuild"`：重建 SQL 表与查询调用点关联边。
    `"temporal_verify"`：时序与历史一致性校验（涉及外部分析与缓存写入）。
其他参数位置逐项说明：
  内层 `arguments` 位置（按操作分组）：
    `set_active_project`：必填 `project`（字符串，工程名如 `"my-app"`，非文件路径）。
    `track`：必填 `path`（字符串，仓库绝对路径）；可选 `name`（字符串，自定义仓前缀名）、`as_worktree`（布尔，强制作为独立 worktree 纳管为 `<base>@<workspace>`）、`force`（布尔，仅运维/CLI 渠道可用，Agent 会话禁对根目录/家目录强用）。
    `untrack`：必填 `path`（字符串，路径或仓前缀）；可选 `confirm`（布尔，传 `true` 才真正删除行数据，省略仅预览）。
    `save_scope`：必填 `name`（作用域名称）+ `repos`（必须是逗号分隔字符串如 `"repoA,repoB"`，严禁传 JSON 数组！）；可选 `paths`（逗号分隔仓内子路径前缀如 `"src/auth,src/api"`）、`description`（描述）。
    `delete_scope`：必填 `name`（要删除的作用域名称）。
    `feedback`：必填 `action`（`"record"` 或 `"query"`）。
      `action:"record"` 可选：`task`（原任务描述）、`useful`（逗号分隔有用符号 ID 串）、`not_needed`（逗号分隔冗余符号 ID 串）、`missing`（逗号分隔遗漏符号 ID 串）、`tool_source`（`"smart_context"` 或 `"prefetch_context"`）。
      `action:"query"` 可选：`top_n`（整数，每类返回条数，默认 10）、`tool_source`（`"smart_context"` | `"prefetch_context"` | `"all"`）、`format`（固定 `"json"`）、`compact`（禁传 `true`）。
    `index`：必填 `path`（仓库绝对路径）；无 `format`。
    `reindex`：可选 `path`（仓库绝对路径或仓前缀，默认主仓）、`paths`（字符串数组 `["dir/a.go"]`，限定局部重扫文件或目录）、`format`（固定 `"json"`）。
    `enrich_churn` / `enrich_releases`：可选 `branch`（分支/Tag/SHA）、`path`（仓前缀或绝对根路径，空则扫全工作区）、`format`（固定 `"json"`）。
    `blame` / `coverage` / `sql_rebuild` / `temporal_verify`：
      `coverage` 必填 `profile`（`cover.out` 路径，绝对或相对仓根）；
      4 项均支持可选域过滤：`repo`（字符串，仓前缀）、`project`（工程名）、`scope`（Saved-Scope slug）、`workspace`（工作区 slug）；不可把 `format`/`max_bytes` 放进内层 `arguments`。
  外层顶层 `output` 位置：
    仅 `blame` / `coverage` / `sql_rebuild` / `temporal_verify` 4 个混合型操作支持：`output: {"format": "json", "max_bytes": 8400}`（禁传 `compact: true`）。
    其余 10 个操作严禁传顶层 `output`。
  仓选键禁传边界：除 `blame`/`coverage`/`sql_rebuild`/`temporal_verify` 支持 `{repo, project, scope, workspace}` 外，其余 10 个操作（`set_active_project`/`track`/`untrack`/`save_scope`/`delete_scope`/`feedback`/`index`/`reindex`/`enrich_churn`/`enrich_releases`）严禁传入 `repo`/`root`/`cwd`/`dir` 等伪仓选键（`save_scope` 专用逗号串 `repos`）。
---
## 一、 检索与探索中枢 (`search` [7] / `explore` [9])
> 检索策略门禁：
> 1. 排查未知逻辑可并发 `search.text`（字面量）+ `search.winnow`（`min_fan_in: 2`），或先 `explore.task`/`explore.context`。
> 2. `.vue` / `.tsx` / `.svelte` 模板事件与闭包通常不在全局导出符号中，必须首选 `search.text` 定位，禁先用 `symbols` 搜空。
> 3. 禁止开发任务随手调用 `explore.localize`（会激活终端锁拦截后续调用）；需要持续排查或改代码请用 `explore.task` / `explore.context`。
### 3. `search` — 精准检索与约束过滤（平铺门面，共 7 个操作）
完整基准调用模板：
  全文/正则检索：`search`（`Arguments={"operation":"text","query":"handle_save_user","options":{"repo":"my-app","path":"frontend/src/views","regexp":false,"limit":50},"output":{"format":"json"}}`）
  约束链检索：`search`（`Arguments={"operation":"winnow","query":"forward_handler","options":{"repo":"my-app","path_prefix":"src/api","min_fan_in":0,"limit":30},"output":{"format":"json","max_bytes":8250}}`）
  符号语义检索：`search`（`Arguments={"operation":"symbols","query":"validateToken","options":{"repo":"my-app","path":"internal/auth","limit":35},"output":{"format":"json","max_bytes":8400}}`）
  AST 结构/内置规则扫描：`search`（`Arguments={"operation":"ast","options":{"repo":"my-app","detector":"help"},"output":{"format":"json","max_bytes":8400}}`）
  文件查找：`search`（`Arguments={"operation":"files","query":"","options":{"repo":"my-app","glob":"**/*.rs","limit":100},"output":{"format":"json"}}`）
`operation` 位置可选参数（全部 7 个）：
  `"text"`：Trigram 加速的全文精确子串或 RE2 正则检索，返回命中行及其外层封闭符号 ID（`symbol_id`）。顶层 `query` 必填。
  `"winnow"`：多轴约束链检索（结合 BM25 文本与结构条件：`kind`、`language`、`fan_in`/`fan_out`、`community`、`path_prefix`、`churn`、`is_test`）。若未传顶层 `query`，则 `options` 必须至少含 1 个有效结构过滤条件（严禁只传 `min_fan_in: 0` 或仅传 `repo`/`project`/`ref`，否则必报空约束错误！）。
  `"symbols"`：支持驼峰分词、概念语义、BM25 排序与内联查询子句的符号/文档检索。顶层 `query` 必填（注意：会解析内联 `kind:`/`lang:`/`path:`/`repo:`/`project:`；若需搜字面冒号请改用 `text`，且勿写 `"A OR B"`）。
  `"ast"`：Tree-sitter AST 结构模式匹配或内置安全/卫生检测器扫描。`query` 与 `options.detector` 严格互斥：原生模式须传顶层 `query` + `options.language`；内置规则须传 `options.detector`（查目录传 `"help"`，勿传 `""` 且无 `query`）。
  `"files"`：按文件名子串或通配符收集文件清单（禁在逻辑排查时盲猜文件）。顶层 `query` 与 `options.glob` 至少必填其一（`query` 禁 `*`/`?` 通配符，通配放 `options.glob`）。
  `"artifacts"`：检索 `.gortex.yaml::artifacts` 登记的非代码知识资产节点（Schema/API 文档/基础设施等）。顶层 `query` 可选。
  `"completion"`：从种子符号执行 1 跳图扩展候选补全。顶层 `query` 必填。
其他参数位置逐项说明：
  顶层 `query` 位置：
    `text`、`symbols`、`completion` 必填；`files`（与 `options.glob` 至少其一）；`winnow`、`artifacts` 可选；`ast` 仅原生 AST 模式必填（与 `options.detector` 互斥）。
  `options` 位置（通用域选择器权限——核心防闪断）：
    `"repo"`（仓前缀）：`text`、`symbols`、`files`、`winnow`、`ast` 允许；`artifacts`、`completion` 严禁传入。
    `"project"`（工程名）：`text`、`symbols`、`files`、`winnow`、`ast` 允许；`artifacts`、`completion` 严禁传入。
    `"ref"`（引用标签）：仅 `symbols`、`winnow`、`ast` 允许；`text`、`files`、`artifacts`、`completion` 严禁传入。
    `"workspace"`（工作区 slug）与 `"scope"`（Saved-Scope slug）：仅 `text`、`symbols`、`files` 允许；`winnow`、`ast`、`artifacts`、`completion` 全部严禁传入！（传不存在的 `scope` 会直接报错）。
  `options` 位置（子路径过滤与专属字段）：
    `"path"`（字符串，逗号分隔仓内相对前缀，严禁带 `<repo>/` 前缀）：仅 `text`、`symbols`、`files` 支持。
    `"path_prefix"`（字符串，逗号分隔仓内相对前缀，严禁带 `<repo>/` 前缀）：仅 `winnow`、`ast` 支持。
    `text` 专属：`regexp`（布尔，默认 `false`；`true` 时须确保为合法 RE2 正则）、`limit`（整数，推荐 `35~80`，上限 `1000`）。
    `winnow` 专属：`kind`（逗号串：`function,method,type,interface,variable,contract`）、`language`、`community`、`min_fan_in`（`0|2`）、`min_fan_out`、`min_churn`、`is_test`（三态布尔）、`test_role`（`"test,benchmark,fuzz,example"`）、`limit`（推荐 `25~50`）、`paginate`（布尔）、`cursor`。
    `symbols` 专属：`kind`、`flavor`（`"struct|class|component|..."`）、`corpus`（`"code|docs|content|all"`）、`expand`（`"both|equivalence|llm|off"`）、`query_class`（`"auto|symbol|concept|path|signature"`）、`vocab_anchored`（布尔）、`max_per_file`、`limit`（推荐 `28~70`）、`paginate`、`cursor`、`debug`（布尔）。（服务端固定 `assist: "off"`，勿传）。
    `ast` 专属：`language`（原生 AST 必填：`go|python|javascript|typescript|ruby|java|kotlin|scala|rust|elixir|php|c|cpp|csharp|bash`）、`detector`（内置规则名或 `"help"`）、`exclude_tests`（布尔）、`min_fan_in_of_enclosing_func`（整数）、`limit`（推荐 `28~50`）。
    `files` 专属：`glob`（通配符串）、`fuzzy`（布尔，默认 `false`）、`limit`（推荐 `70~220`）。
    `artifacts` 专属：`kind`（`"schema|api|infra|doc"`）、`limit`（推荐 `20`）。
    `completion` 专属：`edge_kinds`（`"calls,references|all"`）、`seed_limit`、`max_seed_expansion`、`limit`（推荐 `25`）。
  `output` 位置：
    `"format"`：全 7 个操作均传 `"json"`。
    `"max_bytes"`：仅 `symbols`、`ast` 传 `8400`，`winnow` 传 `8250`；`text`、`files`、`artifacts`、`completion` 在 Schema 中未声明 `max_bytes`，严禁传入！
    `"fields"`（仅 `winnow`、`symbols` 支持，逗号分隔保留字段名）。
    `"compact"`：全门面严禁传 `true`。
---
### 4. `explore` — 上下文装配与任务探索（平铺门面，共 9 个操作）
> ⚠️ Gortex 源码实证纠偏（必读：`facade_tools.go:484-518, 1580-1586, 2117-2126`）：
> 1. `options.path` 与顶层 `path` 的真实行为：`normalizeFacadeAliases` 会把所有 `explore` 操作中的 `path`（无论放在顶层还是 `options.path`）一律转换为 `out["repo"] = path` 并删除 `out["path"]`！因此 `options.path` 在 `explore` 中不是仓内子目录过滤，而是仓库锚点；若要在 `explore.context` 按仓内子目录过滤，必须在 `task` 文本中使用内联 `path:src/foo`（由 `parseFieldQuery` 解析）或用 `save_scope` + `options.scope`！
> 2. `explore.localize` 双写 `task`：`handleExplore` 强校验顶层 `task` 必填，而部分客户端 Schema 检查 `options.task`——必须调 `localize` 时务必同时传顶层 `task` 与 `options.task`。
> 3. `options.new_user_task` 硬白名单：仅 `explore.task`、`explore.localize`（以及 `read.file`）合法且须为布尔值；传给 `context`/`closure`/`plan`/`prefetch` 等其余操作必报 `invalid_argument` 闪消！
完整基准调用模板：
  非终止任务邻域探索（开发排查首选）：`explore`（`Arguments={"operation":"task","task":"定位登录鉴权失败原因","options":{"repo":"my-app","max_symbols":10,"token_budget":1600}}`）
  聚焦上下文装配（支持内联 `path:` 过滤子目录）：`explore`（`Arguments={"operation":"context","task":"path:src/auth 定位登录鉴权失败原因","options":{"repo":"my-app","fidelity":"graded","token_budget":4000},"output":{"format":"json","max_bytes":8400}}`）
  依赖闭包构建：`explore`（`Arguments={"operation":"closure","options":{"files":"my-app/src/a.rs,my-app/src/b.rs","max_depth":6,"token_budget":4000,"repo":"my-app"},"output":{"format":"json"}}`）
  仓库概览：`explore`（`Arguments={"operation":"outline","options":{},"output":{"format":"json","max_bytes":8400}}`）
`operation` 位置可选参数（全部 9 个）：
  `"task"`：非终止邻域搜集（持续诊断、读多文件或改代码任务首选，不锁定后续工具）。
  `"localize"`：定位即止的终端证据提取（会激活 `localization_complete` 会话锁，拦截后续多轮探索，仅一句话定位即止时慎用；服务端固定注入 `localize: true`）。
  `"context"`：单次组装任务所需的最小聚焦上下文（含符号源码、调用关系、编辑计划，支持 `graded` 分级、Token 预估与增量 Delta）。
  `"closure"`：从指定种子文件或种子符号出发构建多跳传递依赖闭包上下文包（按图距离或随机游走近度排序）。
  `"outline"`：单次获取全库叙事级概览（语言分布、社区、热点、高频导入、入口点）。
  `"wakeup"`：获取 ~500 Token 极精简代码库速览摘要。
  `"plan"`：开局工具调用路径路由推荐（返回建议调用序列与预填参数）。
  `"prefetch"`：基于最近关注符号与任务预测下一步需要的上下文。
  `"suggest"`：针对陌生仓库生成 5~10 条冷启动探索问题建议。
其他参数位置逐项说明：
  顶层 `task` 位置：
    `task`、`context` 必填（`context` 支持内联 `path:src/foo`）；
    `localize` 顶层 `task` 与 `options.task` 建议同时传；
    `prefetch` 可选；
    `plan` 顶层禁写 `task`（必须写在 `options.task`）；
    `closure`、`outline`、`wakeup`、`suggest` 不传顶层 `task`。
  顶层 `path` 位置：
    仅作为仓库/工作树文件系统锚点（服务端会将其转为 `repo`）；日常建议统一使用 `options.repo`，避免与仓内子路径混淆。
  `options` 位置：
    域选择器（按源码 `tools_*.go` 与 `facadeFieldConsumed` 验证）：
      `"repo"`：`task`、`localize`、`context`、`closure` 允许；`outline`、`wakeup`、`plan`、`prefetch`、`suggest` 严禁传。
      `"project"` / `"scope"`：仅 `context`、`closure` 允许；其余 7 项 严禁传。
      `"workspace"` / `"ref"`：仅 `closure` 允许；其余 8 项 严禁传。
    `new_user_task`（布尔）：仅 `task`、`localize` 合法（且仅在新用户问题触发的首次调用传 `true`，同任务后续调用与重试禁传）；`context`/`closure` 等其余 7 项严禁传！
    `task` / `localize` 专属：`max_symbols`（默认 10）、`token_budget`（推荐 1600）、`task`（仅 `localize` 补填）。
    `context` 专属：`entry_point`（起始符号 ID 或文件路径）、`max_symbols`（默认 5）、`fidelity`（`"flat"` | `"graded"`）、`token_budget`（默认 8000，仅 `graded` 生效，推荐 `1600~4000`）、`estimate`（布尔，仅估算 Token 成本不返回正文）、`if_none_match`（ETag）、`delta_from`（增量对比 ETag）、`include_call_paths` / `include_flows` / `include_confidence`（布尔）。
    `closure` 专属：`files` 与 `symbols` 至少必填其一（均用逗号分隔字符串，带 `<repo>/` 前缀）、`edge_kinds`（逗号串）、`max_depth`（默认 6）、`max_nodes`（默认 400）、`rank`（`"distance"` | `"proximity"`）、`token_budget`（推荐 4000）。
    `wakeup` 专属：`max_tokens`（默认 500）、`top_communities`（4）、`top_entry_points`（5）、`top_hotspots`（5）。
    `plan` 专属：必填 `task`、可选 `max_calls`（默认 4）。
    `prefetch` 专属：`recent_symbols`（逗号分隔符号 ID 串）、`include_source`（看代码必传 `true`）、`limit`（默认 10）、`max_tokens`、`paginate`（布尔）、`cursor`。
    `suggest` 专属：`limit`（默认 8，上限 20）。
  `output` 位置：
    `context`、`outline`、`prefetch`：`{"format":"json","max_bytes":8400}`（禁 `compact: true`）；
    `closure`、`suggest`：`{"format":"json"}`（无 `max_bytes`）；
    `wakeup`：`{"format":"json"}` 或 `{"format":"markdown"}`（无 `max_bytes`）；
    `task`、`localize`、`plan`：不传 `output`。
---
## 二、 源码精读中枢 (`read` [7])
### 5. `read` — 源码与文件精读（平铺门面，共 7 个操作）
> 容器与防脱敏硬锁：
> 1. 平铺门面，严禁外包 `"arguments"`。
> 2. 已拿到完整 `<repo>/path::Symbol` ID 后优先用 `read.symbols`（批量 2~8 个）或 `read.source`（单个）；短名歧义会报 `symbol shorthand "..." is ambiguous`。
> 3. 绝对禁止折叠：`read.file` / `read.source` / `read.editing_context` 必须保持 `"compress_bodies": false`！
完整基准调用模板：
  分片精读文件原文：`read`（`Arguments={"operation":"file","target":{"file":"my-app/src/main.rs"},"options":{"offset":1,"limit":60},"context":{"compress_bodies":false},"output":{"max_bytes":8400}}`）
  精读单个符号源码：`read`（`Arguments={"operation":"source","target":{"symbol":"my-app/src/main.rs::main"},"context":{"compress_bodies":false,"context_lines":3},"output":{"format":"json","max_bytes":8400}}`）
  批量精读多个符号：`read`（`Arguments={"operation":"symbols","target":{"symbols":["my-app/src/main.rs::main","my-app/src/lib.rs::init"]},"options":{"include_source":true},"context":{"context_lines":3},"output":{"format":"json","max_bytes":8400}}`）
  获取文件符号大纲：`read`（`Arguments={"operation":"summary","target":{"file":"my-app/src/main.rs"},"options":{"repo":"my-app"},"output":{"format":"json","max_bytes":8400}}`）
`operation` 位置可选参数（全部 7 个）：
  `"file"`：按行号切片读取文件原文（适用于非代码配置、Markdown、脚本或确认具体行号片段；绝对禁传 `output.format`）。
  `"source"`：精读单个符号（函数/方法/类型）的完整未折叠源码与签名（`alias`: `symbol`/`symbol_source` 也会路由至此，但请统一写 `"source"`）。
  `"symbols"`：批量精读多个符号的签名、源码（门面默认注入 `include_source: true`）与 1 跳调用者/被调用者样本（推荐一次 2~8 个）。
  `"summary"`：获取文件内定义的所有符号结构大纲（不含源码体；`read` 中唯一允许 `repo` 等域过滤器的操作）。
  `"editing_context"`：改码前单次装配目标文件/符号的签名、直接依赖与直接调用方上下文。
  `"history"`：查询当前 Gortex 会话内已修改符号的计次与变动轨迹（查 Git 行级历史请用 `workspace_admin.blame` 或 `analyze(kind="churn")`）。
  `"artifact"`：读取 `.gortex.yaml::artifacts` 登记的非代码知识资产完整内容（单文件上限 64 KiB）。
其他参数位置逐项说明：
  `target` 位置（严格单属性）：
    `file`、`summary`：必填 `{"file": "<repo>/path/to/file"}`。
    `source`：必填 `{"symbol": "<repo>/path::Symbol"}`。
    `symbols`：必填 `{"symbols": ["<repo>/path::Sym1", ...]}`（严禁传空数组 `[]`；传单个 `{"symbol": "..."}` 会被服务端归一化为 `source`）。
    `editing_context`：必填 `{"file": "<repo>/..."}` 或 `{"symbol": "<repo>/...::Sym"}`（禁传 `symbols`）。
    `history`：可省略 `target`，或传 `{"symbol": "..."}`。
    `artifact`：必填 `{"artifact": "artifact::<path>"}`（或省略 `target` 改传 `options.path`）。
  `context` 位置：
    `"compress_bodies"`（布尔，必须显式传 `false`）：适用于 `file`、`source`、`editing_context`；绝对禁止设为 `true`。
    `"context_lines"`（整数，默认 `3`）：适用于 `source`、`symbols`，在符号首尾额外附带的上下文行数。
    `"max_lines"`（整数）：`file`、`source` 虽有此键，但准备读完整代码或改代码时严禁设置（以免截略源码）。
  `options` 位置（含严格的 `repo` 与 `new_user_task` 禁传边界）：
    域选择器（仅 `summary` 允许！）：`summary` 支持 `repo`、`project`、`scope`、`workspace`、`ref`（多仓查非当前主工程文件必传 `options.repo`）；其余 6 个操作（`file`/`source`/`symbols`/`editing_context`/`history`/`artifact`）严禁传入这 17 个仓选键与 `scope`！
    `"new_user_task"`（布尔）：仅 `read.file` 合法（仅新用户问题首次调用传 `true`）；其余 6 项严禁传！
    `file` 专属：必传 `offset`（1 基起始行号）+ `limit`（推荐 `50~80`，硬上限 `<=150`；高密度中文/长行收紧至 `35~60`）（注：服务端也支持 `start_line`/`end_line` 自动换算为 `offset`/`limit`，但推荐统一用 `offset`+`limit`）；可选 `allow_secrets`（布尔，读 `.env`/`*.yaml`/`*.toml`/`*.properties` 明文及开启 `physical_evidence: true` 时必须设 `true`，否则脱敏占位符导致后续改码失配或直接报错）、`physical_evidence`（布尔，不兼容 `git_ref`/`commit` 历史 `view`）、`digest`（`"sha256"`）、`if_none_match`（ETag）、`keep` / `fidelity_globs` / `max_chars`（无折叠原则下，非极端压缩单行文件不传）。
    `source` 专属：`allow_secrets`（布尔，默认 `false`）、`if_none_match`。
    `symbols` 专属：`include_source`（必须保持 `true`）、`if_none_match`。
    `summary` 专属：`max_tokens`、`if_none_match`。
    `editing_context` 专属：`detail`（`"full"` | `"brief"`）、`max_tokens`、`if_none_match`（编辑前禁传 `fidelity_globs` 省略）。
    `artifact` 专属：`path`（当省略 `target` 时使用）。
  `output` 位置：
    `file`：仅传 `{"max_bytes": 8400}`（绝对禁传 `format`！）。
    `source`、`symbols`、`summary`、`editing_context`：`{"format": "json", "max_bytes": 8400}`（禁传 `compact: true`）。
    `artifact`：`{"format": "json"}`（无 `max_bytes`）。
    `history`：不传 `output`（或 `{"compact": false}`，无 `format`/`max_bytes`）。
---
## 三、 修改与重构中枢 (`edit` [10] / `refactor` [6])
### 6. `edit` — 代码与文件修改（平铺门面，共 10 个操作）
> 键名防串记铁律：
> 单发 `edit.file` / `edit.symbol`：顶层用 `match` + `replacement` + `target`（严禁写 `old_string`/`old_source`）。
> 批量 `edit.batch`：`changes[]` 内用 `old_string`/`new_string`（`edit_file`）或 `old_source`/`new_source`（`edit_symbol`），严禁写 `match`/`replacement`/`target`/`null`！
> 遇 `tool_blocked_by_mode` 先调 `session(operation="planning_mode", arguments={"mode":"editing"})` 解锁，禁连闪重试。
完整基准调用模板（按顶层形状分类）：
  单文件单点替换（含跨文件并发首选）：
    `edit`（`Arguments={"operation":"file","target":{"file":"my-app/src/a.rs"},"match":"old_call()","replacement":"new_call()","guard":{"expected_occurrences":1},"dry_run":false}`）
  原子/同文件多处批量修改：
    `edit`（`Arguments={"operation":"batch","changes":[{"op":"edit_file","path":"my-app/src/a.rs","old_string":"sig_v1","new_string":"sig_v2"},{"op":"edit_symbol","id":"my-app/src/b.rs::caller","old_source":"sig_v1()","new_source":"sig_v2(ctx)"}],"dry_run":false,"options":{"transaction_id":"tx-sig-01"}}`）
  按符号单点替换：
    `edit`（`Arguments={"operation":"symbol","target":{"symbol":"my-app/src/b.rs::calc"},"match":"x + 1","replacement":"x + 2","dry_run":false}`）
  新建文件或整文件覆写：
    `edit`（`Arguments={"operation":"write","target":{"file":"my-app/src/new_mod.rs"},"content":"pub fn ready() -> bool { true }\n","dry_run":false}`）
  将内存虚拟分支落盘：
    `edit`（`Arguments={"operation":"apply_overlay","options":{"from":"experiment-1","into":"main","force":false}}`）
  脚手架生成：
    `edit`（`Arguments={"operation":"scaffold","target":{"symbol":"my-app/src/a.rs::old_fn"},"options":{"new_name":"new_fn"},"dry_run":false}`）
  产物导出（`docs` / `export_graph` / `wiki` / `skill`）：
    `edit`（`Arguments={"operation":"docs","options":{"path_prefix":"src/api"},"output":{"format":"markdown"}}`）
`operation` 位置可选参数（全部 10 个）：
  `"file"`：单文件字符串精准替换（或 `replace_all: true` 全量替换）；独享 `checkParseGate` 语法门禁、`guard` 守卫与真实 `dry_run` diff 预览，单点修改及跨不同文件独立修改首选。
  `"batch"`：跨文件强原子事务或同文件 `>= 2` 处叠加修改（支持 `edit_file`、`edit_symbol`、`move_file`、`delete_file` 混合，失败全回滚）；内部无 `checkParseGate`，源码改后必跑 `change.diagnostics`。
  `"symbol"`：限定在单个符号源码体内精准替换 1 处（无 `guard`、`replace_all` 与 `allow_parse_errors`）。
  `"write"`：新建文件或整文件全量覆写（带 `checkParseGate`）。
  `"apply_overlay"`：将内存 `overlay` 虚拟分支合并并真正写入磁盘（服务端固定 `to_disk: true`）。
  `"scaffold"`：参照已有符号模式生成新实现骨架与测试桩（注意服务端默认 `dry_run: true`，实写落盘必传 `"dry_run": false`）。
  `"docs"`：生成仓库活文档（支持 Markdown / JSON）。
  `"export_graph"`：导出知识图谱（支持 Cypher / GraphML / Mermaid）。
  `"wiki"`：生成代码库 Wiki 页面（服务端固定 `enhance: false` 本地生成，支持 Markdown / HTML）。
  `"skill"`：打包生成 Skill 产物（支持 JSON / GCX / TOON）。
其他参数位置逐项说明：
  `target` 位置：
    `file`、`write`：必填 `{"file": "<repo>/path/to/file"}`；
    `symbol`、`scaffold`：必填 `{"symbol": "<repo>/path::Symbol"}`；
    `batch`、`apply_overlay`、`docs`、`export_graph`、`wiki`、`skill`：严禁传入 `target`！
  顶层编辑载荷位置：
    `match`（字符串，`file`/`symbol` 必填）：精确待替换原文片段（3~8 行唯一锚点；报 `ambiguous_match` 时立即扩窗至 8~12 行）。
    `replacement`（字符串，`file`/`symbol` 必填）：替换后新文本。
    `content`（字符串，`write` 必填）：完整文件内容。
    `changes`（对象数组，`batch` 执行必填，仅查 `status_only` 时必须省略）：推荐 2~6 项、硬上限 8 项且 `< 4 KB`，严禁含 `null` 元素，4 种合法 `op`：
      `{"op":"edit_file","path":"<repo>/...","old_string":"...","new_string":"...","replace_all?":false}`
      `{"op":"edit_symbol","id":"<repo>/...::fn","old_source":"...","new_source":"..."}`
      `{"op":"move_file","source":"<repo>/...","destination":"<repo>/...","expected_sha256?":"..."}`
      `{"op":"delete_file","path":"<repo>/...","expected_sha256?":"..."}`
    `dry_run`（布尔）：`file`/`symbol`/`write`/`batch`/`skill` 默认 `false`（建议实写均显式传 `false`）；`scaffold` 默认 `true`（实写必传 `false`）。注：`batch` 的 `dry_run: true` 仅校验路径生命周期，不匹配 `old_string`。
  `guard` 位置（仅 `file` / `write` 适用）：
    `expected_occurrences`（整数，仅 `file`）：期望匹配次数（防误改/漏改）。
    `base_sha`（字符串，`file`/`write`）：读取时观测到的文件 Git Blob SHA-1（防并发漂移，也可放 `options`）。
  `options` 位置（含仓选键禁传边界）：
    域选择器：`file`、`symbol`、`write`、`batch`、`apply_overlay`、`scaffold`、`skill` 严禁传 `repo` / `project` / `workspace` / `scope` 等 17 个键！仅产物导出类允许：`docs`（`workspace`）、`export_graph`（`repo`、`scope`）、`wiki`（`repo`、`project`、`workspace`）。
    `file` 专属：`replace_all`（布尔，默认 `false`）、`allow_parse_errors`（布尔，跳过 `checkParseGate`）、`mutation_id`（幂等键）、`physical_evidence`（布尔）、`digest`（`"sha256"`）。
    `symbol` 专属：`base_sha`、`mutation_id`、`physical_evidence`、`digest`（严禁传 `allow_parse_errors` / `replace_all` / `guard`）。
    `write` 专属：`allow_parse_errors`、`mutation_id`、`physical_evidence`、`digest`。
    `batch` 专属：`transaction_id`（幂等事务 ID）、`status_only`（布尔，查事务状态时设为 `true` 并省略 `changes`）。
    `apply_overlay` 专属：必填 `from`（源虚拟分支名）、可选 `into`（默认 `"main"`）、`force`（布尔，默认 `false`）。
    `scaffold` 专属：必填 `new_name`（新符号名）。
    `docs`：`include`、`output_path`、`path_prefix`、`run_blame`、`since`、`top`、`workspace`。
    `export_graph`：`kinds`、`languages`、`repo`、`scope`、`max_communities`、`min_community`、`no_synthetic`、`output_path`、`output_dir`。
    `wiki`：`output_dir`、`repo`、`project`、`workspace`、`max_communities`、`min_community`、`force`、`wikilinks`、`no_contracts`、`no_docs`、`no_processes`。
    `skill`：`directory`、`skill_name`、`description`、`output_dir`、`include_patterns`、`ignore_patterns`、`compress`（保持 `false`）、`max_references`。
  `output` 位置：
    `batch`：推荐完全省略 `output`（或传 `{"compact": false}`）；绝对严禁传 `"output": null` 或 `compact: true`。
    `file`/`symbol`/`write`/`apply_overlay`/`scaffold`：不传 `output`（严禁传 `format`）。
    `docs`：`{"format": "markdown" | "json"}`；`export_graph`：`{"format": "cypher" | "graphml" | "mermaid"}`；`wiki`：`{"format": "markdown" | "html"}`；`skill`：`{"format": "json" | "gcx" | "toon"}`。
---
### 7. `refactor` — 语义级重构与 LSP 自动修复（平铺门面，共 6 个操作）
完整基准调用模板：
  全局重命名：`refactor`（`Arguments={"operation":"rename","target":{"symbol":"my-app/src/a.rs::old_fn"},"new_name":"new_fn","dry_run":false}`）
  安全级联删除：`refactor`（`Arguments={"operation":"delete","target":{"symbol":"my-app/src/a.rs::dead_fn"},"options":{"cascade":"apply"},"dry_run":false,"output":{"format":"json"}}`）
  LSP 一键自动修复：`refactor`（`Arguments={"operation":"fix_all","target":{"file":"my-app/src/a.rs"},"options":{"repo":"my-app"}}`）
`operation` 位置可选参数（全部 6 个）：
  `"rename"`：全仓拓扑协同重命名（重写定义、跨文件调用点/引用、接收器及嵌入旧名的测试函数名；带 `checkParseGate` 门禁）。
  `"delete"`：符号安全删除与可选级联清理（实删必须传 `dry_run: false`）。
  `"fix_all"`：循环收敛应用 LSP 自动修复诊断（直接写盘，无 `dry_run`，需 LSP 就绪）。
  `"apply_code_action"`：按 `change.code_actions` 返回的索引应用单个 LSP CodeAction（直接写盘，无 `dry_run`，行列 0-based）。
  `"move"`：当前仅支持 Go 语言，将符号搬移至目标文件/包并更新引用。
  `"inline"`：当前仅支持 Go 语言，内联符号到调用点。
其他参数位置逐项说明：
  `target` 位置：
    `rename`、`delete`、`move`、`inline`：必填 `{"symbol": "<repo>/path::Symbol"}`；
    `fix_all`、`apply_code_action`：必填 `{"file": "<repo>/path/to/file"}`。
  顶层专属字段：
    `new_name`（字符串，`rename` 必填）：新符号名。
    `destination`（字符串，`move` 必填）：目标文件路径（仅 Go）。
    `dry_run`（布尔，`rename`/`delete`/`move`/`inline` 支持，默认 `false`；`fix_all`/`apply_code_action` 无此参数）。
  `options` 位置：
    域选择器：仅 `fix_all` 与 `apply_code_action` 允许 `options.repo`；`rename`、`delete`、`move`、`inline` 严禁传 `repo` / `project` / `workspace` / `scope` 等 17 个键！
    `rename` 专属：`allow_parse_errors`（布尔，默认 `false`）。
    `fix_all` 专属：`kinds`（默认 `"quickfix,source.organizeImports"`）、`diagnostic_timeout_ms`（默认 `5000`）、`max_actions_per_iter`（默认 `50`）、`max_iterations`（默认 `5`）。
    `apply_code_action` 专属：必填 `index`（整数，来自 `change.code_actions`）、`start_line` / `start_char` / `end_line` / `end_char`（0-based）、`only`。
    `move` 专属：`target_package`（字符串）。
    `delete` 专属：`cascade`（`"off"` | `"preview"` | `"apply"`）、`cascade_into_tests`（布尔）、`force`（布尔）、`propagate`（布尔）。
    `inline` 专属：`delete_after`（布尔，默认 `true`）。
  `output` 位置：
    仅 `delete` 支持 `{"format": "json"}`（无 `max_bytes`）；其余 5 个操作不传 `output`。
---
## 四、 变更核验与质量中枢 (`change` [19])
> 容器与闭环门禁：
> 1. 平铺门面，严禁外包 `"arguments"`。源码改后必走 `change.detect` → `change.diagnostics` → `change.tests` 闭环（纯文档/纯注释免跑，无 LSP 时以原生构建兜底）。
> 2. JSON 字符串 vs 对象数组：`preview.workspace_edit`、`simulate.steps`、`contract.ranges`/`workspace_edit` 必须为 JSON 字符串（且 `steps` 禁传 `"[]"`）；而 `ranges.source.ranges` 必须是 JSON 对象数组 `[...]`。
### 8. `change` — 变更影响、诊断与合同裁决（平铺门面，共 19 个操作）
完整基准调用模板（按输入形制分类）：
  变更盘点：`change`（`Arguments={"operation":"detect","source":{"scope":"unstaged"},"options":{"repo":"my-app","limit":50},"output":{"summary_only":false}}`）
  实时 LSP 诊断：`change`（`Arguments={"operation":"diagnostics","source":{"file":"my-app/src/main.rs"},"options":{"repo":"my-app","wait":false}}`）
  影响/测试/守卫：`change`（`Arguments={"operation":"impact","target":{"symbol":"my-app/src/main.rs::main"},"output":{"format":"json","max_bytes":8400,"summary_only":false}}`）
  风险合同裁决：`change`（`Arguments={"operation":"contract","source":{"scope":"unstaged"},"options":{"repo":"my-app","lens":"api"},"output":{"format":"json"}}`）
  写操作回执查验：`change`（`Arguments={"operation":"receipt","options":{"mutation_id":"mut-01"}}`）
`operation` 位置可选参数（全部 19 个）：
  `"detect"`：盘点工作区未提交/已暂存/分支对比 Git 变更并映射到受影响符号与文件列表。
  `"diagnostics"`：提取指定文件实时 LSP 诊断信息（语法/类型/编译错误）。
  `"code_actions"`：查询指定文件/行范围可用的 LSP CodeAction 列表（索引供 `refactor.apply_code_action` 使用，0-based）。
  `"tests"`：沿调用图追踪受改动符号影响的相关测试文件与测试函数。
  `"guards"`：检查改动符号是否触发架构守卫规则告警。
  `"impact"`：按深度分级（`d=1/2/3`）评估改动符号或文件的风险爆炸半径（注意对无代码符号的文件会返回 `recoverable: true, condition: "file_not_indexed"`，勿重复调用）。
  `"contract"`：执行改前全流水线风险合同裁决（输出 `verdict: allow | warn | refuse`）。
  `"api_impact"`：分析 HTTP/gRPC/主题路由变更的跨服务影响。
  `"edit_plan"`：生成多符号编辑的依赖拓扑推荐顺序。
  `"verify"`：验证提议的接口签名变更会破坏哪些调用点。
  `"pattern"`：从示例符号中提取可复用的结构编码模式。
  `"ranges"`：将文件或 1-based 行区间转换为对应的 Gortex 符号 ID 清单。
  `"receipt"`：在写操作超时/流式中断后，按 `receipt`、`mutation_id` 或 `source.path` 查询磁盘落盘与图谱刷新状态。
  `"preview"`：在内存影子区预演单次 LSP `WorkspaceEdit`（不落盘）。
  `"simulate"`：在内存影子区预演多步 `WorkspaceEdit[]`（服务端固定 `keep: false`，不保留 overlay）。
  `"overlay_branches"`：列出当前会话内存 overlay 的分支清单。
  `"overlay_state"`：查看当前会话内存 overlay 覆盖文件列表与状态。
  `"compare_branches"`：对比两个命名分支在指定符号关系上的拓扑差异。
  `"compare_overlay"`：对比当前工作区与内存 overlay 在指定符号关系上的拓扑差异。
其他参数位置逐项说明：
  `target` 位置（严格单属性）：
    `tests`、`guards`、`edit_plan`：必填 `{"symbol": "..."}` 或 `{"symbols": [...]}`。
    `impact`：必填 `{"symbol": "..."}`、`{"symbols": [...]}` 或 `{"file": "<repo>/..."}`。
    `contract`：与非符号 `source` 二选一；传符号时用 `{"symbol": "..."}` 或 `{"symbols": [...]}`。
    `compare_branches`、`compare_overlay`：必填 `{"symbol": "..."}`。
    其余 12 个操作（`detect`/`preview`/`simulate`/`diagnostics`/`code_actions`/`receipt`/`api_impact`/`pattern`/`ranges`/`verify`/`overlay_branches`/`overlay_state`）严禁传入 `target`！
  `source` 位置：
    `detect`：`{"scope": "unstaged" | "staged" | "all" | "compare"}`。
    `diagnostics`、`code_actions`：必填 `{"file": "<repo>/..."}`。
    `api_impact`：`{"file": "<repo>/..."}`（与 `options.route` 至少其一）。
    `receipt`：`{"path": "<repo>/..."}`（或用 `options.receipt`/`mutation_id`）。
    `preview`：必填 `{"workspace_edit": "<WorkspaceEdit JSON字符串>"}`。
    `simulate`：必填 `{"steps": "<WorkspaceEdit[] JSON字符串，禁\"[]\">"}`。
    `pattern`：必填 `{"symbols": ["<repo>/...::Sym"]}`（底层取首项）。
    `ranges`：必填 `{"file": "<repo>/..."}` 或 `{"ranges": [{"file":"...","start_line":1,"end_line":20}]}`（对象数组，1-based）。
    `verify`：必填 `{"changes": [{"symbol_id":"...","new_signature":"..."}]}`。
    `contract`：非符号源时填 `{"scope?":"unstaged|staged|all|compare", "base?":"...", "path?":"...", "ranges?":"<JSON字符串>", "workspace_edit?":"<JSON字符串>"}`（⚠️ `start_line` / `end_line` 必须放 `options` 内，严禁放进 `source`！）。
  `options` 位置（含严格的 `repo` 许可白名单）：
    域选择器（仅以下 5 个操作允许，其余 14 个严禁传 `repo`/`project`/`workspace`/`scope`！）：
      `detect`：`repo`；
      `diagnostics`、`code_actions`：`repo`、`project`；
      `api_impact`：`repo`、`project`、`ref`；
      `contract`：`repo`。
    `detect` 专属：`base_ref`（`compare` 基准分支）、`limit`（默认 50/100）、`offset`（默认 0）。
    `preview` / `simulate` 专属：`diagnostics`（布尔，默认 `true`）、`diagnostics_timeout_ms`、`inherit_overlay`（布尔）；`simulate` 另有 `stop_on_error`（布尔，默认 `true`）。
    `diagnostics` 专属：`wait`（布尔，默认 `false`）、`timeout_ms`。
    `code_actions` 专属：`start_line`、`start_char`、`end_line`、`end_char`（均 0-based）、`only`。
    `receipt` 专属：`receipt`、`mutation_id`。
    `tests` / `edit_plan` 专属：`depth`（默认 `3`）。
    `contract` 专属：`risk_gate`（布尔）、`lens`（`"api"`）、`start_line`、`end_line`（1-based，与 `source.path` 配合）。
    `api_impact` 专属：`route`（路由模式串）。
    `pattern` 专属：`max_source_lines`（默认 `40`）。
    `ranges` 专属：`start_line`、`end_line`（1-based）。
    `compare_branches` / `compare_overlay` 专属：`a`、`b`（`compare_branches` 必填两分支名）、必填 `kind`（`"find_usages" | "get_callers" | "get_call_chain" | "get_dependencies" | "get_dependents"`）、`depth`、`limit`。
  `output` 位置：
    `guards`、`api_impact`：`{"format": "json", "max_bytes": 8400}`（禁 `compact: true`）。
    `impact`：`{"format": "json", "max_bytes": 8400, "limit": 100, "offset": 0, "summary_only": false}`（注意 `limit`/`offset`/`summary_only` 在 `output` 而非 `options`；禁传 `summary_only: true`）。
    `contract`、`ranges`：`{"format": "json"}`（无 `max_bytes`）。
    `pattern`：`{"max_bytes": 8400}`（无 `format`）。
    `detect`：`{"summary_only": false}`（无 `format`/`max_bytes`）。
    `tests`、`verify`：不传 `output` 或 `{"compact": false}`（无 `format`/`max_bytes`）。
    其余操作不传 `output`。
---
## 五、 架构分析中枢 (`analyze` [96] — 顶层必传 `kind`，严禁写 `operation`！)
> 源码核验三大重点（`facade_tools.go:2764-2940`、`tools_enhancements.go:160-228, 820-825`）：
> 1. 顶层鉴别键固定为 `"kind"`（严禁写成 `"operation"`）。
> 2. `target` 仅 4 个 `kind` 允许：`co_change`、`why`、`def_use` 必填 `target`，`impact` 可选 `target`；其余 92 个 `kind` 严禁传 `target`。
> 3. 域选择器白名单与 14 个全禁域 `kind`：
>    全禁域（14 个，严禁传 `repo` / `project` / `workspace` / `scope`）：`architecture`、`coupling`、`churn`、`co_change`、`untested`、`extraction`、`knowledge_gaps`、`surprising_connections`、`agent_config`（仅允许 `root`）、`inspection_catalog`、`replay`、`lint`、`citation`、`help`。
>    部分限域（2 个）：`contracts` 仅允许 `{repo, project, ref}`（禁 `scope`/`workspace`）；`why` 仅允许记忆域 `scope: "workspace"|"global"`（禁 `repo`/`project`/`workspace`）。
>    其余 80 个 `kind`（含 `cycles`、`unsafe_patterns`、`dbt_models` 等）：均允许 `{repo, project, scope, workspace}`（注意 `cycles` 的 `options.scope` 是文件路径/包前缀如 `"src/auth"`，不是 Saved-Scope slug！；`cross_repo` 的 `options.repo` 作跨仓边界过滤）。
### 9. `analyze` — 静态图谱分析、安全审计与架构度量（平铺门面，仅保留日常高频 `kind`；共 96 个）
完整基准调用模板：
  无 `target` 的通用架构/静态扫描：
    `analyze`（`Arguments={"kind":"cycles","options":{"repo":"my-app","scope":"src/auth"},"output":{"format":"json","max_bytes":8400}}`）
  带 `target` 的符号/文件影响或定义-使用链分析：
    `analyze`（`Arguments={"kind":"impact","target":{"symbol":"my-app/src/main.rs::main"},"options":{"repo":"my-app"},"output":{"format":"json","max_bytes":8400}}`）
  安全漏洞扫描：
    `analyze`（`Arguments={"kind":"sast","options":{"repo":"my-app","language":"rust","severity":"error,warning"},"output":{"format":"json","max_bytes":8400}}`）
  现场查询全量 96 个 `kind` 目录（需小众/特定框架分析时）：
    `analyze`（`Arguments={"kind":"help"}`）
`kind` 位置高频可选参数（日常编程核心 16 项）：
  结构与变更影响（6 项）：
    `"cycles"`：检测模块/包/文件循环依赖（`options.scope` 为路径或包前缀如 `"src/auth"`）。
    `"would_create_cycle"`：预检添加 `from_id -> to_id` 边是否成环（`options.from_id`, `to_id` 必填）。
    `"dead_code"`：无入边死代码检测（默认排除变量/字段/常量/CGo/linkname）。
    `"hotspots"`：高复杂度与高扇入耦合热点排名。
    `"impact"`：全库或指定符号/文件的综合爆炸半径评分（可选 `target`；服务端固定 `refresh_cochange: false`）。
    `"def_use"`：过程内定义-使用链（必填 `target:{symbol}`）。
  安全、卫生与静态检查（4 项）：
    `"sast"`：多语言 SAST 安全漏洞模式扫描（支持 `cwe`/`language`/`severity`/`detector`/`kinds_only`）。
    `"hygiene"`：代码卫生与反模式检查。
    `"lint"`：无 LSP 时运行外置 Lint（Go/Python/Shell；必填 `options.path`，禁域过滤，无 `output`）。
    `"todos"`：全库 `TODO`/`FIXME`/`HACK`/`XXX`/`NOTE` 追踪与责任人/工单过滤。
  并发与跨仓边界（4 项）：
    `"race_writes"`：潜在并发竞态写入点探测。
    `"field_writers"`：结构体/类字段的全部写入点追踪（排查状态共享风险）。
    `"contracts"`：跨仓 API 契约清单/校验/桥接（仅允许 `{repo, project, ref}`）。
    `"cross_repo"`：跨仓库边界调用/实现/继承边汇总（`options.repo` 作边界仓过滤）。
  辅助与按需展开（2 项）：
    `"untested"`：无测试路径可达的未测生产符号（禁域过滤）。
    `"help"`：返回全部 96 个 `kind` 的单行速查表（禁域过滤，无 `output`）。
  其余约 80 个中低频/特定生态 `kind`（如架构聚类 `architecture`/`clusters`/`coupling`/`co_change`、健康/覆盖率 `health_score`/`coverage_gaps`/`ownership`/`stale_code`、特定生态 `routes`/`dbt_models`/`k8s_resources`/`components`/`swiftui_views`、以及图谱内部诊断等）：日常无需背诵，需要时先调 `analyze`（`Arguments={"kind":"help"}`）或 `capabilities`（`Arguments={"domain":"analyze","operation":"<kind>","detail":"schema"}`）现查入参。
其他参数位置逐项说明：
  `target` 位置：
    `co_change`：必填 `{"file": "..."}` 或 `{"symbol": "..."}`；
    `why`、`def_use`：必填 `{"symbol": "..."}`；
    `impact`：可选 `{"file": "..."}` 或 `{"symbol": "..."}`（省略则输出全库爆炸半径排名）；
    其余 92 个 `kind`：绝对严禁传 `target`！
  `options` 位置：
    域选择器 `{repo?, project?, scope?, workspace?}`：适用于除上述 14 个全禁域与 `contracts`/`why` 外的全部 80 个 `kind`（注意 `cycles` 中 `scope` 是路径/包前缀；`cross_repo` 中 `repo` 是边界仓过滤；`contracts` 仅 `{repo, project, ref}`；`why` 仅 `scope: "workspace"|"global"`；`agent_config` 仅 `root`）。
    高频项常用选项：
      `would_create_cycle`：必填 `from_id`, `to_id`；
      `lint`：必填 `path`、可选 `language`（`"go"|"python"|"shell"`）、`timeout_ms`；
      `sast` / `hygiene`：`detector`、`language`、`severity`、`exclude_tests`（`sast` 另有 `cwe`、`kinds_only`）；
      `dead_code`：`include_variables`、`include_fields`、`include_constants`、`include_cgo_exports`、`include_linkname_targets`、`skip_cross_repo_nodes`（均布尔）；
      `hotspots`：`threshold`；`impact`：`id`、`ids`、`path`、`kinds`、`min_score`、`max_score`、`path_prefix`；
      `todos`：`tag`、`assignee`、`has_assignee`、`ticket`；`contracts`：`action`（`"list"|"check"|"validate"|"bridge"`）；`cross_repo`：`base_kind`（`"calls"|"implements"|"extends"`）；
      `untested`：`file_prefix`、`min_fan_in`；`def_use`：`id`。
  `output` 位置：
    无 `output`（6 个）：`communities`、`health`、`lint`、`recent_changes`、`help`、`agent_config`（仅可传 `compact: false`，无 `format`/`max_bytes`）。
    `{"format":"json"}` 无 `max_bytes`（12 个）：`architecture`、`coupling`（可加 `limit: 50`）、`co_change`（`limit: 20`）、`churn`（`limit: 100`）、`extraction`（`limit: 25`）、`inspection_catalog`、`inspections`、`why`（`limit: 20`）、`replay`（`limit: 25`）、`citation`、`knowledge_gaps`、`surprising_connections`（`limit: 25`）。
    `{"limit": 50}` 无 `format`/`max_bytes`（1 个）：`untested`。
    其余所有 `kind`：传 `{"format": "json", "max_bytes": 8400}`（`cross_repo`/`error_surface`/`unsafe_patterns` 可加 `limit: 200`；禁传 `compact: true`）。
---
## 六、 关系网络中枢 (`relations` [11])
> 核心门禁：改公共函数/类型签名前必先 `callers` 或 `usages`；查向下依赖唯一合法操作名为 `dependencies`（严禁自创 `callees`）；查引用用 `usages`/`references`（严禁写 `instantiations`）。
### 10. `relations` — 符号调用、依赖与继承关系查询（平铺门面，共 11 个操作）
完整基准调用模板：
  查调用方：`relations`（`Arguments={"operation":"callers","target":{"symbol":"my-app/src/mod.rs::init"},"options":{"limit":40,"exclude_tests":true},"output":{"format":"json","max_bytes":8400}}`）
  查声明点：`relations`（`Arguments={"operation":"declaration","target":{"query":"validateToken"},"options":{"limit":20},"output":{"format":"json"}}`）
  查导入路径：`relations`（`Arguments={"operation":"import_path","target":{"symbol":"my-app/src/mod.rs::Client"},"options":{"path":"src/handler.rs"}}`）
`operation` 位置可选参数（全部 11 个）：
  `"callers"`：向上查询谁调用了目标符号。
  `"dependencies"`：向下查询目标符号/文件依赖了哪些导入、调用与类型（唯一合法名，勿写 `callees`）。
  `"usages"`：按使用语境（参数类型/返回值/字段/泛型/调用/导入等）全量查找符号被使用位置。
  `"implementations"`：查找接口/抽象类的具体实现类型与方法。
  `"overrides"`：查找方法的重写子方法或被重写的父方法。
  `"hierarchy"`：查询类/接口的完整继承层次树（向上/向下/双向）。
  `"dependents"`：多跳影响面查询（谁直接或间接依赖目标符号；不支持 `target.file`）。
  `"cluster"`：以目标符号为中心提取指定半径内的局部交互子图。
  `"references"`：基于 AST 与文本证据核查符号的所有引用（不强制要求完整图谱边，可用 `options.name` 查短名）。
  `"declaration"`：根据代码使用点片段反向定位其原始声明符号。
  `"import_path"`：给定要使用的符号 ID 与目标文件，直接返回正确的导入语句/包路径。
其他参数位置逐项说明：
  `target` 位置（严格单属性）：
    `callers`、`dependencies`、`usages`、`implementations`、`overrides`、`hierarchy`、`dependents`、`cluster`、`import_path`：必填 `{"symbol": "<repo>/path::Symbol"}`。
    `references`：可选 `{"symbol": "<repo>/path::Symbol"}`（省略时须传 `options.name`）。
    `declaration`：必填 `{"query": "使用点代码片段"}`。
  `options` 位置：
    域选择器：
      前 8 项（`callers`、`dependencies`、`usages`、`implementations`、`overrides`、`hierarchy`、`dependents`、`cluster`）允许 `{repo, project, scope, workspace, ref}`；
      后 3 项（`references`、`declaration`、`import_path`）严禁传入 `repo` / `project` / `workspace` / `scope` 等 17 个键！
    通用置信度过滤（前 8 项 + `references` 支持 `min_tier`）：`min_tier`（可信度降序：`"lsp_resolved"` > `"lsp_dispatch"` > `"ast_resolved"` > `"ast_inferred"` > `"text_matched"`）、`include_speculative`（布尔，前 8 项支持）。
    `exclude_tests`（布尔）：仅 `callers`、`usages`、`references` 支持（`dependencies`、`dependents` 不支持！）。
    `callers` / `dependencies`：`depth`（默认 2）、`limit`（推荐 `40~50`）。
    `usages`：`context`（`"parameter_type|return_type|field|value|type|attribute|generic_arg|call|import"`）、`flavor`、`return_usage`（`"discarded|assigned|partially_ignored|returned|goroutine|deferred|argument|condition"`）、`group_by`（`"file"`）、`limit`（默认 50）、`max_tokens`。
    `overrides`：`direction`（`"children"|"parents"`）。
    `hierarchy`：`direction`（`"both"|"up"|"down"`）、`depth`（默认 5）、`include_methods`（布尔）。
    `dependents`：`depth`（默认 3）、`limit`（默认 50）。
    `cluster`：`radius`（默认 2）、`limit`（默认 50）。
    `references`：`name`（符号名）、`evidence_limit`（默认 50）。
    `declaration`：`kind`、`limit`（默认 20）、`path_prefix`（无 `<repo>/` 前缀）、`regex`（布尔，默认 `false`）。
    `import_path`：必填 `path`（目标使用文件的路径）。
  `output` 位置：
    前 8 项：`{"format": "json", "max_bytes": 8400}`（禁 `compact: true`）；
    `references`、`declaration`：`{"format": "json"}`（无 `max_bytes`，`declaration` 禁传 `"toon"`）；
    `import_path`：不传 `output`。
---
## 七、 链路追踪中枢 (`trace` [7])
### 11. `trace` — 跨函数路径、数据流、污点与控制流追踪（平铺门面，共 7 个操作）
完整基准调用模板：
  双端点路径追踪：`trace`（`Arguments={"operation":"path","target":{"symbol":"my-app/src/a.rs::fn1"},"to":{"symbol":"my-app/src/b.rs::fn2"},"options":{"max_depth":16,"k":1},"output":{"format":"json","max_bytes":8400}}`）
  污点追踪：`trace`（`Arguments={"operation":"taint","target":{"query":"req.Body"},"to":{"query":"db.Exec"},"options":{"max_depth":8,"limit":20},"output":{"format":"json","max_bytes":8400}}`）
  调用链展开：`trace`（`Arguments={"operation":"call_chain","target":{"symbol":"my-app/src/a.rs::fn1"},"options":{"depth":4,"limit":50},"output":{"format":"json","max_bytes":8400}}`）
  图查询 DSL：`trace`（`Arguments={"operation":"graph","options":{"query":"nodes kind=interface | traverse implements in","limit":100},"output":{"format":"json"}}`）
`operation` 位置可选参数（全部 7 个）：
  `"path"`：查找从起点符号 `target` 到终点符号 `to` 的最短/K-最短调用与引用路径（双端点）。
  `"flow"`：追踪从起点符号 `target` 到终点符号 `to` 的过程间数据流/控制流路径（双端点）。
  `"taint"`：从输入源关键字 `target` 到危险汇点关键字 `to` 的污点传播路径分析（双端点）。
  `"cfg"`：提取单个函数的过程内控制流图（CFG），可选导出 Mermaid（单端点）。
  `"call_chain"`：以目标符号为根展开完整多层调用链与行号（单端点）。
  `"walk"`：按自定义边类型和方向从起点符号执行可控预算图游走（单端点）。
  `"graph"`：执行 Gortex 图查询语言（DSL）管道查询（无端点）。
其他参数位置逐项说明：
  `target` 与 `to` 位置：
    `path`、`flow`：必填 `target: {"symbol": "起点ID"}` + `to: {"symbol": "终点ID"}`。
    `taint`：必填 `target: {"query": "源关键字"}` + `to: {"query": "汇关键字"}`（⚠️ 用 `query`，不用 `symbol`）。
    `cfg`、`call_chain`、`walk`：必填 `target: {"symbol": "符号ID"}`，严禁传 `to`。
    `graph`：严禁传 `target` 与 `to`。
  `options` 位置：
    域选择器：仅 `call_chain` 与 `walk` 允许 `{repo, project, scope, workspace, ref}`；其余 5 项（`path`/`flow`/`taint`/`cfg`/`graph`）严禁传入 `repo` 等 17 个键！
    `path`：`max_depth`（推荐 `8~24`）、`k`（默认 1）、`include_references`（默认 `true`）、`max_frontier`、`min_tier`。
    `flow`：`max_depth`（默认 8）、`max_paths`（默认 10）、`min_tier`。
    `taint`：`max_depth`（默认 8）、`limit`（默认 20）、`min_tier`。
    `cfg`：`mermaid`（布尔，默认 `false`）。
    `call_chain`：`depth`（默认 4）、`limit`（默认 50）、`max_tokens`、`min_tier`、`include_speculative`（布尔）。
    `walk`：`edge_kinds`（默认 `"calls"`）、`direction`（`"out"|"in"|"both"`）、`max_depth`（默认 8）、`token_budget`（默认 6000）、`community`。
    `graph`：必填 `query`（DSL 查询串）、可选 `limit`（默认 100）。
  `output` 位置：
    `path`、`flow`、`taint`、`cfg`、`call_chain`：`{"format": "json", "max_bytes": 8400}`（`call_chain` 严禁传 `compact: true`）。
    `walk`、`graph`：`{"format": "json"}`（无 `max_bytes`）。
---
## 八、 虚拟推演与认知记忆 (`overlay` [10] / `recall` [8] / `remember` [8])
> 冷门面通用原则：这 3 个门面均将全部业务参数（及 `format`/`max_bytes`）封装在内层 `"arguments": {...}` 中。
> 数组 vs 逗号串类型边界：`remember.memory` 的 `symbol_ids`/`file_paths`/`tags`/`supersedes`、`recall.surface` 的 `symbol_ids`/`file_paths`、`remember.note`/`notebook` 的 `tags`、`remember.risk_ack` 的 `symbols` 全部必须为逗号分隔字符串（如 `"a,b"`），严禁传 JSON 数组 `[...]`！
### 12. `overlay` — 内存虚拟分支推演（冷门面，共 10 个操作，均无 `format`）
完整基准调用模板：
  `overlay`（`Arguments={"operation":"fork","arguments":{"name":"exp-refactor","from":"main","activate":true}}`）
  `overlay`（`Arguments={"operation":"push","arguments":{"path":"my-app/src/a.rs","content":"..."}}`）
`operation` 位置可选参数（全部 10 个）：
  `"register"`：注册/初始化会话级 overlay。
  `"drop"`：销毁整个会话 overlay。
  `"keepalive"`：续期当前会话 overlay 的生存时间（TTL）。
  `"fork"`：创建新的虚拟内存分支。
  `"switch"`：切换当前活跃的虚拟内存分支。
  `"drop_branch"`：删除指定虚拟内存分支。
  `"push"`：向当前虚拟分支写入虚拟文件内容或删除标记。
  `"delete"`：在当前虚拟分支标记删除某个文件。
  `"merge"`：在内存中进行虚拟分支合并（服务端固定 `to_disk: false`，仅内存生效；真正落盘须用 `edit.apply_overlay`）。
  `"simulate"`：在保留 overlay 的前提下预演多步 `WorkspaceEdit`（服务端固定 `keep: true`）。
内层 `arguments` 位置参数逐项说明：
  `register`：`{ "workspace_id?": "..." }`；`drop` / `keepalive`：`{}`。
  `fork`：必填 `name`（字母数字/短横/下划线，`<=64` 字符，严禁含 `/`），可选 `from`、`activate`（布尔）。
  `switch` / `drop_branch`：必填 `name`。
  `push`：必填 `path`（`<repo>/...`），可选 `content`、`base_sha`、`deleted`（布尔）。
  `delete`：必填 `path`。
  `merge`：必填 `from`，可选 `into`（默认 `"main"`）、`force`（布尔，默认 `false`）。
  `simulate`：必填 `steps`（`WorkspaceEdit[]` JSON 字符串，禁 `"[]"`），可选 `diagnostics`（布尔，默认 `true`）、`diagnostics_timeout_ms`、`inherit_overlay`、`stop_on_error`（布尔，默认 `true`）。
  严禁传入仓库 `repo` / `project` / `workspace` 等 17 个伪选键。
---
### 13. `recall` — 认知记忆检索与召回（冷门面，共 8 个操作，均传 `"format":"json"`）
完整基准调用模板：
  `recall`（`Arguments={"operation":"surface","arguments":{"task":"修改鉴权逻辑","scope":"both","limit":10,"format":"json"}}`）
  `recall`（`Arguments={"operation":"distill","arguments":{"max_symbols":10,"max_files":10,"format":"json"}}`）
`operation` 位置可选参数（全部 8 个）：
  `"surface"`：按任务描述、符号 ID 或文件路径主动浮现相关历史记忆（服务端固定 `mark_accessed: false`）。
  `"distill"`：提炼当前会话/全部会话的核心记忆摘要（长对话压缩后恢复工作记忆首选）。
  `"memories"`：按文本、类型、标签、重要度等条件精确检索记忆条目。
  `"notes"`：检索开发笔记。
  `"notebook_list"`：列出笔记本条目清单。
  `"notebook_find"`：按关键字检索笔记本条目。
  `"notebook_show"`：查看指定 ID 的笔记本完整条目。
  `"onboarding"`：检查是否已满足入职/冷启动必需记忆基线。
内层 `arguments` 位置参数逐项说明：
  `surface`：`task`、`symbol_ids`（逗号串）、`file_paths`（逗号串）、`scope`（认知检索域：`"workspace" | "global" | "both"`，非 Saved-Scope）、`limit`（默认 10）、`min_score`、`excerpt_chars`（默认 320）、`include_superseded`（布尔，默认 `false`）、`format:"json"`。
  `distill`：`max_symbols`（10）、`max_files`（10）、`max_tags`（10）、`max_recent`（8）、`excerpt_chars`（240）、`session_id`（传 `"all"` 查全部）、`format:"json"`。
  `memories`：`text`、`kind`（`"invariant"|"constraint"|"convention"|"gotcha"|"decision"|"incident"|"reference"`）、`symbol_id`、`file_path`、`tag`、`source`、`author`、`scope`（`"workspace"|"global"|"both"`）、`min_importance`、`pinned_only`、`include_superseded`、`since`、`limit`（50）、`format:"json"`。
  `notes`：`text`、`symbol_id`、`file_path`、`tag`、`session_id`、`pinned_only`、`since`、`limit`（50）、`format:"json"`。
  `notebook_list`：`limit`（50）、`format:"json"`；`notebook_find`：`query`、`limit`（50）、`format:"json"`；`notebook_show`：必填 `id`、`format:"json"`。
  `onboarding`：`essential_kinds`、`min_per_kind`、`min_total`、`format:"json"`。
  严禁传入仓库 `repo` / `project` 等伪选键（且全组无 `max_bytes`）。
---
### 14. `remember` — 记忆写入、笔记归档与风险确认（冷门面，共 8 个操作）
完整基准调用模板：
  `remember`（`Arguments={"operation":"memory","arguments":{"title":"鉴权边界约定","body":"...","kind":"invariant","symbol_ids":"my-app/src/auth.rs::verify","scope":"workspace","format":"json"}}`）
  `remember`（`Arguments={"operation":"risk_ack","arguments":{"source":"symbols","symbols":"my-app/src/auth.rs::verify","repo":"my-app","format":"json"}}`）
`operation` 位置可选参数（全部 8 个）：
  `"memory"`：存储或更新一条长期工程记忆（不变量/约束/约定/踩坑/决策等）。
  `"edit_memory"`：按正则或字面量局部修改已有记忆内容。
  `"rename_memory"`：将记忆在 `"workspace"` 与 `"global"` 作用域间迁移。
  `"note"`：保存开发过程笔记。
  `"notebook"`：保存结构化笔记本条目。
  `"notebook_used"`：标记某笔记本条目已被采用。
  `"risk_ack"`：为高风险变更记录人工确认回执（服务端固定注入 `ack: true`）。
  `"suppress_finding"`：添加、列出或移除对静态分析告警的抑制规则（`remember` 中唯一支持 `max_bytes`）。
内层 `arguments` 位置参数逐项说明：
  `memory`：`title`、`body`、`kind`、`symbol_ids`（逗号串）、`file_paths`（逗号串）、`tags`（逗号串）、`importance`（`1..5`）、`confidence`、`scope`（认知写入域：`"workspace" | "global"`）、`source`、`pinned`（布尔）、`id`、`supersedes`（逗号串）、`superseded_by`、`no_autolink`（布尔）、`format:"json"`。
  `edit_memory`：必填 `id`, `pattern`, `replacement`，可选 `mode`（`"regex"|"literal"`）、`scope`（`"workspace"|"global"`）、`format:"json"`。
  `rename_memory`：必填 `id`, `to_scope`，可选 `from_scope`、`format:"json"`。
  `note`：必填 `body`，可选 `file_path`、`symbol_id`、`links`、`tags`（逗号串）、`pinned`、`id`、`no_autolink`、`format:"json"`。
  `notebook`：必填 `body`，可选 `title`、`tags`（逗号串）、`id`、`format:"json"`；`notebook_used`：必填 `id`、`format:"json"`。
  `risk_ack`（唯一允许 `repo` 的 `remember` 操作）：`source`（字符串枚举：`"auto"|"edit"|"diff"|"symbols"|"ranges"`，非对象）、`symbols`（逗号串）、`scope`（diff scope）、`base`、`path`、`start_line`、`end_line`、`ranges`（JSON 字符串）、`workspace_edit`（JSON 字符串）、`lens`、`risk_gate`、`repo`、`format:"json"`。
  `suppress_finding`：`action`（`"add"|"list"|"remove"`）、`identity_key`、`rule`、`category`、`symbol_id`、`file`、`source_line`、`line`、`reason`、`author`、`format:"json"`、`max_bytes: 8400`。
  除 `risk_ack` 允许 `repo` 外，其余 7 项严禁传仓库 `repo` 等 17 个伪选键。
---
## 九、 本地代码审查 (`review` [6])
### 15. `review` — 本地代码审查（平铺门面，本地常用 6 个操作，全量支持 `options.repo`）
完整基准调用模板：
  `review`（`Arguments={"operation":"run","source":{"scope":"unstaged"},"options":{"repo":"my-app","use_llm":false},"output":{"format":"json","max_bytes":8400}}`）
`operation` 位置可选参数（本地常用 6 个）：
  `"run"`：运行本地变更代码审查，输出结构化问题与风险提示。
  `"pack"`：生成完整审查数据包（含上下文与可选脱敏）。
  `"diff_context"`：返回 Git diff 受影响符号的图谱增强上下文（无 `format`/`max_bytes`）。
  `"questions"`：基于枢纽节点、介数中心性与社区结构自动生成待核查评审问题。
  `"sibling_context"`：提取变更文件/符号周边兄弟实现模式对照上下文。
  `"critique"`：对已有审查结果 `findings` / `prior_review` 进行二次复核挑刺。
其他参数位置逐项说明：
  `source` 位置：`scope`（`"unstaged"|"staged"|"all"|"compare"`；`questions` 无 `scope`）、`base`（基准分支/提交，除 `diff_context` 外均支持）、`diff`（原始 diff 字符串，仅 `run`/`pack`/`critique`）。
  `options` 位置：全量支持 `repo`；通用 `base_ref`、`max_tokens`；
    `run`/`pack`：`use_llm`（默认 `false`）；`pack` 另有 `include_pack`（`false`）、`scrub`（`false`）；
    `questions`：`ids`、`path_prefix`、`categories`、`hub_threshold`（8）、`min_betweenness`（20）、`min_community_size`（3）、`limit`（20）；
    `sibling_context`：`focus_file`、`focus_files`、`focus_symbol_id`；
    `critique`：`findings`、`prior_review`、`max_tokens`（默认 1500）。
  `output` 位置：除 `diff_context` 无 `format`/`max_bytes`（禁传 `compact: true`）外，其余 5 项均传 `{"format": "json", "max_bytes": 8400}`。
---
## 十、 缓冲提炼 (`response` [5])、会话治理 (`session` [8])、自省 (`capabilities` / `ask`)
### 16. `response` — 大输出缓冲提炼（冷门面，共 5 个操作）
完整基准调用模板：
  `response`（`Arguments={"operation":"grep","arguments":{"pattern":"error","response_id":"resp-01","context":2}}`）
  `response`（`Arguments={"operation":"export_context","arguments":{"task":"导出认证流程简报","token_budget":2000,"format":"json"}}`）
`operation` 位置可选参数（全部 5 个）：
  `"grep"`：对已缓存的大输出执行正则或字面量行匹配检索（必填 `pattern`）。
  `"slice"`：按行号区间 `[from, to]` 切片提取缓存输出。
  `"peek"`：预览缓存输出前 N 行。
  `"stats"`：列出当前会话近期缓存的 `response_id` 与字节/行数统计。
  `"export_context"`：生成可跨工具传递的便携式任务上下文简报（必填 `task`）。
内层 `arguments` 位置参数逐项说明：
  `grep`：必填 `pattern`，可选 `response_id`、`context`、`before`、`after`、`ignore_case`（布尔）、`literal`（布尔）、`max_matches`。
  `slice`：`from`（默认 1）、`to`、`response_id`。
  `peek`：`lines`（默认 20）、`response_id`。
  `stats`：`{}`。
  `export_context`：必填 `task`，可选 `entry_point`、`max_symbols`（5）、`token_budget`（2000，上限 8000）、`format`（强制传 `"json"`）。
  严禁传 `repo`/`project`/`workspace` 等 17 个键。
---
### 17. `session` — 会话状态与事件治理（共 8 个公开操作，⚠️ 均禁传顶层 `output`！）
完整基准调用模板：
  频道订阅型（`subscribe` / `unsubscribe`）：
    `session`（`Arguments={"operation":"subscribe","channel":"diagnostics","arguments":{}}`）
  内层封装型（其余 6 个操作）：
    `session`（`Arguments={"operation":"planning_mode","arguments":{"mode":"editing"}}`）
    `session`（`Arguments={"operation":"cursor","arguments":{"action":"goto","id":"my-app/src/main.rs::main","format":"json"}}`）
`operation` 位置可选参数（全部 8 个）：
  `"subscribe"` / `"unsubscribe"`：订阅或退订守护进程实时推送频道（展开为 `subscribe_<channel>` / `unsubscribe_<channel>`）。
  `"planning_mode"`：切换当前会话的只读规划态与可写编辑态（解除 `tool_blocked_by_mode` 拦截）。
  `"cursor"`：有状态图谱游标导航（在会话内维护当前定位点与回退栈，单跳漫游）。
  `"agents"`：多 Agent 协同注册表（宣告在线、同步焦点 cursor 与声明文件建议锁，防并发改冲突）。
  `"workflow"`：管理结构化开发工作流阶段锁（`explore -> edit -> verify`）。
  `"proxy_enable"` / `"proxy_disable"`：仅对当前 MCP 会话临时启用或禁用某个远程 Gortex 代理（断连自动还原）。
其他参数位置逐项说明：
  顶层 `channel` 位置（仅 `subscribe` / `unsubscribe` 必填，严禁塞进内层 `arguments`）：
    可选值：`"daemon_health"` | `"diagnostics"` | `"graph_invalidated"` | `"stale_refs"` | `"workspace_readiness"`。
  内层 `arguments` 位置：
    `planning_mode`：必填 `mode`（`"planning"` | `"editing"`）。
    `cursor`：必填 `action`（`"goto" | "into" | "up" | "sibling" | "back" | "where" | "read"`）、`id`（`goto` 必填符号 ID）、`select`（`into/up/sibling` 选第几个邻居或名称子串）、`format`（传 `"json"`）。
    `agents`：`action`（`"list" | "register" | "heartbeat" | "lock" | "unlock" | "unregister"`，默认 `"list"`）、`id`（Agent ID；除 `register` 外必填）、`name`、`cursor`、`status`、`paths`（逗号分隔路径串）、`format`（传 `"json"`）。
    `workflow`：必填 `action`（`"start" | "advance" | "status" | "stop"`）、可选 `mode`（`"block" | "warn"`）。
    `proxy_enable` / `proxy_disable`：必填 `slug`（远程代理标识）。
  顶层严格禁传项：
    全 8 个操作严禁传顶层 `output`；
    6 个内层操作顶层只允许 `{operation, arguments, view}`，严禁传 `require_exact` / `require_fresh` / `wait_deadline` 及 17 个仓选键。
---
### 18. `capabilities` 与 19. `ask` — 自省门面与条件研究门面
`capabilities` 完整基准调用模板：
  查全库门面总览：`capabilities`（`Arguments={"detail":"summary"}`）
  查单门面操作列表：`capabilities`（`Arguments={"domain":"change","detail":"summary"}`）
  查精确入参 Schema：`capabilities`（`Arguments={"domain":"edit","operation":"batch","detail":"schema"}`）
`capabilities` 参数逐项说明：
  `domain`（字符串，可选）：21 个门面名之一；省略时返回全部门面摘要。
  `operation`（字符串，可选）：该门面下的具体 `operation`（或 `analyze` 下的 `kind`）；省略时返回该门面下所有操作清单。
  `detail`（字符串，可选，默认 `"summary"`）：合法值仅 `"summary"` 或 `"schema"`（严禁传 `"operations"`）。
`ask` 完整基准调用模板：
  `ask`（`Arguments={"question":"该系统鉴权链路如何流转？","options":{},"output":{"format":"json"}}`）
`ask` 参数逐项说明：
  `question`（字符串，必填）：自然语言研究问题。
  `options` / `output`：可选参数与输出格式。前置门禁：当前未配置研究提供方时 `capabilities(domain="ask")` 返回 `operations: []`，切勿盲调。
---
## 第四部分：MCP 原生 Resource（13 个）、Prompt（3 个）与防闪断自查清单
### 1. MCP 原生 Resource（13 个 URI）
支持 MCP Resource 的宿主可直接读取以下 URI（免消耗工具调用轮次）：
`gortex://workspace`、`gortex://repos`、`gortex://active-project`、`gortex://index-health`、`gortex://stats`、`gortex://schema`、`gortex://report`、`gortex://session`、`gortex://guide`（支持按章读取 `gortex://guide/{topic}`）、`gortex://audit`、`gortex://surprises`、`gortex://god-nodes`、`gortex://questions`。
### 2. MCP 原生 Prompt（3 个）
`pre_commit`：参数 `{"scope":"unstaged","repo?":"..."}`
`safe_to_change`：参数 必填 `{"ids":"<逗号分隔符号ID串>"}`
`orientation`：无必填参数
### 3. Antigravity 执行遵从性与零闪断自查清单 (Compliance Checklist)
1.【容器隔离】：10 个平铺门面（含 `analyze` 用 `kind`）外层 `Arguments` 是否未套 `"arguments"`？6 个冷门面是否均封装在内层 `"arguments"`（仅 `workspace_admin` 4 项混合分析外附 `output`）？`session` 是否绝未传顶层 `output`（且 6 项内层操作未传 `require_exact`/`require_fresh`/`wait_deadline`）？
2.【开局探测】：是否已通过 `workspace`（`Arguments={"operation":"repos","arguments":{"format":"json","max_bytes":8400}}`）确认工作区模式、仓状态与 `<repo>/` 前缀？
3.【17 伪选键与双通道】：是否仅向第三部分各门面明确允许的 `operation`/`kind` 传递了精确的 `repo`/`project`/`workspace`/`scope`/`ref`（或 `agent_config` 的 `root`）？子路径前缀过滤（`search`、`analyze`）是否零 `<repo>/` 拼接？`explore.context` 按子目录过滤是否写进了 `task` 内联 `path:src/foo` 而非被劫持的 `options.path`？
4.【三大防闪锁】：调用 `explore.localize` 时是否同时传了顶层 `task` 与 `options.task`？`options.new_user_task` 是否仅用于新提问首次 `explore.task`/`localize` 或 `read.file`？`read.file` 读含敏配置时是否传了 `allow_secrets: true`？读取符号是否优先用了完整 `<repo>/file::Symbol`？
5.【`target`、`view` 与字符串类型】：`target`/`to` 是否严格仅含 1 个合法键（禁 `{}`、`null`、`path`），无 `target` 操作是否完全省略？`change.contract` 的 `start_line`/`end_line` 是否放在 `options`（非 `source`）？`remember`/`recall` 的 `symbol_ids`/`file_paths`/`tags`/`symbols` 是否传逗号分隔字符串而非数组？
6.【强制 JSON、无折叠与禁止压缩】：查询/分析类 `format` 接口是否一律传 `"format": "json"`，且 `read.file` 未硬塞 `format`？是否已全库禁用 `compress_bodies: true`（显式传 `false`）、`compact: true`、`summary_only: true` 与待改源码省略过滤？`read.file` 是否按 `limit: 50~80` 分段读取原文？
7.【未读禁盲改、已读不重读、稳态路由与快速熔断】：编辑前上下文是否已有未折叠原文（已读未变则直接改、绝不重复重读）？单点或跨不同文件互独立修改是否优先走 `edit.file(match/replacement)`？走 `edit.batch` 时是否仅用于原子/同文件多处叠加，且控量 `2~6` 项（硬上限 `8` 项/`<4 KB`）、绝不含 `changes:[null]`/`"output":null`、绝不混用 `match` 与 `old_`？`edit.batch` 是否严格执行 2 错降批（或直切 `match`）、3 错硬切 `match`？
8.【核验闭环、防溢出与 `capabilities` 首错停猜】：源码改后是否完成 `change.detect` → `change.diagnostics`（无 LSP 时原生构建兜底）→ `change.tests` 并跑原生测试？若出现 `output.txt` 是否只用 `response` 提炼？一旦发生任何参数错误是否立即停猜并查一次 `capabilities(domain, operation, detail:"schema")`？