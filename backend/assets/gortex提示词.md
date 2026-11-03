# Gortex 统一架构全景全生命周期规范契约 (Unified Gortex MCP Specification - v101 阶梯分级完备版)

## 【文档定位与阶梯分级导航】

本规范是 Gortex MCP 体系的**唯一权威执行规范**。针对多项目联合分析、高频代码重构与大型系统治理需求，本文档在**单一文件内部**设立了严谨的**四级执行位阶体系（L0 ~ L3）**：

* **【L0 级：全局统领宪法与多项目底座】（必读 · 核心最高位阶）**：开局多项目探测、双通道路径零拼接、大输出守卫、动态自省等全任务通用底线，模型在任何交互中必须 100% 无条件服从；
* **【L1 级：日常开发与多项目协同黄金闭环】（高频 · 协-查-读-写-验五大核心中枢）**：覆盖 95% 的多仓库协同、代码探测、精准研读、AST与物理批量原子写入、LSP 报错自检与差异核验；
* **【L2 级：深度拓扑与架构审计】（进阶 · 析-联-迹三大中枢）**：处理深层依赖网络、接口与实现反查、死锁/死代码/变动热点分析与全链路跨过程追踪；
* **【L3 级：协同推演与空间治理】（专项 · 虚拟沙箱/审查/PR/底层运维）**：涉及多分支内存推演、Git Forge 审查、记事本与工作区底层数据库管理。

---

## 【命令排版语法与调用准则】

1. **多选一候选间隔说明 `"|"`**：
   规范与示例中的 `"|"` 纯属文档排版的多选一说明，**绝非参数实际取值**！调用时必须且仅能选择传入单一合法字符串（合规正例：`analyze(operation="cycles")`、`change(operation="tests")`；严禁照抄包含 `"|"` 的字符串）。
2. **动态参数范围说明 `"~"`（如 `"limit": 25~50`, `"max_lines": 30~180`）**：
   规范中的 `"~"` 代表参数取值的**闭区间合法范围 `[最低下限 ~ 最大上限 Ceiling]`**。实际构造调用时，**严禁字面照抄包含 `"~"` 的字符串**，必须自主选定区间内的一个充沛整数（如 `limit: 45`）；严禁无故跌破下限，严禁超越上限；由 `max_bytes: 8400` 底层自动早停兜底。
3. **参数容器标准归属**：
   - 绝大多数业务门面一律平铺顶级字段：`operation`、`target`、`options`、`output`；严禁外层多余包裹 `arguments: {}` 或 `params: {}`；
   - **冷门面例外**：`workspace`、`workspace_admin`、`overlay`、`remember`、`recall`、`response` 属于底层严格限定的冷门面，其业务参数**必须且仅能封装在 `arguments: { ... }` 容器内**。

---

# 【L0 级：全局统领宪法与多项目底座】（最高强制位阶 · 每次必读）

### 铁律 1：外部文档只读豁免与 Gortex MCP 闭环契约
1. **外部系统规范文档只读豁免**：位于代码仓库外部系统目录的规范文件（如当前提示词），属于初始环境法定豁免文件，必须且仅能使用宿主原生只读工具（如 `read_file`）读取，严禁调用 Gortex MCP 读取该外部文件（Gortex 仅索引代码仓库，访问外部路径会触发安全拦截）。
2. **工程代码 100% Gortex 闭环**：外部文档预读完毕后，对工程项目代码的所有探测、检索、阅读、修改、验证与审查，**必须全面切换至 Gortex MCP 工具闭环**。
3. **原生工具显式跳出限制**：仅当用户输入中明确包含“使用原生工具”显式指令（如本地 Git 提交备份、外部分支检出等）时，方可单次跳出 Gortex MCP 执行原生操作，完成后立即重返 Gortex 闭环。

### 铁律 2：寻址定位 vs 范围过滤【双通道路径与零拼接终极铁律】
针对 Gortex 服务端底层索引机制，所有路径传参必须严格区分**寻址定位**与**范围过滤**两类容器：

```text
┌─────────────────────────────────────────────────────────────────────────────────┐
│ 通道 A:【全局定位寻址器】 (target.symbol / target.file / changes.path)           │
│ 作用范围: edit.file, edit.batch, edit.symbol, read.file, read.source,           │
│          relations.target, trace.target, trace.to                               │
│ 契约规则: 必须提供全局精确位置。在多仓库环境中，【合法且必须】包含仓库名前缀！  │
│ 正确范例: target: { file: "repo-main/frontend/src/views/UserView.vue" }         │
│          target: { symbol: "repo-main/src/api/handler.rs::process_request" }    │
│          changes: [{ path: "repo-main/src/api/handler.rs", ... }]               │
└─────────────────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────────────────┐
│ 通道 B:【仓库内部子路径过滤器】 (options.path 容器)                              │
│ 作用范围: search.symbols, search.winnow, search.text, search.files             │
│ 契约规则: 【绝对严禁】在 path 中拼接仓库名前缀！仓库限定必须单独使用 options.repo│
│ 底层机理: Gortex 索引树已剔除仓库名，底层使用 pathMatchesAnyPrefix 比对内部相对 │
│          路径。拼接仓库名会导致前缀比对永远为 false，引发 100% 搜空假象！       │
│ 正确范例: options: { repo: "repo-main", path: "frontend/src/views/UserView.vue" } │
│ 严重违规: options: { path: "repo-main/frontend/src/views/UserView.vue" } (搜空!)│
└─────────────────────────────────────────────────────────────────────────────────┘
```

### 铁律 3：开局多项目协同与四步前置探测状态机 (Probe-Before-Analyze Lifecycle)
在单守护进程联合图谱（Unified Graph）中，模型在接入新任务或排查多仓库前，**必须首先执行工作区前置探测**：
1. **Step 1 边界探测**：调用 `workspace(operation="repos", arguments: {})` 获取当前图谱已 track 的合法仓库清单，明确工程边界；
2. **Step 2 归属探测**：调用 `workspace(operation="active_project", arguments: {})` 确认当前会话聚焦的 Primary Project 与绑定状态；
3. **Step 3 索引健康诊断**：调用 `workspace(operation="index", arguments: {})` 获取解析就绪度与 health_score，确认 AST 置信度；
4. **Step 4 进入代码分析**：确认资产与主场后，方可合法下发后续代码命令。严禁未经探测直接盲目搜索，严禁凭空臆测“仓库未索引”而退回宿主工具。

### 铁律 4：多项目双穿透架构与主工程切域法则 (Dual-Penetration Multi-Repo Mandate)
在多仓库已 track 联合环境中，遵循严格的穿透与切域边界：
1. **只读轻量穿透（免切项目）**：
   - 跨库检索符号：调用 `workspace(operation="project", arguments: {project: "repo-core", query: "UserHandler"})`；
   - 跨库精读源码：使用包含外部仓库前缀的路径，如 `read(operation="file", target: {file: "repo-core/src/lib.rs"})`；
2. **深度批量修改与重构（必须切域）**：
   - 若需对外部仓库执行多文件修改（`edit.batch`）或回归测试（`change.tests`），**必须首先热切换主工程上下文**：
     `workspace_admin(operation="set_active_project", arguments: {project: "repo-core"})`；
   - 严禁向未声明 `repo` 属性的单库操作随意注入 `repo/project` 字段，杜绝 `no_repository_selector` 报错。

### 铁律 5：工具签名强类型契约、全链路 JSON 与字节兜底
1. **显式 Operation 契约**：凡调用 Gortex 门面，顶层必须显式传入 `"operation"` 字段，绝对严禁省略发起无头请求；
2. **统一 JSON 序列化**：所有工具调用必须显式传入 `output: { format: "json" }`，确保精确行号、AST 符号 ID 与拓扑关系零损失传输；
3. **字节上限硬锁**：检索与精读请求必须携带 `options: { max_bytes: 8400 }`（精读视窗可设为 `8250`），由底层引擎 `applyBudget` 自动早停兜底，杜绝 Token 溢出。

### 铁律 6：单轮全量批量原子交付与写故障退避自愈状态机
1. **单轮全量交付**：严禁切香肠式碎片化试探修改。单次任务涉及的所有修改点，必须一次性组装入 `edit(operation="batch", changes: [...])` 单轮原子化提交；
2. **写故障三步退避自愈状态机**：
   - **Step 1 拦截感知**：修改遇到行号漂移、上下文不匹配报错时，严禁盲目重试；
   - **Step 2 状态检视**：立即调用 `change(operation="detect")` 核验当前变动真实符号与未暂存范围；
   - **Step 3 基线对齐与二次提交**：调用 `read(operation="file")` 带精确 offset/limit 重新拉取最新行号基线，重新组装批量修改批次二次提交。

### 铁律 7：大输出防溢出安全隔离守卫 (Anti-Spill Guard)
当 Gortex MCP 工具输出触发安全截断并提示：
`The output was large and was saved to: output.txt`
1. **绝对禁令**：**严禁调用任何宿主原生工具（如 read_file、cat、Get-Content）直接读取该 output.txt**！
2. **法定操作**：必须且仅能使用 Gortex 专属溢出提炼工具：
   - 模式 A（关键词定向提取）：`response(operation="grep", arguments: {pattern: "...", context: 5})`
   - 模式 B（行号精确视窗切片）：`response(operation="slice", arguments: {from: 1, to: 100})`

### 铁律 8：终极动态自省探针 (Capabilities Schema Discovery)
当模型对任何门面的参数结构、容器名称（`target` vs `arguments`）或专属选项存在任何疑问或犹豫时，**绝对严禁瞎猜参数**！唯一法定动作：调用自省探针查询服务端权威即时 Schema：
```json
capabilities({
  "domain": "change",
  "operation": "preview",
  "detail": "schema"
})
```

### 铁律 9：运行时权威裁决五大位阶 (Conflict Resolution Hierarchy)
当规则规范、源码实现与运行时表现产生分歧时，必须严格自顶向下裁决执行，高位阶效力绝对压制低位阶：
1. **第一位阶**：当前工具返回的真实运行时响应（error 报错、guard 拦截、view 视图、completion 状态）；
2. **第二位阶**：当前运行时 `capabilities` 与 Schema 返回的权威结构规范；
3. **第三位阶**：物理文件系统与代码仓库的真实状态（工作区绑定状态、Git 树状态）；
4. **第四位阶**：Gortex 服务端守护进程源码实现逻辑；
5. **第五位阶**：本文档书面规则。

---

# 【L1 级：日常开发与多项目协同黄金闭环】（高频聚焦 · 协-查-读-写-验五大核心中枢）

本层级规范覆盖 95% 的多仓库协同、业务开发、缺陷修复与功能实现全流程。模型在处理代码任务时，必须严格遵循“环境探测 $\to$ 检索定界 $\to$ 精准研读 $\to$ 原子批量修改 $\to$ 质量自检与核验”五步流水线闭环。

---

## 零、 工作区与多项目协同中枢规范契约 (Workspace & Admin Hub)

### 1. 核心工作区与多仓协同准则
1. **先探测再分析代码原则 (Probe Before Analyze)**：
   在处理多项目关联分析或陌生仓库任务时，首动必须先调用 `workspace.repos` 确认已纳管仓库资产，调用 `workspace.active_project` 确认当前主工程，调用 `workspace.index` 确认图谱健康分。
2. **多项目轻量穿透 vs 深度切域边界**：
   - **只读轻量穿透**：主场在 `repo-main`，只需查阅 `repo-core` 的符号时，使用 `workspace.project(project="repo-core", query="...")` 或跨仓只读 `read.file(target: {file: "repo-core/..."})`；
   - **深度修改切域**：若要对外部项目 `repo-core` 进行文件修改或回归测试，必须调用 `workspace_admin.set_active_project(project="repo-core")` 将主上下文热切换过去。
3. **冷门面 arguments 封装铁律**：
   `workspace` 与 `workspace_admin` 的所有业务参数**必须且仅能封装在 `arguments: { ... }` 容器内**，严禁顶层平铺！

### 2. 工作区与多项目协同分流状态机矩阵 (Trigger-Action Matrix)

| 触发场景 (Trigger) | 涉及门面 | 唯一法定操作 (Action) | 核心必填参数与门禁规则 |
| :--- | :--- | :--- | :--- |
| **开局探测当前工作区纳管的全部已 track 仓库清单** | `workspace` | `workspace(operation="repos")` | 强制容器: `arguments: {}` (严禁传 repo 属性) |
| **开局探测当前会话绑定的主场工程与主项目** | `workspace` | `workspace(operation="active_project")` | 强制容器: `arguments: {}` |
| **开局探测图谱健康分、解析失败数与过期文件** | `workspace` | `workspace(operation="index")` | 强制容器: `arguments: {}`，检查健康度 |
| **免切主工程，跨项目单次穿透检索外部仓库符号** | `workspace` | `workspace(operation="project")` | 强制容器: `arguments: { project: "外部项目名", query: "符号名" }` |
| **获取全局知识图谱节点、边数与内存占用指标** | `workspace` | `workspace(operation="graph")` | 强制容器: `arguments: {}` |
| **获取工作区根目录、绑定模式与排除标记** | `workspace` | `workspace(operation="info")` | 强制容器: `arguments: {}` |
| **诊断具体物理文件绑定的工作树路由与代际** | `workspace` | `workspace(operation="checkouts")` | 强制容器: `arguments: { family: "项目名" }` |
| **将当前会话的主上下文热切换至指定外部工程** | `workspace_admin` | `workspace_admin(operation="set_active_project")` | 强制容器: `arguments: { project: "目标工程标识" }` (严禁传文件路径) |
| **注册新代码仓库至全局守护进程联合图谱** | `workspace_admin` | `workspace_admin(operation="track")` | 强制容器: `arguments: { path: "绝对目录", name: "仓库名" }` |
| **从全局图谱注销指定代码仓库** | `workspace_admin` | `workspace_admin(operation="untrack")` | 强制容器: `arguments: { path: "绝对目录" }` |
| **触发指定代码仓库的底层全量/增量图谱重建** | `workspace_admin` | `workspace_admin(operation="reindex")` | 强制容器: `arguments: { path: "仓库名或相对路径" }` |

### 3. 工作区与多项目协同标准调用模板

```json
// 模板 1: 开局四步探测之仓库边界与主场工程核验 (workspace.repos & workspace.active_project)
workspace({
  "operation": "repos",
  "arguments": {}
})
```
```json
workspace({
  "operation": "active_project",
  "arguments": {}
})
```

```json
// 模板 2: 免切主工程，跨项目单次穿透检索外部仓库符号 (workspace.project)
workspace({
  "operation": "project",
  "arguments": {
    "project": "repo-core",
    "query": "ConnectionPool"
  }
})
```

```json
// 模板 3: 深度修改外部项目前，热切换主工程上下文 (workspace_admin.set_active_project)
workspace_admin({
  "operation": "set_active_project",
  "arguments": {
    "project": "repo-core"
  }
})
```

---

## 一、 检索中枢规范契约 (Search Hub)

### 1. 核心检索准则与防御门禁
1. **业务逻辑首动并发探测（双探针原则）**：
   凡排查代码业务逻辑或功能实现，首动必须发起双通道并发检索：
   * 通道 1（字面量探针）：`search.text(query="核心业务词/报错文本", options: {limit: 35~80, max_bytes: 8400})`
   * 通道 2（拓扑雷达）：`search.winnow(query="实体动作词元", options: {limit: 25~50, min_fan_in: 2, max_bytes: 8250})`
2. **前端单文件组件 (Vue/React SFC) 探针法则**：
   前端单文件组件（如 `.vue`、`.tsx`）内的内部数据、闭包函数与模板事件绑定（如 `@click="handle_edit"`），属于内部匿名闭包或模板指令，**不在全局 AST 导出符号索引库中**。排查此类文件必须首选 `search(operation="text")`，严禁直接使用 `symbols` 导致误杀搜空。
3. **AST 架构场景拓扑升格死锁 (Escalation Gate)**：
   若首轮并发中 `search.winnow` 未命中，**绝对严禁直接进入 `read.file` 全量裸读或直接进入 `edit.batch` 盲改**！唯一法定动作：从 `search.text` 命中行中穿透提取包裹该代码的外层真实函数名/结构体名，强制发起第二轮定向拓扑定界：
   `search(operation="winnow", query="真实符号名", options: {min_fan_in: 0, limit: 25~50, max_bytes: 8250})`，获取调用度与拓扑依赖后方可进入读写环节。
4. **禁以文件名代替架构检索令**：
   无论用户指令中是否包含看似文件名、组件名的词汇（如 `handler` / `UserView`），严禁首动调用 `search.files` 投机取巧！必须经由双探针锁定中枢。仅当纯粹确认文件存在性或收集路径名单时方可使用 `search.files`。

### 2. 检索中枢确定性分流状态机矩阵 (Trigger-Action Matrix for Search)

| 触发场景 (Trigger) | 唯一法定操作 (Action) | 核心必填参数与门禁规则 |
| :--- | :--- | :--- |
| **排查错误日志、物理字符串、Vue/React 模板事件绑定** | `search(operation="text")` | `query: "文本"`, `options: {limit: 35~80, max_bytes: 8400}`。禁止在 `options.path` 中拼仓库名！ |
| **首次探索陌生架构、核心业务主干入口** | `search(operation="winnow")` | `query: "词元"`, `options: {min_fan_in: 2, limit: 25~50, max_bytes: 8250}`。 |
| **已知真实符号名，补齐扇入扇出依赖拓扑** | `search(operation="winnow")` | `query: "真实符号名"`, `options: {min_fan_in: 0, limit: 25~50, max_bytes: 8250}`。 |
| **检索公共类、接口、结构体定义点** | `search(operation="symbols")` | `query: "符号名"`, `options: {kind: "struct/class", assist: "off", limit: 28~70}`。 |
| **根据语法树模式跨文件匹配特定语法结构** | `search(operation="ast")` | `query: "Tree-sitter模式串"`, `options: {language: "rust/ts", limit: 28~50}`。 |
| **跨项目搜寻特定后缀或命名规律的文件集合** | `search(operation="files")` | `query: "名称片段"`, `options: {glob: "*.rs", limit: 70~220}`。 |
| **检索已编译二进制/中间产物或构建产物元数据** | `search(operation="artifacts")` | `query: "产物名"`, `options: {limit: 20, max_bytes: 8400}`。 |
| **补全符号名称或模糊代码补全探针** | `search(operation="completion")` | `query: "前缀词元"`, `options: {limit: 20}`。 |

### 3. 检索中枢标准调用模板

```json
// 模板 1: 物理字符串与报错日志精准探针 (search.text)
search({
  "operation": "text",
  "query": "handle_save_user",
  "options": { "repo": "repo-main", "path": "frontend/src/views", "limit": 50, "max_bytes": 8400 },
  "output": { "format": "json" }
})
```

```json
// 模板 2: 真实符号升格定向拓扑检索 (search.winnow)
search({
  "operation": "winnow",
  "query": "forward_handler",
  "options": { "repo": "repo-main", "limit": 30, "min_fan_in": 0, "max_bytes": 8250 },
  "output": { "format": "json" }
})
```

```json
// 模板 3: 符号定义点检索 (search.symbols)
search({
  "operation": "symbols",
  "query": "UserProfileRequest",
  "options": { "repo": "repo-main", "path": "models", "kind": "struct", "limit": 40, "max_bytes": 8400 },
  "output": { "format": "json" }
})
```

---

## 二、 源码精读中枢规范契约 (Read Hub)

### 1. 核心精读准则与防御门禁
1. **符号级精读绝对优先原则**：
   只要前序步骤获得了符号 ID，**首选且必须优先调用 `read.symbols` 或 `read.source`**；严禁退化为读取大段物理文件。
2. **物理文件切片安全锁 (Slicing Protection)**：
   仅当必须研读无导出符号的代码（如配置文件、脚本、Vue 模板布局）时，方可调用 `read.file`。**严禁不带行号范围盲读整文件！**
   必须显式声明 `options: { offset: 行号, limit: 行数 }`，且每次读取行数 `limit` 必须严格限制在 **150 行以内**（推荐 30~80 行）。
3. **Git 符号演进溯源机制 (`read.history`)**：
   在分析复杂业务逻辑、重构历史成因或 Bug 引入背景时，调用 `read.history` 提取该符号的历史 Git 提交演进与责任记录。

### 2. 精读中枢分流状态机矩阵 (Trigger-Action Matrix for Read)

| 触发场景 (Trigger) | 唯一法定操作 (Action) | 核心必填参数与门禁规则 |
| :--- | :--- | :--- |
| **已获得 2~8 个候选符号 ID，批量提取完整实现** | `read(operation="symbols")` | `symbols: ["repo-main/...#100::fn1", "repo-main/...#200::fn2"]`。 |
| **精准研读单个核心函数/类的完整定义源码** | `read(operation="source")` | `target: { symbol: "repo-main/src/handler.rs::handle_req" }`。 |
| **追溯某函数/类的历史 Git 提交记录与演进背景** | `read(operation="history")` | `target: { symbol: "repo-main/src/handler.rs::handle_req" }`，获取责任提交链。 |
| **物理非符号文件、配置或单文件组件切片研读** | `read(operation="file")` | `target: { file: "repo-main/path/to/file" }`, 强锁 `options: { offset: N, limit: <=150 }`。 |
| **大文件骨架大纲提取（初探文件结构）** | `read(operation="summary")` | `target: { file: "repo-main/path/to/file" }`, 产出骨架大纲与各函数起始行号。 |
| **修改前装配局部符号拓扑与直连依赖** | `read(operation="editing_context")` | `target: { file: "repo-main/..." }`, `options: { keep: "all/local/direct" }`。 |
| **读取特定构建产物文件或二进制元数据** | `read(operation="artifact")` | `target: { artifact: "产物标识" }`。 |

### 3. 精读中枢标准调用模板

```json
// 模板 1: 批量符号深度精读 (read.symbols)
read({
  "operation": "symbols",
  "symbols": [
    "repo-main/src/api/handler.rs#script:120::handle_request",
    "repo-main/src/db/pool.rs#script:45::acquire_connection"
  ],
  "output": { "format": "json" }
})
```

```json
// 模板 2: 符号历史 Git 演进与责任追溯 (read.history)
read({
  "operation": "history",
  "target": { "symbol": "repo-main/src/api/handler.rs::handle_request" },
  "options": { "limit": 10, "max_bytes": 8400 },
  "output": { "format": "json" }
})
```

```json
// 模板 3: 物理文件行号视窗安全切片 (read.file)
read({
  "operation": "file",
  "target": { "file": "repo-main/frontend/src/views/UserView.vue" },
  "options": { "offset": 120, "limit": 40, "max_bytes": 8400 },
  "output": { "format": "json" }
})
```

---

## 三、 代码修改与重构中枢规范契约 (Edit & Refactor Hub)

### 1. 核心修改准则与防御门禁
1. **全局寻址与前缀契约**：
   `edit.file` / `edit.write` 的 `target: { file: "..." }` 以及 `edit.batch` 的 `changes[].path` 作为全局物理定位器，**在多仓库环境中合法且推荐携带仓库名前缀**（如 `repo-main/src/...`），确保物理磁盘写入精确映射。
2. **唯一法定落地：单轮全量批量原子交付 (`edit.batch`)**：
   凡涉及多处代码修改或跨文件改动，**必须统一组装为单个 `edit.batch` 一次性提交**！严禁切香肠式逐步尝试。
3. **AST 符号级精准替换武器 (`edit.symbol`)**：
   当对特定函数/方法进行整体逻辑重写时，优先使用 `edit(operation="symbol")`，直接传入 `target: { symbol: "id" }` 与 `replacement: "新函数体源码"`。AST 引擎将在语法树层面完成节点替换，**彻底免除多行文本空白符、缩进与 CRLF/LF 换行符不匹配的报错烦恼**！
4. **写故障退避自愈状态机**：
   若发生修改冲突或行号漂移报错：
   - 严禁连续盲目重放；
   - 立即调用 `change.detect` 检视当前代码改动影响面；
   - 调用 `read.file` 重新读取基线行号，重新组装 `edit.batch` 完整批次后二次提交。

### 2. 修改与重构分流状态机矩阵 (Trigger-Action Matrix for Edit & Refactor)

| 触发场景 (Trigger) | 涉及门面 | 唯一法定操作 (Action) | 核心必填参数与门禁规则 |
| :--- | :--- | :--- | :--- |
| **单文件或跨多文件同时修改多处代码** | `edit` | `edit(operation="batch")` | 聚合所有改动：`changes: [{ path: "...", replacements: [{ target: { text }, replacement }] }]`。 |
| **函数/方法整体逻辑重塑（免文本锚点）** | `edit` | `edit(operation="symbol")` | `target: { symbol: "id" }`, `replacement: "新函数体完整源码"` (AST 节点精准替换)。 |
| **单文件单处确定性小修补** | `edit` | `edit(operation="file")` | `target: { file: "...", text: "精准旧文本" }`, `replacement: "新文本"`。 |
| **在项目中新建全新文件** | `edit` | `edit(operation="write")` | `target: { file: "..." }`, `content: "新文件完整正文"`。 |
| **将内存虚拟推演分支改动批量落入物理磁盘** | `edit` | `edit(operation="apply_overlay")` | `target: { file: "..." }`，原子写入物理磁盘。 |
| **全局协同重命名符号（联动修改定义与所有调用点）** | `refactor` | `refactor(operation="rename")` | `target: { symbol: "id" }`, `new_name: "新名称"`。 |
| **一键收敛单文件所有 LSP 诊断报错与整理导入** | `refactor` | `refactor(operation="fix_all")` | `target: { file: "..." }`, 自动修复语法与整理 imports。 |
| **跨文件/跨模块搬移符号并自动修正 import** | `refactor` | `refactor(operation="move")` | `target: { symbol: "id" }`, `destination_file: "目标文件"`。 |
| **安全删除符号（带调用安全门禁）** | `refactor` | `refactor(operation="delete")` | `target: { symbol: "id" }`, 活引用一票否决拒绝删除。 |
| **单行/单返回值函数内联展开消除** | `refactor` | `refactor(operation="inline")` | `target: { symbol: "id" }`, 自动替换所有调用处并清理原定义。 |

### 3. 修改与重构中枢标准调用模板

```json
// 模板 1: 单轮全量跨文件/多处批量原子修改 (edit.batch - 最高法定写入)
edit({
  "operation": "batch",
  "changes": [
    {
      "path": "repo-main/frontend/src/views/UserView.vue",
      "replacements": [
        {
          "target": {
            "text": "    <v-chip color=\"error\" variant=\"tonal\">\n      离线\n    </v-chip>"
          },
          "replacement": "    <v-chip color=\"success\" variant=\"flat\">\n      在线\n    </v-chip>"
        }
      ]
    }
  ],
  "output": { "format": "json" }
})
```

```json
// 模板 2: AST 语法树级单函数免锚点直接替换 (edit.symbol)
edit({
  "operation": "symbol",
  "target": { "symbol": "repo-main/src/api/handler.rs::process_user_client" },
  "replacement": "pub fn process_user_client(id: u64) -> Result<User, Error> {\n    // 新函数体实现\n}",
  "output": { "format": "json" }
})
```

```json
// 模板 3: 全局符号安全协同重命名 (refactor.rename)
refactor({
  "operation": "rename",
  "target": { "symbol": "repo-main/src/api/handler.rs::process_client" },
  "new_name": "process_user_client",
  "output": { "format": "json" }
})
```

---

## 四、 变更核验与质量中枢规范契约 (Change Hub)

### 1. 核心核验准则与防御门禁
1. **写后核验与自检闭环铁律**：
   代码修改落盘后，严禁凭空假设代码完全正确，必须执行写后质量闭环：
   * 第一动：调用 `change.detect` 检视变动文件范围；
   * 第二动：调用 `change.diagnostics` 读取 LSP 语言服务器语法与类型诊断报错；
   * 第三动：调用 `change.tests` 提取受本次代码改动波及的靶向单元测试集，指导运行回归验证。
2. **法定补丁预览**：
   查看未提交代码的 Git 行级差异补丁，唯一法定操作为 `change(operation="preview")`（严禁使用不存在的 `diff` 命令）。

### 2. 变更核验分流状态机矩阵 (Trigger-Action Matrix for Change)

| 触发场景 (Trigger) | 唯一法定操作 (Action) | 核心必填参数与门禁规则 |
| :--- | :--- | :--- |
| **提交或交付前检视所有未暂存脏文件与受影响符号** | `change(operation="detect")` | 检查工作区所有脏文件列表与受影响的符号集合。 |
| **精确核验修改后生成的 Git 行级差异补丁 (Diff Preview)** | `change(operation="preview")` | 查看改动补丁，输出统一 diff 格式（替代生造的 diff 命令）。 |
| **写完代码后，立即提取语言服务器 (LSP) 语法与编译报错** | `change(operation="diagnostics")` | `source: { file: "repo-main/..." }`，输出精准编译/类型错误。 |
| **核验刚刚发起的写操作是否已真正落盘并被图谱重新索引** | `change(operation="receipt")` | 核验写事务落地状态（mutation status）。 |
| **根据变动符号智能提取必须执行回归的单测名单** | `change(operation="tests")` | 自动反查下游测试用例，输出高精度回归测试集。 |
| **核验变动是否触犯项目架构守卫规则 (Guards)** | `change(operation="guards")` | 验证模块边界、私有可见性与依赖禁忌规则。 |
| **在不同 Git 分支间比对图谱结构破坏度与差异** | `change(operation="compare_branches")` | `options: { base: "main", head: "feature" }`。 |

### 3. 变更核验标准调用模板

```json
// 模板 1: 修改后变动范围审计与 LSP 语法报错自检 (change.detect & change.diagnostics)
change({
  "operation": "detect",
  "output": { "format": "json" }
})
```
```json
change({
  "operation": "diagnostics",
  "source": { "file": "repo-main/frontend/src/views/UserView.vue" },
  "output": { "format": "json" }
})
```

```json
// 模板 2: 补丁预览与靶向单测提取 (change.preview & change.tests)
change({
  "operation": "preview",
  "output": { "format": "json" }
})
```
```json
change({
  "operation": "tests",
  "output": { "format": "json" }
})
```

---

# 【L2 级：深度拓扑与架构审计】（进阶 · 析-联-迹三大中枢）

本层级规范面向**复杂系统诊断、架构重构前评估、死锁/死代码/并发竞态排查、接口实现全局反查与全链路跨过程追踪**。在需要宏观理解系统关系与深度依赖时，激活本层级各工具门面。

---

## 五、 架构分析中枢规范契约 (Analyze Hub)

### 1. 核心分析准则与防御门禁
1. **架构健康审计优先原则**：
   在实施大规模重构或公共类变更前，必须运行 `analyze.cycles` 检查循环依赖，运行 `analyze.dead_code` 避免向已废弃代码注入无效修补。
2. **并发与竞态写入审计**：
   在排查多线程死锁、数据不一致或状态机竞争时，必须调用 `analyze.race_writes` 定位多个函数对同一共享字段的并发无锁写入。

### 2. 架构分析分流状态机矩阵 (Trigger-Action Matrix for Analyze)

| 触发场景 (Trigger) | 唯一法定操作 (Action) | 核心必填参数与门禁规则 |
| :--- | :--- | :--- |
| **模块间是否存在恶性循环依赖诊断** | `analyze(operation="cycles")` | 检查 AST 图谱环路，输出成环路径与最小断环建议。 |
| **排查高频变动文件与高认知复杂度交集点** | `analyze(operation="hotspots")` | 输出 Git 提交热度 $\times$ 环路复杂度最高的风险核心类。 |
| **排查从未被调用的孤儿死代码与无效导出** | `analyze(operation="dead_code")` | `options: { include_tests: false }`，输出零扇入孤立符号。 |
| **评估两个模块或包之间的依赖耦合强度** | `analyze(operation="coupling")` | `target: { source_package: "pkgA", target_package: "pkgB" }`。 |
| **审计多线程/异步任务对共享字段的竞态写入** | `analyze(operation="race_writes")` | `target: { symbol: "repo-main/src/pool.rs::SharedState" }`。 |
| **排查违反分层单向依赖架构规约的代码点** | `analyze(operation="layers")` | 验证领域层是否非法反向依赖基础设施层/视图层。 |
| **统计代码符号的历史代码扰动与变动速率** | `analyze(operation="churn")` | `target: { file: "repo-main/..." }`，输出提交扰动频率。 |

### 3. 架构分析标准调用模板

```json
// 模板 1: 模块间循环依赖诊断 (analyze.cycles)
analyze({
  "operation": "cycles",
  "options": { "repo": "repo-main", "max_depth": 8, "limit": 20 },
  "output": { "format": "json" }
})
```

```json
// 模板 2: 共享状态并发竞态写入审计 (analyze.race_writes)
analyze({
  "operation": "race_writes",
  "target": { "symbol": "repo-main/src/net/connection_pool.rs::ConnectionPool" },
  "options": { "repo": "repo-main" },
  "output": { "format": "json" }
})
```

---

## 六、 关系网络中枢规范契约 (Relations Hub)

### 1. 核心关系网络准则与防御门禁
1. **修改前波及面反查铁律**：
   在改动任何公共函数签名或返回值前，**必须调用 `relations.callers` 或 `relations.usages`** 向上反查所有直接与间接调用方，绝对杜绝破坏未感知调用点的破窗效应。
2. **规范命名与符号定位**：
   - 向上查谁调用我：`relations(operation="callers")`；
   - 向下排查依赖：**唯一法定名称为 `relations(operation="dependencies")`**（严禁使用生造的 `callees`）；
   - 排查全局引用：**使用 `relations(operation="usages")` 或 `references`**（严禁使用生造的 `instantiations`）；
   - 面向接口编程：**使用 `relations(operation="implementations")` 全局反查接口与 Trait 的具体实现类**。

### 2. 关系网络分流状态机矩阵 (Trigger-Action Matrix for Relations)

| 触发场景 (Trigger) | 唯一法定操作 (Action) | 核心必填参数与门禁规则 |
| :--- | :--- | :--- |
| **向上反查某函数的全部直接调用方 (Callers)** | `relations(operation="callers")` | `target: { symbol: "id" }`, 查清“谁调用了我”。 |
| **向下排查某函数内部触发的全部被调用依赖 (Dependencies)** | `relations(operation="dependencies")` | `target: { symbol: "id" }`, 查清“我依赖了谁”（替代生造的 callees）。 |
| **面向接口反查：列出实现某 Trait/Interface 的全部具体类** | `relations(operation="implementations")` | `target: { symbol: "id" }`, 全局搜索具体实现类名单。 |
| **反查虚方法、特征方法被哪些子类重写覆盖 (Overrides)** | `relations(operation="overrides")` | `target: { symbol: "id" }`, 排查多态方法覆盖关系。 |
| **排查类/结构体/常量的全局所有读写引用与使用点** | `relations(operation="usages")` | `target: { symbol: "id" }`, 识别实例化、字段访问与传参点。 |
| **精准查验符号引用的合法性与有效性 (References)** | `relations(operation="references")` | `target: { symbol: "id" }`, 查验引用有效性。 |
| **查验类、特征 (Trait)、接口的完整继承与实现树** | `relations(operation="hierarchy")` | `target: { symbol: "id" }`, `options: { direction: "both" }`。 |
| **排查受该模块或文件改动波及的所有下游依赖者** | `relations(operation="dependents")` | `target: { file: "repo-main/..." }` 或 `target: { symbol: "id" }`。 |

### 3. 关系网络标准调用模板

```json
// 模板 1: 向上反查某函数的全部调用方 (relations.callers)
relations({
  "operation": "callers",
  "target": { "symbol": "repo-main/src/api/handler.rs::process_user_client" },
  "options": { "limit": 40, "max_bytes": 8400 },
  "output": { "format": "json" }
})
```

```json
// 模板 2: 面向接口全局反查具体实现类 (relations.implementations)
relations({
  "operation": "implementations",
  "target": { "symbol": "repo-main/src/core/engine.rs::DataProcessor" },
  "output": { "format": "json" }
})
```

```json
// 模板 3: 向下排查某函数的全部被调用依赖 (relations.dependencies)
relations({
  "operation": "dependencies",
  "target": { "symbol": "repo-main/src/api/handler.rs::process_user_client" },
  "options": { "limit": 40, "max_bytes": 8400 },
  "output": { "format": "json" }
})
```

---

## 七、 链路追踪中枢规范契约 (Trace Hub)

### 1. 核心链路追踪准则与防御门禁
1. **双端点容器互斥契约**：
   `trace.path` / `trace.flow` / `trace.taint` 为双端点操作，必须显式声明源头 `target: { symbol: "A" }` 与宿点 `to: { symbol: "B" }`。而单点操作（如 `cfg`、`call_chain`）严禁传入 `to` 容器。
2. **行号精确保留与禁止单行压缩**：
   Trace 类操作严禁传入 `compact: true`，必须输出完整的节点拓扑与边所对应的源代码行号（`edge.line`），确保追踪出的调用链能一眼定位到每一跳的精确物理行。

### 2. 链路追踪分流状态机矩阵 (Trigger-Action Matrix for Trace)

| 触发场景 (Trigger) | 唯一法定操作 (Action) | 核心必填参数与门禁规则 |
| :--- | :--- | :--- |
| **计算两点间的最短调用路径与跨模块跳数** | `trace(operation="path")` | `target: { symbol: "A" }`, `to: { symbol: "B" }`, `options: { max_depth: 16 }`。 |
| **追踪数据流、参数或变量在跨过程函数间的传递路径** | `trace(operation="flow")` | `target: { symbol: "A" }`, `to: { symbol: "B" }`, 追踪参数传递。 |
| **审计单个复杂函数内部的控制流图分支与基本块** | `trace(operation="cfg")` | `target: { symbol: "id" }`, `options: { mermaid: true }`，生成 CFG 流程图。 |
| **自某函数向下完整发散，输出整棵正向调用子树** | `trace(operation="call_chain")` | `target: { symbol: "id" }`, `options: { depth: 3, limit: 50 }`。 |
| **跨系统排查未校验输入源是否流向危险执行点 (Sink)** | `trace(operation="taint")` | `target: { query: "输入源" }`, `to: { query: "危险点" }`。 |
| **沿特定边类型（如 calls/references）有预算自由图探索** | `trace(operation="walk")` | `target: { symbol: "id" }`, `options: { edge_kinds: "calls", token_budget: 1500 }`。 |
| **执行管道只读 DSL 高级拓扑筛选** | `trace(operation="graph")` | `target: { query: "nodes kind=interface | traverse implements in" }`。 |

### 3. 链路追踪标准调用模板

```json
// 模板 1: 两点间跨模块最短调用链路追踪 (trace.path)
trace({
  "operation": "path",
  "target": { "symbol": "repo-main/src/api/handler.rs::handle_request" },
  "to": { "symbol": "repo-main/src/db/pool.rs::acquire_connection" },
  "options": { "max_depth": 16, "k": 1, "max_bytes": 8400 },
  "output": { "format": "json" }
})
```

```json
// 模板 2: 复杂函数内部控制流分支生成 (trace.cfg)
trace({
  "operation": "cfg",
  "target": { "symbol": "repo-main/src/net/state_machine.rs::transition_state" },
  "options": { "mermaid": true, "max_bytes": 8400 },
  "output": { "format": "json" }
})
```

---

# 【L3 级：协同推演与空间治理】（专项 · 虚拟沙箱/审查/PR/底层运维）

本层级规范面向**内存虚拟推演、多方案沙箱分支比对、团队持久记忆、代码审查批注发布、Pull Request 冲突雷达以及工作区底层离线运维**。日常编码无需优先激发本层级，但当涉及虚拟推演、代码评审或空间维护时，本层级提供严格的契约保障。

---

## 八、 虚拟推演与认知记忆中枢规范契约 (Overlay & Recall & Remember)

### 1. 核心推演与记忆准则
1. **沙箱零污染原则 (`overlay`)**：
   对于高危重构或多方案设计，模型可先在内存沙箱中分叉推演（`overlay.fork`）并注入虚拟文件（`overlay.push`），在完全不修改物理磁盘文件的前提下验证图谱变化。推演成功后再调用 `edit.apply_overlay` 统一落盘。
2. **主动召回与压缩提炼自愈铁律 (`recall`)**：
   * **开局主动召回**：处理核心业务或架构修改前，必须调用 `recall(operation="surface")` 调阅绑定的架构规约（固定参数 `mark_accessed: false` 避免污染热度计数）；
   * **上下文压缩自愈**：当对话遭遇 Context Compaction 截断恢复后，首动调用 `recall(operation="distill")`，以极低 Token 恢复核心会话决策与上下文。
3. **高危变更免责合同 (`remember.risk_ack`)**：
   当修改核心关键符号触发风险门禁时，必须调用 `remember(operation="risk_ack")` 并携带 `ack: true` 作为唯一法定确认凭据。

### 2. 虚拟推演与记忆状态机矩阵 (Trigger-Action Matrix)

| 触发场景 (Trigger) | 涉及门面 | 唯一法定操作 (Action) | 核心必填参数与门禁规则 |
| :--- | :--- | :--- | :--- |
| **高危修改前在内存注入虚拟文件推演** | `overlay` | `overlay(operation="push")` | `arguments: { path: "...", content: "...", base_sha: "..." }`。 |
| **创建独立的具名推演分支以比对方案** | `overlay` | `overlay(operation="fork")` | `arguments: { name: "strategy-b", activate: true }`。 |
| **在不同推演分支间切换视角** | `overlay` | `overlay(operation="switch")` | `arguments: { name: "strategy-b" }`。 |
| **将推演胜出方案的改动批量落物理盘** | `edit` | `edit(operation="apply_overlay")` | `target: { file: "..." }`，原子写入物理磁盘。 |
| **新任务开局调阅绑定的架构不变式** | `recall` | `recall(operation="surface")` | `arguments: { task: "...", symbol_ids: "..." }`, 固定 `mark_accessed: false`。 |
| **遭遇长对话压缩后一键提炼会话记忆** | `recall` | `recall(operation="distill")` | `arguments: { max_symbols: 10, max_files: 10 }`。 |
| **沉淀随 Git 提交的团队架构决议** | `remember` | `remember(operation="notebook")` | `arguments: { title: "...", body: "...", tags: "..." }`，写入 `.gortex/notebook`。 |

### 3. 虚拟推演与记忆标准调用模板

```json
// 模板 1: 内存推演分支创建与虚拟文件推送 (overlay.fork & push)
overlay({
  "operation": "fork",
  "arguments": { "name": "refactor-async-pool", "activate": true }
})
```
```json
overlay({
  "operation": "push",
  "arguments": {
    "path": "repo-main/src/net/connection_pool.rs",
    "content": "pub struct ConnectionPool { max_conns: usize }",
    "base_sha": "a1b2c3d4e5f678901234567890abcdef12345678"
  }
})
```

```json
// 模板 2: 任务开局架构规约主动召回 (recall.surface)
recall({
  "operation": "surface",
  "arguments": {
    "task": "重构底层网络连接池",
    "symbol_ids": "repo-main/src/net/connection_pool.rs::ConnectionPool",
    "limit": 10
  }
})
```

---

## 九、 协同审查与 PR 合并雷达中枢规范契约 (Review & PR Hub)

### 1. 核心协同审查准则
1. **多 PR 拓扑冲突预警**：
   在多分支并行开发环境下，必须调用 `pr.conflicts` 预判多个开放 PR 触及的 AST 社区重叠度，计算安全的合流序列（Merge Train），杜绝无序合并破坏主线。
2. **行级审查凭据完整性**：
   在向远端代码平台发布行级审查意见前，所有草稿批注必须包含精确的行号、建议代码块与风险级别。

### 2. 协同审查分流状态机矩阵 (Trigger-Action Matrix)

| 触发场景 (Trigger) | 涉及门面 | 唯一法定操作 (Action) | 核心必填参数与门禁规则 |
| :--- | :--- | :--- | :--- |
| **拉取所有开放 PR 并汇总 CI 与阻断状态** | `pr` | `pr(operation="list")` | `options: { state: "open", limit: 30 }`。 |
| **基于 AST 计算单 PR 爆炸半径与风险分** | `pr` | `pr(operation="impact")` | `target: { pr_number: 101 }`，输出 5 维加权风险打分。 |
| **扫描多 PR 共同触及的模块与冲突排期** | `pr` | `pr(operation="conflicts")` | `arguments: { pr_numbers: [101, 102, 105] }`，计算 Merge Train。 |
| **在本地创建针对特定文件的代码审查草稿** | `review` | `review(operation="draft")` | 记录行级建议、发现的潜在缺陷与最佳实践指导。 |
| **将最终审查结果一键推送至远端 Git 平台** | `publish_review` | `publish_review(operation="publish")` | 将草稿发布至 GitHub/GitLab PR 评论区。 |

### 3. 协同审查标准调用模板

```json
// 模板: PR 爆炸半径与合并冲突雷达评估 (pr.impact & pr.conflicts)
pr({
  "operation": "impact",
  "target": { "pr_number": 108 },
  "output": { "format": "json" }
})
```

---

## 十、 工作区底层空间运维中枢规范契约 (Workspace_Admin Admin-Only Hub)

### 1. 核心空间运维准则
1. **索引异常恢复红线 (`workspace_admin`)**：
   仅当底层图谱因断电或强制杀进程导致 SQLite 数据库损坏、或者新增大量文件未被探测时，方可调用 `workspace_admin.rebuild` 触发全量底层重新索引。常规开发严禁滥用重建。
2. **落盘分析隔离门禁**：
   历史涉及持久化落盘分析的四个特殊操作——`blame`、`coverage`、`sql_rebuild`、`temporal_verify`，必须且仅能通过 `workspace_admin` 门面执行。

### 2. 空间运维分流状态机矩阵 (Trigger-Action Matrix)

| 触发场景 (Trigger) | 涉及门面 | 唯一法定操作 (Action) | 核心必填参数与门禁规则 |
| :--- | :--- | :--- | :--- |
| **底层数据库损坏或索引漂移时全量离线重建** | `workspace_admin` | `workspace_admin(operation="rebuild")` | 重建 AST 节点表、BM25 词元索引与拓扑关系。 |
| **清理孤立缓存、压缩 SQLite 碎片与垃圾回收** | `workspace_admin` | `workspace_admin(operation="gc")` | 回收磁盘空间，优化查询性能。 |
| **执行 Git Blame 代码提交责任人数据挖掘落盘** | `workspace_admin` | `workspace_admin(operation="blame")` | 强制容器: `arguments: {}`。 |
| **导入单测覆盖率报告文件更新图谱节点权重** | `workspace_admin` | `workspace_admin(operation="coverage")` | 强制容器: `arguments: { profile: "coverage.out" }`。 |

---

## 十一、 MCP 协议级原生映射与标准模板 (Resources & Prompts)

Gortex 原生暴露了高价值的 MCP Resource 与 Prompt 契约，支持零 Token 损耗调阅系统状态：

### 1. 原生 Resource 资源调阅
* **`READ_RESOURCE gortex://session`**：
  在上下文压缩或长会话唤醒后，直接调阅当前会话的工作记忆、改动缓存与活跃符号。
* **`READ_RESOURCE gortex://report`**：
  陌生项目开局时直接调阅项目健康度报告、核心上帝类（God Nodes）与关键业务架构分布。

### 2. 原生 Prompt 提示词模版
* **`GET_PROMPT pre_commit with arguments: {"scope": "unstaged"}`**：
  在最终提交代码前，自动化触发未暂存变更的爆炸半径审查。
* **`GET_PROMPT safe_to_change with arguments: {"ids": "repo-main/src/pool.rs::ConnectionPool"}`**：
  全自动输出修改某符号的下游波及面、架构守卫规则约束与单测靶向名单。
* **`GET_PROMPT orientation`**：
  快速输出陌生仓库的核心骨干业务流与骨干符号体系。

---

# 【执行遵从性与防踏空检查自查清单】 (Compliance Checklist)

模型在任何单轮工具调用前，必须在内心执行以下 6 项快速自查：
1. [ ] **【开局多项目自查】**：新任务或跨库分析时，是否首动执行了 `workspace.repos` 与 `workspace.active_project` 探测？
2. [ ] **【路径零拼接自查】**：在 `options.path` 中是否绝对去除了仓库名前缀？（严禁出现 `repo-main/src/...`；仓库名前缀仅能在 `target.file`、`target.symbol` 或 `changes[].path` 中合法存在）。
3. [ ] **【前端组件自查】**：涉及 Vue/React 单文件组件模板事件与内部闭包，是否正确使用了 `search(operation="text")` 而非 `symbols`？
4. [ ] **【精读切片自查】**：调用 `read.file` 时，是否强制附加了 `offset` 和 `limit`（$\le 150$ 行）？严禁全文件裸读！
5. [ ] **【原子交付自查】**：修改代码时，是否已将所有修改点整合成单轮 `edit(operation="batch")` 一次性提交？或局部单函数直接使用 `edit(operation="symbol")`？
6. [ ] **【防溢出截断自查】**：若返回 `output.txt`，是否严格使用 `response.grep` / `response.slice` 提炼，坚决不使用原生工具违规越界？
