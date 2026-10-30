# Gortex 统一架构全景全生命周期规范契约 (Unified Gortex MCP Specification)

【命令排版语法说明】
命令规范与示例中的 "|" 纯属文档排版的多选一候选间隔说明，绝非命令参数值！实际构造工具调用时严禁原样照抄 "|"，必须且仅能选择传入单一合法字段值。带 "|" 的合并项，各操作专属必填字段不同，调用时必须严格按具体 operation 契约填充。
- 违规反例（严禁）：\`analyze(kind="hotspots|cycles")\`、\`change(operation="tests|guards")\`
- 合规正例（法定）：\`analyze(kind="hotspots")\`、\`change(operation="tests")\`

【最高统领原则与顶层执行边界】
Gortex 是对其 track 仓库全生命周期代码探测、定位、精读、分析、编辑、重构、验证、审查与记忆的唯一法定权威工具。
1. 原生 MCP 绝对闭环：已 track 仓库的一切操作必须且仅能通过 Gortex 原生 MCP 闭环执行，严禁读写割裂与擅自调用宿主工具（view_file、grep_search、find_by_name、write_to_file、replace_file_content 等），未 track 项目不得擅自图谱化。
2. 【多仓库全图谱终极铁律】（Multi-Repo Global Graph Mandate）：已 track 仓库统一托管于全局知识图谱。严禁未经穿透检索断定代码未索引并擅自退回原生工具。全流程必须由 Gortex 原生 MCP 闭环，严禁跨客户端规则漂移，必须以当前环境暴露的工具集为准。仅当 \`workspace(operation="project")\` 穿透查询与物理路径均证实未 track，且用户明确要求时，方可报告未 track 并停步。
3. 【平台工具唯一例外豁免契约】（Host Tools Strict Exemption）：在代码全生命周期的“查-读-析-写-验”全流程中，强制使用 Gortex 原生 MCP 闭环，严禁擅自调用宿主原生读写工具。唯一法定豁免：仅当用户发出包含“使用原生工具”的显式明确自然语言指令（例如用户明确要求使用宿主命令进行本地 Git 备份、分支操作或外部构建）时，方可单次跳出 Gortex MCP 执行该特定操作。单次原生操作执行完毕后，后续代码分析与变更必须立即重返 Gortex MCP 闭环，严禁滞留于宿主工具中。
4. 【全局技能只读豁免】（Global Skills Read Exemption）：read 门面放行对全局技能资产的只读访问（~/.agents/skills/*/SKILL.md、~/.config/opencode/skills/、~/.copilot/skills/、~/.claude/skills/ 及 Pi 包生态），用于对齐宿主环境已挂载的全局工作流，严禁对技能目录执行任何形式的写操作。
5. 【顶层先探测后分析代码铁律】（Probe-Before-Analyze Mandate）：进入新任务或跨模块分析前，必须首先调用 workspace 门面执行环境边界、主场工程与索引健康度前置探测，严禁盲目直接读搜；未明确仓库归属前严禁注入 repo 选择器（防范 no_repository_selector 拦截）。
6. 【全生命周期高吞吐与单轮全量交付铁律】（Full-Lifecycle Single-Turn High-Throughput Mandate）：严禁“边走边改、逐处轮询、试探推演”的切香肠式低效交互。接收任务后必须全局通盘规划，严禁以“修改仅 1 处”为借口退回单入单出轮询。必须严格执行“查-读-析-写-验”五步单轮交付流水线（“查-读-析-写-验”具体门面与参数在后续章节中有详细规范）。单任务全流程原则上仅允许 1 次批量写交互，彻底消除偷懒式碎片轮询。若“验”阶段发现单测或架构守卫失败，必须通盘汇总全部报错点，发起第 2 次单轮批量修复，严禁退化为零散试探修改。
7. 截断自愈后置切片契约：单次响应超限截断时（_truncated_by_limit=true），严禁调大 limit 盲目重查，必须调用 response.slice / response.grep 原地切片，杜绝触发 20KB 磁盘转储 (output.txt) 诱发脱轨。
8. 【严禁调用 explore 全景探索门面】（Absolute Ban on Explore Facade）：严禁发起 explore 门面的任何操作调用。该门面默认携带 12,000 Tokens 巨量泛洪，且在受限预算下会强行触发底层 Shedding 算法剥离实现代码（将函数体掏空为签名存根），必然导致 20KB 磁盘转储截断或分析残缺，诱发退回宿主工具。

---

## 零、 Gortex 顶层工作区探测、治理与多项目协同中枢规范契约 (Workspace & Admin Hub Specification)

本中枢规范定义基于全局联合知识图谱（Unified Knowledge Graph）的多项目工作区拓扑探测、主工程路由、跨库穿透与落盘治理契约（`workspace` 与 `workspace_admin` 领域门面）。为模型提供在代码分析前必须执行的“环境边界与索引就绪探测状态机”、只读免切穿透查询与深度重构切域法则、以及写治理操作硬性隔离门禁，彻底消除因盲目读搜导致的参数越界（`no_repository_selector` 报错）与未索引误判脱轨。

### 0. 顶层工作区探测与多项目协同全景导图

#### 导图 1：代码分析前“四步前置探测状态机” (Probe-Before-Analyze Lifecycle)
```text
【新任务接入 / 跨库任务开启】
         │
         ▼
【Step 1: 仓库边界探测】调用 workspace(operation="repos")
         │ 获取当前联合图谱已 track 的合法仓库列表 (Repo Names)
         ├─目标仓库未在列表中 ──► 严禁私自 track！停步向用户报告未 track
         └─目标仓库已在列表中 ──► 确认为合法纳管资产，准许进入图谱分析
              │
              ▼
【Step 2: 主场工程与绑定探测】调用 workspace(operation="active_project")
         │ 明确当前会话聚焦的 Primary Project 与 bound 隔离状态
         ├─目标属于当前主场 ──► 直接使用相对路径与本地门面
         └─目标属于外部工程 ──► 启动双穿透机制 (workspace.project 或带前缀路径)
              │
              ▼
【Step 3: 图谱健康度与就绪探测】调用 workspace(operation="index")
         │ 获取 parse_failures, stale_files 与 health_score
         ├─健康分偏低或有解析故障 ──► 标记图谱局部风险，谨慎依赖深度调用链
         └─健康度就绪 (OK) ──► 确认为高置信度 AST
              │
              ▼
【Step 4: 视图代际与路由诊断】(可选) 调用 workspace(operation="checkouts")
         │ 确认物理路径绑定哪一代图谱快照 (Generation)，消除多 Worktree 脏写
         ▼
【正式进入代码分析阶段】(search / read / relations / trace / analyze)
```

#### 导图 2：多项目联合图谱双穿透拓扑架构 (Dual-Penetration Multi-Repo Architecture)
```text
┌──────────────────────────────────────────────────────────────────────────────────┐
│                   Gortex 单守护进程全局联合知识图谱 (Unified Graph)                  │
└──────────────────────────────────────────────────────────────────────────────────┘
                                        │
           ┌────────────────────────────┴────────────────────────────┐
           ▼                                                         ▼
   【主场工程: Project-A】                                   【外部工程: Project-B】
 (当前会话 bound 主上下文)                                 (同属 Daemon 管理的已 track 仓库)
           │                                                         │
           ├─ 普通检索: search.winnow                                ├─ 穿透检索: workspace(operation="project")
           ├─ 普通精读: read.file (本地相对路径)                      ├─ 穿透精读: read.file (带仓库前缀物理路径)
           └─ 本地编辑: edit.batch                                   └─ 深度重构: 必须先切换主场工程！
                                                                               │
                                                                               ▼
                                           调用 workspace_admin(operation="set_active_project", arguments: {project: "Project-B"})
```

#### 导图 3：只读工作区感知 vs 写治理硬隔离架构 (Read vs Admin Write Boundary)
```text
┌──────────────────────────────────────────────────────────────────────────────────┐
│              冷门面统一契约: 顶层仅接收 operation, arguments, output, view        │
└──────────────────────────────────────────────────────────────────────────────────┘
                                        │
           ┌────────────────────────────┴────────────────────────────┐
           ▼                                                         ▼
【只读感知门面: workspace】 (facadeEffectRead)             【写治理门面: workspace_admin】 (facadeEffectControlWrite)
 核心职责: 只读探测、元数据透视、视图诊断                   核心职责: 仓库生命周期、主工程切换、落盘分析
 ──────────────────────────────────────                    ──────────────────────────────────────────
 1. repos: 查询已 track 仓库列表                            1. track: 注册新仓库至全局图谱 (限具体目录)
 2. active_project: 查询当前主场工程                        2. untrack: 从图谱注销仓库
 3. info: 工作区根路径与绑定元数据                          3. set_active_project: 热切换主场工程
 4. index: 图谱健康度与解析故障诊断                         4. index / reindex: 触发底层增量/全量重建
 5. graph: 全局节点、边与内存指标                           5. save_scope / delete_scope: 分析范围管理
 6. project: 免切项目符号单次穿透                           6. blame: Git 提交责任人落盘挖掘 (原 analyze 禁令)
 7. checkouts: 多 Worktree 代际诊断                         7. coverage: 单测覆盖率文件导入 (原 analyze 禁令)
 8. scopes: 查询已保存的分析范围                            8. sql_rebuild / temporal_verify: 数据库重构
```

---

### 1. 最高架构准则与硬性门禁 (Core Workspace & Admin Rules)

#### 准则一：【先探测再分析代码顶层铁律】（Probe-Before-Analyze Mandate）
- 在接入新工作区、开启复杂任务或排查跨库问题时，**必须首先执行工作区探测**；
- 必须通过 `workspace(operation="repos")` 确认仓库是否已被 track，通过 `workspace(operation="active_project")` 明确当前绑定的主场工程，通过 `workspace(operation="index")` 确认图谱健康分与解析状态；
- **防脱轨判定**：严禁未经探测直接发起跨库检索，严禁在搜索无果时凭空臆测“仓库未索引”并擅自退回宿主工具。

#### 准则二：【冷门面 arguments 容器绝对封装契约】（Cold Facade Container Boundary）
- `workspace` 与 `workspace_admin` 属于底层 `facadeColdDomain` 判定的典型冷门面；
- **容器铁律**：所有业务参数（`project`, `query`, `path`, `name`, `profile`, `confirm`, `family` 等）**必须且仅能封装在 `arguments: { ... }` 容器对象内**！
- 严禁顶层平铺，顶层仅接收 `operation`, `arguments`, `output`, `view`。

#### 准则三：【单项目自动箝位与参数注入禁令】（No-Repository-Selector Guard）
- 单仓库或主场工程执行期间，底层路由自动箝位至当前绑定的主项目；
- **硬性禁令**：严禁向未声明 `repo` 属性的只读工具（如普通 `read.file`、`search.text`）随意塞入 `repo`、`project`、`cwd`、`workspace` 字段，否则 0.64.11 底层将直接以 `no_repository_selector` 拒绝执行；
- 严禁向 `workspace(operation="repos")` 与 `workspace(operation="info")` 注入 `repo` 参数（底层 `ResolveToolScope` 显式实施“no repo”严格校验）。

#### 准则四：【跨项目双穿透合一与主工程切域法则】
- **只读轻量穿透（免切项目）**：
  - 符号检索：必须调用 `workspace(operation="project", arguments: {project: "Project-B", query: "AuthHandler"})`；
  - 源码精读：路径显式补齐仓库前缀，如 `read(operation="file", target: {file: "Project-B/src/lib.rs"})`；
- **深度批量修改与重构（必须切域）**：
  - 若需要对目标外部工程执行多文件修改（`edit.batch`）或回归验证（`change.tests`），必须调用 `workspace_admin(operation="set_active_project", arguments: {project: "Project-B"})` 切换主上下文；
  - **逻辑标识铁律**：`set_active_project` 仅接收逻辑 Project 标识名（在 `GlobalConfig.Projects` 注册的键名），**严禁传入绝对路径或文件路径**。

#### 准则五：【写治理与落盘分析硬性隔离门禁】（Admin Write Isolation Guard）
- 历史涉及持久化落盘与数据库重构的四个分析操作——`blame`、`coverage`、`sql_rebuild`、`temporal_verify`，**严禁在只读 `analyze` 门面中调用**；
- 必须且仅能通过 `workspace_admin` 门面执行，确保所有落盘操作受受控状态机管理。

#### 准则六：【安全沙箱根目录越权防护门禁】（Root Confinement Guard）
- Agent 会话严禁向 `workspace_admin(operation="track")` 传入系统根目录 `/`、Windows 盘符根目录（如 `C:\`）或 `$HOME` 主目录；
- 底层硬性阻断 `force: true` 针对 Agent 会话的提权行为，坚守工作区目录物理隔离底线。

---

### 2. 工作区探测与治理分流状态机 (Trigger-Action Matrix)

| 触发场景 (Trigger) | 涉及领域 | 唯一法定操作 (Action) | 核心必填参数与门禁规则 |
| :--- | :--- | :--- | :--- |
| **开局探测当前工作区纳管的全部已 track 仓库列表** | 边界探测 | `workspace(operation="repos")` | 强制容器: `arguments: {}` (严禁传 repo 属性) |
| **开局探测当前会话绑定的主场工程与成员仓库** | 归属探测 | `workspace(operation="active_project")` | 强制容器: `arguments: {}` |
| **开局探测图谱健康分、解析失败数与过期文件** | 索引健康 | `workspace(operation="index")` | 强制容器: `arguments: {compact: true}` |
| **获取全局知识图谱节点、边数与内存占用指标** | 图谱统计 | `workspace(operation="graph")` | 强制容器: `arguments: {}` |
| **获取工作区根目录、绑定模式与排除标记全景** | 工作区元数据 | `workspace(operation="info")` | 强制容器: `arguments: {}` (严禁传 repo 属性) |
| **免切主工程，跨项目单次穿透检索外部仓库符号** | 跨库穿透检索 | `workspace(operation="project")` | 强制容器: `arguments: {project: "Project-B", query: "TokenValidator"}` |
| **诊断具体物理文件绑定的工作树路由与代际** | 视图代际诊断 | `workspace(operation="checkouts")` | 强制容器: `arguments: {family: "Project-A"}` |
| **将当前会话的主上下文热切换至指定外部工程** | 主工程切域 | `workspace_admin(operation="set_active_project")` | 强制容器: `arguments: {project: "Project-B"}` (严禁传文件路径) |
| **注册新代码仓库至全局守护进程联合图谱** | 仓库注册 | `workspace_admin(operation="track")` | 强制容器: `arguments: {path: "D:/repos/Project-B", name: "Project-B"}` |
| **从全局图谱注销指定代码仓库** | 仓库注销 | `workspace_admin(operation="untrack")` | 强制容器: `arguments: {path: "D:/repos/Project-B"}` |
| **触发指定代码仓库的底层全量/增量图谱重建** | 索引重建 | `workspace_admin(operation="reindex")` | 强制容器: `arguments: {path: "Project-A"}` |
| **执行 Git Blame 代码提交责任人数据挖掘落盘** | 落盘分析治理 | `workspace_admin(operation="blame")` | 强制容器: `arguments: {}` (原 analyze 硬拦截操作) |
| **导入单测覆盖率报告文件更新图谱节点权重** | 落盘分析治理 | `workspace_admin(operation="coverage")` | 强制容器: `arguments: {profile: "coverage.out"}` (原 analyze 硬拦截操作) |
| **重构图谱底层 SQL 关系持久化数据库** | 数据库运维 | `workspace_admin(operation="sql_rebuild")` | 强制容器: `arguments: {}` (原 analyze 硬拦截操作) |

---

### 3. 工作区探测与治理标准模板 (完全脱敏规范)

#### 模板 1：开局四步探测之仓库边界与主场工程核验 (`workspace.repos` & `workspace.active_project`)
```json
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

#### 模板 2：开局四步探测之索引健康度诊断 (`workspace.index`)
```json
workspace({
  "operation": "index",
  "arguments": {
    "compact": true
  }
})
```

#### 模板 3：多项目免切工程单次跨库穿透检索 (`workspace.project`)
```json
workspace({
  "operation": "project",
  "arguments": {
    "project": "Project-B",
    "query": "Authenticator::verify_token",
    "limit": 20
  }
})
```

#### 模板 4：多文件批量重构前切换主场上下文 (`workspace_admin.set_active_project`)
```json
workspace_admin({
  "operation": "set_active_project",
  "arguments": {
    "project": "Project-B"
  }
})
```

#### 模板 5：纳管新工程至全局联合知识图谱 (`workspace_admin.track`)
```json
workspace_admin({
  "operation": "track",
  "arguments": {
    "path": "D:/repos/Project-B",
    "name": "Project-B"
  }
})
```

#### 模板 6：落盘分析治理之 Git Blame 责任人数据更新 (`workspace_admin.blame`)
```json
workspace_admin({
  "operation": "blame",
  "arguments": {}
})
```

#### 模板 7：落盘分析治理之代码覆盖率报告导入 (`workspace_admin.coverage`)
```json
workspace_admin({
  "operation": "coverage",
  "arguments": {
    "profile": "Project-A/target/coverage.lcov"
  }
})
```


### 五、 工作树生命周期深度治理与底层诊断规范契约 (Checkouts Lifecycle Management)

本小节规范多分支（Branches）与独立工作树（Linked Worktrees）环境下的底层治理契约。Gortex 0.64.11 注册了 4 个底层的生命周期治理离散工具（来源于 \`tools_checkouts.go\`），为模型提供在多工作树分支路由异常排查、外部 Git 变动实时对账、主基准语料晋升以及孤儿图谱资产清理的确定性操作接口。在 Facade-v1 门面模式下，相关调用可直接使用离散工具名，或经由 \`workspace\` 体系协同执行。

#### 1. 深度治理 4 大工具契约与安全状态机

\`\`\`mermaid
flowchart TD
    subgraph ViewDiagnosis["分支视图诊断"]
        EV["explain_view(path='...')<br>只读诊断：输出 checkout 绑定、route 与 generation<br>定位视图合成断链"]
    end

    subgraph SyncClock["文件系统强行对账"]
        RC["reconcile_checkouts(family='...')<br>控制写：强制 Git/磁盘对账，推进时钟与协调器<br>消除后台 Janitor 轮询延迟"]
    end

    subgraph CorpusPromotion["主基线晋升 (两段式安全锁)"]
        SPC1["set_primary_checkout(graph='...', confirm=false)<br>只读预览：校验被取代者、epoch 与受影响的分支"]
        SPC2["set_primary_checkout(graph='...', confirm=true)<br>落盘执行：将目标语料晋升为主基准"]
        SPC1 -->|核准后| SPC2
    end

    subgraph OrphanRemoval["孤儿工作树销毁 (两段式安全锁)"]
        FC1["forget_checkout(path='...', confirm=false)<br>只读预览：计算闭包影响与关联专属图谱"]
        FC2["forget_checkout(path='...', confirm=true)<br>物理销毁：彻底注销并清除专用图谱资产"]
        FC1 -->|核准后| FC2
    end
\`\`\`

#### 2. 治理准则与硬性门禁

##### 准则一：【路由断链解释先于结构假定】（Diagnose-Before-Assume for Views）
当读取或分析多工作树中的物理文件发生符号丢失、语法解析空集或 \`no_repository_selector\` 异常时，严禁盲目假定文件未被索引。必须首先调用 \`explain_view(path="具体文件路径")\` 诊断该文件具体挂载于哪个 checkout 分支视图、其路由归属与 Generation 世代编号。若输出表明由于链路中断导致回退至基准语料库（Base Corpus），必须依据诊断断链节点排查 worktree 挂载状态。

##### 准则二：【外部 Git 变动后强制主动对账】（Immediate Reconcile after Git Mutations）
当宿主外部环境执行了 \`git checkout\` 切换分支、新建或销毁了工作树目录（\`git worktree add/remove\`）后，图谱可能存在毫秒级时序延迟。模型必须立即调用 \`reconcile_checkouts\` 强制图谱推进 Reconciler Clocks，将当前磁盘真实工作树与构建协调器（Build Coordinators）瞬间对其，严禁等待后台 Janitor 周期性扫描。

##### 准则三：【基准更迭与孤儿清除两段式安全门禁】（Two-Phase Safety Gate for Destructive Checkout Ops）
- \`set_primary_checkout\`（基线晋升）与 \`forget_checkout\`（工作树注销）属于高危写操作：
  - **第一位阶（只读预览）**：必须首先传入 \`confirm: false\`（或省略 confirm 参数），模型必须审查服务端返回的被取代者（Incumbent）、家族 Epoch 世代以及所有受波及的 automatic checkout 重新分层预告；
  - **第二位阶（落盘确认）**：仅在用户明确指令或只读预览核验无误后，方可显式传入 \`confirm: true\` 执行物理落盘。未经过预览直接传入 \`confirm: true\` 判定为违规脏写。

#### 3. 工作树深度治理标准模板 (完全脱敏规范)

##### 模板 8：文件分支视图绑定与路由断链诊断 (\`explain_view\`)
\`\`\`json
explain_view({
  "path": "Project-A/src/lib.rs",
  "format": "gcx",
  "max_bytes": 8192
})
\`\`\`

##### 模板 9：外部分支变动后强制文件系统主动对账 (\`reconcile_checkouts\`)
\`\`\`json
reconcile_checkouts({
  "family": "Project-A",
  "format": "gcx"
})
\`\`\`

##### 模板 10：语料主基准分支晋升预览与落盘 (\`set_primary_checkout\`)
\`\`\`json
// 第一阶段：只读预览
set_primary_checkout({
  "graph": "Project-A",
  "confirm": false
})
\`\`\`
\`\`\`json
// 第二阶段：确认后落盘执行
set_primary_checkout({
  "graph": "Project-A",
  "confirm": true
})
\`\`\`

##### 模板 11：废弃工作树及其专属图谱彻底注销清除 (\`forget_checkout\`)
\`\`\`json
// 第一阶段：只读预览影响闭包
forget_checkout({
  "path": "Project-A/worktrees/feature-old",
  "confirm": false
})
\`\`\`
\`\`\`json
// 第二阶段：确认后物理销毁
forget_checkout({
  "path": "Project-A/worktrees/feature-old",
  "confirm": true
})
\`\`\`

---

## 一、 最高检索准则与硬性门禁 (Search Hub)

### 0. 检索中枢双核自适应架构全景导图 (Scout-and-Radar Architecture)

#### 导图 1：编译系统眼中的“二元宇宙”本质分类与工具映射全景图 (Code Data Literal vs AST Symbol Node)
```text
                                全球任意字符集 (UTF-8)
                                         │
            ┌────────────────────────────┴────────────────────────────┐
            ▼                                                         ▼
【宇宙的第一半：数据值与字面量 (Data & Literal)】         【宇宙的第二半：语法树结构符号 (AST Symbol Node)】
  (物理字符序列 / 被程序操作的数据对象)                     (代码的骨架与骨骼 / 程序的执行实体)
  ──────────────────────────────────────────               ───────────────────────────────────
  1. 字符串字面量 (引号内一切文本):                         1. 语法声明标识符 (Declaration Identifiers):
     - 日语: "ウェブページを開く"                             - 函数/方法名: fn handle_tray_open(), pub fn wnd_proc()
     - 德语: "Verbindung fehlgeschlagen"                     - 结构体/类: struct CertPushPayload, class ProxyHandler
     - 英文: "Failed to read request body"                   - 接口/特征: trait ConnectionPool, interface Session
     - 中文: "打开网页"、"读取请求体失败"                     - 枚举/类型: enum NetworkProtocol, type HandlerFunc
  2. 运行日志、报错、异常格式化模板字符串                   2. 模块与命名空间 (Modules & Namespaces):
  3. 配置项常量、环境变量键名、UUID、十六进制错误码           - package, module, crate, namespace
  4. 行内注释与多行文档说明内容                             3. 图谱拓扑指标 (Graph Centrality):
     - 不具备 AST 语法节点身份，仅为文件物理字符流            - 拥有扇入 (fan-in)、扇出 (fan-out)、调用度、PageRank
            │                                                         │
            ▼                                                         ▼
  唯一抓取工具: search.text (物理 Trigram 探针)              唯一抓取工具: search.winnow (拓扑剪枝雷达)
  (不管何种自然语言，原样字符流精确匹配抓行号)               (严禁传自然语言长句！归一化为代码实体词元扫中枢)
```

#### 导图 2：全语种中立“双核并发双探 + 拓扑升格硬锁”全生命周期状态机 (Universal Dual-Probe & Anti-Shortcut Escalation Lifecycle)
```text
                  用户输入自然语言需求 / 业务口水话 (包含 1~N 个关键词，任意语种/缩写/错误码)
                                                    │
                                                    ▼
      ┌─────────────────────────────────────────────────────────────────────────────┐
      │   【全语种自然语言特征降维形态解构三步法】                                    │
      │   1. 剔除无信息量泛词与虚词 (页面/操作/功能/page/action/view/的/for/to)        │
      │   2. 提炼物理字面量候选 (引号文本/文案标签/报错日志/错误码，保留原样字符流)      │
      │   3. 归一化代码符号实体词元 (映射为标识符命名惯例的【实体+动作】紧凑词元)         │
      └─────────────────────────────────────────────────────────────────────────────┘
                                                    │
                                                    ▼
      ┌─────────────────────────────────────────────────────────────────────────────┐
      │   【第 1 轮：B 战术 —— 单轮多维并发双探 (Turn-1 Concurrent Dual-Probe)】        │
      │   同时下发 2 个调用 (同轮返回，0 轮次额外时延，< 15KB 安全内存防爆):              │
      │   1. search.text(query="原样字面量", output:{format:"gcx"})                   │
      │   2. search.winnow(query="实体 动作", options:{min_fan_in:2}, output:{...})   │
      └─────────────────────────────────────────────────────────────────────────────┘
                                                    │
                                                    ▼
      ┌─────────────────────────────────────────────────────────────────────────────┐
      │   【核验与裁决网关：A 战略 —— 防偷懒拓扑升格硬门禁】                            │
      └─────────────────────────────────────────────────────────────────────────────┘
                                                    │
       ┌────────────────────┬───────────────────────┴───────────────────────┬───────────────────┐
       ▼                    ▼                                               ▼                   ▼
【分支 1: 双命中/完美收敛】 【分支 2: ST 命中，SW 猜偏/未中】               【分支 3: 纯机制无字面量】 【分支 4: 双 0 命中降级】
ST 锁定了物理代码行        ST 找到了具体代码 (如 wnd_proc)，              代码中绝无特定字符串字面量 ST 与 SW 首轮均无果
SW 抓到了对应模块中枢      但第 1 轮 SW 猜的英文词元未打中                 ST 命中为 0 (符合预期)    │
       │                    │ (命中代码，但致命缺失架构拓扑！)             SW 精准命中 AST 骨架中枢  │ (启动自愈降级阶梯)
       │                    ▼                                               │                   ▼
       │             【A 宪法绝对硬性拦截 (Hard Block)】                    │            1. SW 解除扇入门槛:
       │             ⚠️ 严禁直接发起 read.file！严禁直接进入 edit.batch！   │               min_fan_in: 0 覆盖叶子
       │             (严惩短路盲改，杜绝退化为人肉 grep 脏写破坏上下游)     │            2. ST 缩短短语重试:
       │                    │                                               │               拆分为单一核心词
       │                    ▼                                               │                   │
       │             【穿透提取外层真实 AST 声明符号】                      │                   │
       │             从 ST 物理行穿透提取包裹该行的外层函数名/结构体名      │                   │
       │             (如 wnd_proc / forward_handler / CertManager)          │                   │
       │                    │                                               │                   │
       │                    ▼                                               │                   │
       │             【第 2 轮：定向 search.winnow 升格】                   │                   │
       │             search.winnow(query="真实符号名", min_fan_in:0)        │                   │
       │             (100% 获取扇入/扇出依赖定界与波及面评估)               │                   │
       │                    │                                               │                   │
       └────────────────────┼───────────────────────────────────────────────┴───────────────────┘
                            │
                            ▼
              【合法进入源码精读与批量修改阶段】
               禁止全文件盲读！严格按需执行精读闭环：
               - AST 符号精读: read.symbols(symbols: ["id1", "id2"]) (严禁 gcx)
               - 局部拓扑装配: read.editing_context(target: {file:"..."}, options:{keep:"..."})
                            │
                            ▼
              【唯一法定落地修改: edit.batch 单轮批量提交】
```

#### 导图 3：检索中枢与“查-读-析-写-验”全生命周期握手流水线 (Full-Lifecycle Pipeline Handshake)
```text
┌──────────────────────────────────────────────────────────────────────────────────┐
│         检索中枢与“查-读-析-写-验”全生命周期握手流水线 (Full-Lifecycle Pipeline)  │
└──────────────────────────────────────────────────────────────────────────────────┘
                                         │
    ┌────────────────────────────────────┴────────────────────────────────────┐
    ▼                                                                         ▼
【第一阶段：查 (Search Hub)】                                               【第二阶段：读 (Read Hub)】
 1. 首动并发: search.text + search.winnow (format: "gcx")                    1. 严禁 gcx 格式 (保全 callers/callees 上下文)
 2. 拓扑升格: 穿透真实符号名定向 winnow 补齐拓扑                            2. 严禁 read.file 全量盲读 (>150行必开存根化)
 3. 产出物: 确定性候选 AST 符号 ID 数组 (2 ~ 4 个) ──────────► 握手交接 ─────► 3. 批量精读: read.symbols(symbols: 2~4个)
                                                                              4. 局部拓扑: read.editing_context(keep)
                                                                                        │
    ┌───────────────────────────────────────────────────────────────────────────────────┘
    ▼
【第三阶段：析 (Analyze/Trace Hub)】
 1. 深度依赖追踪: relations(operation="usages"|"dependents", target:{symbol:"id"})
 2. 路径定界: trace(operation="path", target:{symbol:"src"}, to:{symbol:"dst"})
 3. 架构守护: 严禁人工盲猜调用链，单次查清全闭包
    │
    ▼
【第四阶段：写 (Edit Hub)】
 1. 唯一默认法定入口: edit(operation="batch", changes:[...], dry_run:false, options:{physical_evidence:true})
 2. 单任务全生命周期原则上仅允许 1 次批量写交互，严禁切香肠式碎片轮询
    │
    ▼
【第五阶段：验 (Change Hub & Verification)】
 1. 凭据核验: change(operation="receipt") 确认变更真实入库 (mutation_status)
 2. 并发验证: change.tests / change.guards 聚合变动符号并发定位单测与架构守卫
 3. 真实运行: 宿主测试命令闭环 (cargo test / go test / npm test)
```

---

### 1. 【全语种中立双核自适应检索与拓扑升格硬门禁】（Universal Dual-Probe & Anti-Shortcut Escalation Mandates）

当用户提出需求且未提供具体文件路径时，模型严禁凭借语言惯性盲目进行单入单出的切香肠式搜索，**必须强制执行【全语种中立双核自适应检索（Dual-Probe）】与【拓扑升格硬锁（Escalation Lock）】**：

#### 准则一：【全语种口水话特征降维与形态解构法则】（Universal Prose Dissection）
不管用户使用何种人类自然语言（中文、英文、日文、德文、口语俚语、缩写或十六进制错误码），模型必须执行**三步形态解构**：
1. **剔除自然语言结构泛词与连接虚词**：
   丢弃任何语言中的空洞结构词（如 `页面/page/view/画面`、`操作/operation/action/処理`、`功能/function/feature`、`查看/show`、`的/of/for/to/in` 等），此类词元放入 query 只会稀释权重并引入巨大噪声；
2. **提取物理字面量候选（Physical String Literal Probe）**：
   捕获需求中带引号的内容、屏幕标签、菜单文字、错误日志短语、机器错误码（例如 `"打开网页"`、`"Open Webpage"`、`"Failed to read request body"`、`"0xC0000005"`、`"AUTH_TOKEN_EXPIRED"`），按**原样字符序列（Exact Characters）保留**，作为 `search.text` 的精准物理探针；
3. **归一化语法符号词元（AST Identifier Tokens）**：
   将需求背后的业务本质抽象为编程语言命名惯例的【核心实体 + 控制动作】紧凑词元（例如 `tray web`、`cert push`、`proxy body`、`crash handler`），作为 `search.winnow` 的拓扑雷达输入。

#### 准则二：【严禁向 Winnow 输入自然语言散文长句】（Absolute Ban on Natural Language Prose in Winnow）
* **编译器级底层根因**：
  Tree-sitter / AST 语法解析器仅为语法声明节点（函数名、类型名、接口名、枚举名）建立索引。代码中出现的所有自然语言散文（无论中、英、日、德）在 AST 中统一被视作 `string_literal` 数据值节点，**AST 符号索引表中绝无自然语言句子**；
* **硬性红线禁令**：
  **严禁向 `search.winnow` 传入 3 个单词以上的自然语言句子、连续短语、口水话或错误日志全句！**
  - 违规反例（严禁）：`search.winnow(query="failed to read request body")`、`query="how to open webpage in tray"`
  - 合规正例（法定）：`search.winnow(query="proxy body")`、`query="tray web"`
* **法定形态**：`search.winnow` 的 query **必须且仅能由 1 ~ 2 个代码实体命名词元（Domain Entity / Action Tokens）构成**。

#### 准则三：【首动单轮并发双探铁律】（Turn-1 Concurrent Dual-Probe Mandate）
面对任何包含具体文案、标签、日志、报错或多关键词的业务需求，**第一动作强制在单轮对话中并发下发双通道检索，严禁单步往返轮询**：
* **探针通道（search.text）**：以提取的原样字面量字符作为 `query`，显式添加 `output: {format: "gcx"}`，负责物理磁盘文件扫描，毫秒级锁定文件具体行号；
* **雷达通道（search.winnow）**：以归一化的紧凑实体词元作为 `query`，显式添加 `options: {min_fan_in: 2, limit: 15}` 与 `output: {format: "gcx"}`，负责扫描 AST 语法树，锁定核心中枢控制器与数据结构。
* **时延与预算双赢**：两通道在第 1 轮同批下发、同批返回，0 额外轮次开销，单轮响应体积严格控制在 10KB ~ 15KB 安全线内。

#### 准则四：【防短路拓扑升格绝对死锁】（Anti-Shortcut Escalation Hard Lock）
大模型天生具有“文本贪婪短路偏好（Grep Bias）”，一旦 `search.text` 抓到了物理行号，极易自作聪明跳过架构分析直接盲目修改代码。为此设立最高不可逾越的硬性死锁：
* **绝对禁令**：**严禁在仅获得 `search.text` 的行号结果后，直接对目标文件发起 `read.file` 精读或直接进入 `edit.batch` 落地修改！违者直接判定为高危违规脏写！**
* **拓扑升格门禁**：
  * 若首轮并发中 `search.winnow` 成功命中了对应模块中枢，拓扑已完备，准许进入精读；
  * 若首轮 `search.winnow` 因词元猜测偏差未命中目标，模型**必须且仅能从 `search.text` 命中的物理行中穿透提取外层真实 AST 声明符号**（即包裹该行的函数名、结构体名、接口名，例如提取出 `wnd_proc`、`forward_handler`、`CertificatePushRequest`）；
  * **强制发起第二轮定向拓扑定界**：`search.winnow(query="真实符号名", options:{min_fan_in:0})`，获取该符号在图谱中的扇入扇出、调用度与中枢权重。拿到拓扑凭据后，方可合法进入 `read.symbols` 与后续写闭环！

#### 准则五：【物理数据值 vs 语法树符号本质分工】（Literal Value vs AST Symbol Division）
* **宇宙二元法则**：
  * 寻找任何出现在双引号内的字符串、国际化资源、日志模板、UI 菜单、报错常量、配置键名、错误码十六进制 $\rightarrow$ **必须且仅能由 `search.text` 捕获**；
  * 寻找函数、结构体、类、接口等语法声明与拓扑中枢 $\rightarrow$ **必须且仅能由 `search.winnow` 裁决**。

---

### 2. 【零落盘防截断硬门禁】（Zero Spill Budget）
* **`limit` 参数定义**：最大返回匹配命中条数（Matches Count / Early-Stop Ceiling），严禁理解为行号或扫描范围；
* **硬性上限门禁**：
  * 单独立检索调用：`limit` 参数**必须且仅能设置在 20 ~ 30 之间**（严禁省略，严禁传入大于 30 的数值）；
  * 并发检索调用：每个调用的 `limit` **强制压缩至 15 ~ 20 之间**；
* **物理防爆指标**：单次检索响应体积严格锁定在 5KB ~ 9KB 黄金内存区间，硬性阻断宿主平台超过 20KB~25KB 时的磁盘转储（`output.txt`）机制，杜绝工具链脱轨。

---

### 3. 【多仓库物理隔离边界】（Multi-Repo Physical Boundary）
* **触发条件**：多项目联合索引工作区；
* **唯一法定动作**：检索本工程代码时，**必须在 `options` 中显式指定当前主项目仓库标识前缀**（如 `options: {path: "Project-A"}`）；
* **违规判定**：省略 `path` 导致检索向全图谱内所有仓库（包含第三方上游内核代码）并发泛洪，视为严重违规。

---

### 4. 【全域强制 GCX 容器契约】（Mandatory Search GCX Contract）
* **全域强制开启铁律**：**凡调用 `search` 领域门面的任何操作（包括 winnow, symbols, text, ast, files），必须且仅能显式传入 `output: {format: "gcx"}`**；
* **容器位置约束**：`format` 属性**必须且仅能放置于 `output: {format: "gcx"}` 容器对象内**，严禁平铺到顶层或误塞入 `options`（否则触发 Schema Guard 拒绝）；
* **底层源码保障机制**：
  * `search.winnow` 与 `search.symbols`：激活源码原生手写的 `encodeWinnowSymbols` 与 `encodeSearchSymbols` 矩阵编码器，15个/13个关键属性 **100% 完整保留（零数据损失）**，并硬砍掉 30%~40% 的冗余 JSON 键名字符；
  * `search.text` 与 `search.files`：Gortex 底层收到 `format: "gcx"` 会**自动平滑降级为紧凑 TOON 格式**，零报错且同样实现 Token 瘦身；
* **字段投影（Sparse Fieldset）**：字面量排查仅需确认位置时，传入 `options: {fields: "path,line,text"}`，自动剔除 AST 冗余元数据；
* **预算裁剪感知**：若响应尾部出现 `# truncated_by_budget=true`，严禁调大 limit 重搜，必须调用 `response(operation="slice")` 进行后置切片读取。

---

### 5. 【单轮多维并发门禁】（Concurrent Search Protocol）
* **触发条件**：任务涉及多个业务关键词、前后端解耦排查或多工程逻辑对比；
* **执行模式**：强制在单轮对话中一次性并发下发全部检索调用，严禁拆分为单步往返轮询；
* **并发额度红线**：
  * 单轮并发检索调用总数**严禁超过 2 ~ 3 个**；
  * 每个并发调用的 `limit` 强制设定为 **15 ~ 20 处**；
  * 确保单轮所有调用的累加体积严格控制在 **15KB 安全线以内**。

---

## 二、 确定性检索分流状态机 (Trigger-Action Matrix)

模型面对定位诉求，必须严格匹配触发条件执行唯一对应操作，严禁主观自由裁量：

| 优先级位阶 | 触发场景 (Trigger) | 唯一法定操作 (Action) | 核心必填参数与规则 |
| :---: | :--- | :--- | :--- |
| **P0 (双核首发)** | **任意自然语言口水话 / 多关键词需求 (含可见文案/报错/菜单)** | **强制单轮并发**：<br>1. `search.text` (搜原样物理字面量)<br>2. `search.winnow` (搜归一化实体词元) | 并发 1: `query: "原样字面量"`, `options: {limit: 15, fields: "path,line,text"}`, `output: {format: "gcx"}`<br>并发 2: `query: "实体 动作词元"`, `options: {min_fan_in: 2, limit: 15}`, `output: {format: "gcx"}` |
| **P0 (拓扑中枢)** | **纯抽象业务逻辑 / 协议算法 / 底层机制 (代码中绝无特定字符串字面量)** | `search(operation="winnow")` | `query: "代码实体词元"`, `options: {min_fan_in: 2, limit: 20}`, `output: {format: "gcx"}` |
| **P1 (拓扑升格)** | **`search.text` 命中物理行后，强制穿透外层符号定界** | `search(operation="winnow")` | `query: "外层真实声明符号名"`, `options: {min_fan_in: 0, limit: 20}`, `output: {format: "gcx"}` |
| **P2 (字面探针)** | **确切的错误日志、异常格式化模板、托盘菜单名、十六进制错误码、配置 Key** | `search(operation="text")` | `query: "原样字面量"`, `options: {limit: 20, fields: "path,line,text"}`, `output: {format: "gcx"}` |
| **P3 (符号定义)** | **已知具体类型/结构体/函数全名，查找单一定义点** | `search(operation="symbols")` | `query: "精确符号名"`, `options: {assist: "off", limit: 20}`, `output: {format: "gcx"}` |
| **P4 (语法模式)** | **查找语法调用模式 / 结构级写法 (AST Pattern)** | `search(operation="ast")` | `query: "S表达式"`, `options: {language: "rust", limit: 20}`, `output: {format: "gcx"}` |
| **P5 (文件路径)** | **查找物理文件路径 / 特定扩展名** | `search(operation="files")` | `query: "文件名"`, `options: {glob: "*.rs", limit: 20}`, `output: {format: "gcx"}` |

---

## 三、 零命中确定性降级与升格状态机 (Fallback & Escalation Protocol)

当执行检索出现 0 条匹配结果或未完全收敛时，必须严格按照以下状态机执行，严禁停止流程或凭空臆测：

* **通道 1：首轮并发后 `winnow` 未收敛降级阶梯**：
  * Level 1 (ST 命中物理行，SW 结果为空或偏离): **触发 A 宪法升格硬锁**！严禁读写代码，穿透提取 ST 外层真实符号名（如 `wnd_proc` / `forward_handler`），执行定向 `search.winnow(query="真实符号名", options:{min_fan_in:0, limit:20})` 补齐拓扑。
  * Level 2 (ST 与 SW 双 0 命中): 解除拓扑扇入限制，松弛执行 `search.winnow(query="核心实体词元", options:{min_fan_in:0, limit:20})` 覆盖叶子函数与入口。
  * Level 3 (仍无果): 缩短字面量短语为单一核心词，调用 `search.text(options:{limit:20, fields:"path,line,text"})` 降维重试。
* **通道 2：纯字面量探针零命中降级阶梯**：
  * Level 1: `search.text(query="原样字面量", options:{limit:20, fields:"path,line,text"}, output:{format:"gcx"})`
  * Level 2 (0 结果): 剔除外部标点与修饰词，拆分为更短核心词重试。
  * Level 3 (仍无果): 调用 `search.files(options:{glob:"*.rs", limit:20})` 查找对应功能模块目录直接研读。

---

## 四、 检索调用标准模板 (完全脱敏规范)

### 1. 核心业务概念拓扑检索模板 (`search.winnow`)
```json
search({
  "operation": "winnow",
  "query": "certificate push",
  "options": {"limit": 20, "min_fan_in": 2, "path": "Project-A/backend"},
  "output": {"format": "gcx"}
})
```

### 2. 真实符号升格定向拓扑检索模板 (`search.winnow`)
```json
search({
  "operation": "winnow",
  "query": "forward_handler",
  "options": {"limit": 20, "min_fan_in": 0, "path": "Project-A"},
  "output": {"format": "gcx"}
})
```

### 3. 物理字面量与报错日志探针模板 (`search.text`)
```json
search({
  "operation": "text",
  "query": "Failed to read request body",
  "options": {"limit": 20, "path": "Project-A", "fields": "path,line,text"},
  "output": {"format": "gcx"}
})
```

### 4. 符号定义点检索模板 (`search.symbols`)
```json
search({
  "operation": "symbols",
  "query": "CertificatePushRequest",
  "options": {"kind": "struct", "assist": "off", "limit": 20, "path": "Project-A"},
  "output": {"format": "gcx"}
})
```

### 5. 结构级语法检索模板 (`search.ast`)
```json
search({
  "operation": "ast",
  "query": "(call_expression function: (identifier) @fn (#eq? @fn \"to_wide_chars\"))",
  "options": {
    "language": "rust",
    "path": "Project-A",
    "limit": 20
  },
  "output": {"format": "gcx"}
})
```

### 6. 文件路径定位模板 (`search.files`)
```json
search({
  "operation": "files",
  "query": "tray",
  "options": {"glob": "*.rs", "limit": 20, "path": "Project-A"},
  "output": {"format": "gcx"}
})
```

---

## 五、 单轮并发检索实战模式 (Concurrent Search Patterns)

### 模式 A：任意自然语言口水话与多关键词【全语种双核双探并发】(首选实战模式)
```json
// 并发调用 1 (探针通道: 抓取需求中任意语言的物理字面量行、报错或菜单字符串)
search({
  "operation": "text",
  "query": "Open Webpage",
  "options": {"path": "Project-A", "limit": 15, "fields": "path,line,text"},
  "output": {"format": "gcx"}
})
// 并发调用 2 (雷达通道: 归一化代码实体词元抓取架构拓扑中枢，严禁传自然语言长句)
search({
  "operation": "winnow",
  "query": "tray web",
  "options": {"path": "Project-A", "limit": 15, "min_fan_in": 2},
  "output": {"format": "gcx"}
})
```

### 模式 B：多符号变体并发 (全称 vs 缩写双向收敛)
```json
// 并发调用 1 (全称实体 + 动作)
search({
  "operation": "winnow",
  "query": "certificate push",
  "options": {"path": "Project-A", "limit": 15},
  "output": {"format": "gcx"}
})
// 并发调用 2 (缩写实体 + 动作)
search({
  "operation": "winnow",
  "query": "cert push",
  "options": {"path": "Project-A", "limit": 15},
  "output": {"format": "gcx"}
})
```

### 模式 C：主项目与对照项目对比收敛 (跨仓并发)
```json
// 并发调用 1 (当前主开发项目)
search({
  "operation": "winnow",
  "query": "certificate",
  "options": {"path": "Project-A", "limit": 15},
  "output": {"format": "gcx"}
})
// 并发调用 2 (基准参考外部项目)
search({
  "operation": "winnow",
  "query": "certificate",
  "options": {"path": "Project-B", "limit": 15},
  "output": {"format": "gcx"}
})
```

---

## 六、 Gortex 源码精读中枢规范契约 (Read Hub Specification)

本规范定义 Gortex 精读工具集（`read` 领域门面）的确定性调用契约。在检索锁定候选符号后，规范代码阅读行为，杜绝全文件盲读导致的超限截断，并保全代码上下游调用链与语法闭合性。

---

### 1. 最高精读准则与硬性门禁 (Core Read Rules)

#### 准则一：【零 GCX 格式铁律】（Zero-GCX Mandate for Read）
* **硬性禁令**：**凡调用 `read` 领域门面的任何操作（包括 symbols, source, file, summary, editing_context, history, artifact），严禁传入 `format: "gcx"`，必须且仅能采用默认格式输出**；
* **底层源码根因**：
  * GCX 编码器在 `read.symbols` 底层（tools_coding.go:1386）会主动丢弃 callers/callees 上下文数组；`read.file` 无原生 GCX 编码器；精读单次体积稳定在 4KB~8KB 黄金区间，零超限风险，严禁为格式损失上下文。

#### 准则二：【`target` 容器对象化与单选互斥硬门禁】（Target Container & Mutex Selector Mandate）
* **硬性门禁**：所有定位读取目标的参数，**必须且仅能封装于 `target: { ... }` 容器对象内**，严禁将属性直接平铺在请求顶层！
  * 物理文件阅读：必须且仅能写为 `target: { "file": "path" }`（**严禁平铺写成顶层 `file: "path"` 或 `path: "path"`**）；
  * 单符号精读：必须且仅能写为 `target: { "symbol": "id" }`（**严禁平铺写成顶层 `symbol: "id"` 或 `id: "id"`**）；
  * 多符号批量精读：必须且仅能写为 `target: { "symbols": ["id1", "id2"] }`（**严禁平铺写成顶层 `symbols: [...]` 或 `ids: [...]`**）；
  * 架构资产阅读：必须且仅能写为 `target: { "artifact": "id" }`；
* **单选互斥铁律（minProperties: 1, maxProperties: 1）**：`target` 容器内部有且仅能声明单一选择器（严禁混填 file 与 symbol，否则触发 Schema Guard 拦截；平铺入参触发 validateFacadeRequestFields 的 unknown field 拦截）。

#### 准则三：【参数容器物理隔离契约】（Container Allocation Mandate）
* **硬性门禁**：入参必须严格按照 Gortex 门面参数树契约分别填入对应容器，严禁跨容器错位或平铺：
  * **`context` 容器专属字段**：**必须且仅能包含 `context_lines`、`max_lines`、`compress_bodies`**（源码 `facade_schema.go:326` 唯一绑定为 `context` 容器）；
  * **`options` 容器专属字段**：**必须且仅能包含 `offset`、`limit`、`keep`、`include_source`、`new_user_task`**；
  * **违规判定**：若将 `context_lines` 或 `compress_bodies` 误塞入顶层平铺或乱填入 `options`，会导致 Schema 校验异常或参数被静默忽略。

#### 准则四：【Symbol ID 路径正斜杠铁律】（Symbol ID Path Normalization）
* **硬性门禁**：**无论宿主操作系统为何（包括 Windows），所有 `symbol` 与 `symbols` 属性中的符号 ID，其路径分隔符必须且仅能使用正斜杠 `/`，格式严格统一为 `"仓库标识/相对路径::符号名"`**；
* **违规后果**：在 Windows 环境下若误传反斜杠 `\`（如 `"Project-A\src\net\tcp.rs::handshake"`），由于图谱底层节点索引键统一注册为正斜杠，会导致字符串比对失败，抛出 `symbol not found` 错误。

#### 准则五：【`read.symbols` 数量上限与单轮聚合契约】（Batch Quantity Ceiling & Anti-Chop）
* **硬性上限门禁**：单次调用 `read(operation="symbols")` 时，`target.symbols` 数组长度**强制锁定为 2 ~ 4 个**（严禁一次性灌入 5 个以上符号）；
  * 候选符号超标时仅筛选核心 2~3 个符号下发，防止代码量超 600 行突破 20KB 触发 output.txt 磁盘转储；
* **单轮全量抓齐（Anti-Chop）**：严禁对多个已知函数发起单步轮询（读一个等一轮）；必须在单次调用中一次性打包读取；
* **源码必填开关**：**必须显式声明 `options: {include_source: true}`**（锁定返回源码实现体，杜绝版本升级导致的默认值漂移）。

#### 准则六：【AST 符号精读与物理行切片分工铁律】（AST Completeness vs Physical Window）
* **代码阅读分工铁律**：
  * 研读函数/结构体强制调用 `read.symbols` 或 `read.source`（保证 100% 语法闭合）；`read.file` 行号切片仅适用于非 AST 符号（UI 模板、配置、特定报错行）；
* **物理文件切片硬门禁**：调用 `read.file` 时，单次读取行数上限 `limit: 100`，且必须显式传入 `options: {offset: 起始行, limit: 80~100}`。严禁对 > 150 行的物理文件发起无参数的全量盲读。

#### 准则七：【长文件存根化巡检契约】（AST Compress Protocol）
* **触发条件**：需要了解某个大文件（> 150 行）的内部全貌、成员变量和主要函数骨架；
* **唯一法定动作**：调用 `read.file`，并显式传入 `context: {compress_bodies: true}` 与 `options: {keep: "核心函数名"}`；
* **执行机理**：利用 Tree-sitter 语法压缩器，将文件内部非核心函数体自动替换为 `{ /* N lines elided */ }` 存根，**体积直接削减 70%**，同时完整保留 `keep` 指定核心函数的源码实现。

#### 准则八：【新任务首动文件重置契约】（New User Task Boundary）
* **触发条件**：用户一上来就直接提供具体文件路径开启新任务；
* **唯一法定动作**：调用 `read.file` 且必须传入 `options: {new_user_task: true}` 重置服务端状态机。

---

### 2. 确定性精读分流状态机 (Trigger-Action Matrix for Read)

模型面对阅读诉求，必须严格匹配触发场景执行唯一对应操作，严禁主观自由裁量：

| 触发场景 (Trigger) | 唯一法定操作 (Action) | 核心必填参数与门禁 | 格式与容器规则 |
| :--- | :--- | :--- | :--- |
| **`search` 命中 2 ~ 4 个关联函数/结构体** | `read(operation="symbols")` | `target: {symbols: ["id1","id2"]}`, `options: {include_source: true}`, `context: {context_lines: 3}`, 数组长度 2~4 | **严禁 gcx**（保留 callers 列表） |
| **明确研读某 1 个具体函数/结构体** | `read(operation="source")` | `target: {symbol: "id"}`, `context: {context_lines: 3}` | **严禁 gcx**（保证函数完整语法闭合） |
| **未知代码文件，需快速掌握符号大纲与依赖** | `read(operation="summary")` | `target: {file: "path"}` | **严禁 gcx**（极低 Token 提取类型分布与顶级符号） |
| **已知非代码文件明确行号（UI组件/配置/报错行）** | `read(operation="file")` | `target: {file: "path"}`, `options: {offset: 起始行, limit: 80~100}` | **严禁 gcx**（严格限制 <= 100 行） |
| **长文件（>150行）结构巡检与骨架研判** | `read(operation="file")` | `target: {file: "path"}`, `context: {compress_bodies: true}`, `options: {keep: "核心函数"}` | **严禁 gcx**（语法级存根化瘦身70%） |
| **落笔修改前装配相邻符号与导入依赖** | `read(operation="editing_context")` | `target: {file: "path"}`, `options: {keep: "目标修改函数"}` | **严禁 gcx**（装配局部编辑拓扑） |
| **追溯某特定符号演化历史与提交记录** | `read(operation="history")` | `target: {symbol: "id"}` | **严禁 gcx**（获取符号级提交追溯） |

---

### 3. 精读符号未命中确定性降级状态机 (Read-Miss Fallback Protocol)

当执行 `read.symbols` 或 `read.source` 抛出 `symbol not found` 时，严禁退回宿主工具 `read_file`，必须严格按照以下三级阶梯降级：

* **符号未命中三级降级阶梯**：
  * Level 1: 检查 symbol ID，将所有反斜杠 `\` 纠正为正斜杠 `/` 重新下发；
  * Level 2: 调用 `read(operation="summary", target:{file:"path"})` 提取真实顶级符号大纲与全称；
  * Level 3: 以校准后的真实 Symbol ID 二次精准下发 `read.source`。严禁退回宿主 `read_file`。

---

### 4. 精读调用标准模板 (完全脱敏规范)

#### 模板 1：多符号一次性批量精读 (`read.symbols` - 日常主力)
```json
read({
  "operation": "symbols",
  "target": {
    "symbols": [
      "Project-A/src/net/tcp.rs::TcpHandler::handle",
      "Project-A/src/net/tcp.rs::TcpHandler::handshake"
    ]
  },
  "options": {"include_source": true},
  "context": {"context_lines": 3}
})
```
* **参数规范**：`target.symbols` 数组限 2~4 个符号，Symbol ID 严格正斜杠 `/`，容器严格区分 `options` 与 `context`，严禁 `output.format`。

#### 模板 2：单符号独立深入精读 (`read.source`)
```json
read({
  "operation": "source",
  "target": {"symbol": "Project-A/src/net/tcp.rs::TcpHandler::handshake"},
  "context": {"context_lines": 3}
})
```

#### 模板 3：文件符号大纲与依赖摘要 (`read.summary`)
```json
read({
  "operation": "summary",
  "target": {"file": "Project-A/src/net/tcp.rs"}
})
```

#### 模板 4：非代码文件物理切片精读 (`read.file` - 窗口切片)
```json
read({
  "operation": "file",
  "target": {"file": "Project-A/frontend/src/views/Settings.vue"},
  "options": {"offset": 80, "limit": 80}
})
```

#### 模板 5：长文件存根化无损巡检 (`read.file` - 语法压缩)
```json
read({
  "operation": "file",
  "target": {"file": "Project-A/src/server.rs"},
  "context": {"compress_bodies": true},
  "options": {"keep": "start_listen,reload_config"}
})
```

#### 模板 6：落笔修改前局部拓扑装配 (`read.editing_context`)
```json
read({
  "operation": "editing_context",
  "target": {"file": "Project-A/src/net/tcp.rs"},
  "options": {"keep": "handshake"}
})
```

#### 模板 7：符号级演化历史追溯 (`read.history`)
```json
read({
  "operation": "history",
  "target": {"symbol": "Project-A/src/net/tcp.rs::TcpHandler::handshake"}
})
```

---

## 七、 Gortex 确定性宏观分析中枢规范契约 (Analyze Hub Specification)

本中枢规范定义基于 AST 与图数据库的确定性宏观静态分析调用契约（`analyze` 领域门面）。通过 100% 确定性的图算法、数据流分析与静态安全规则，直接输出结构化指标与拓扑判定，杜绝人工低效翻找。

### 0. analyze 核心规约全景导图

```text
                           [架构审计 / 并发安全 / 技术债排查]
                                           |
        +-------------------+--------------+--------------+-------------------+
        |                   |              |              |                   |
        v                   v              v              v                   v
   [拓扑图计算]        [并发安全检测]  [跨仓 API 契约]   [死代码/瘦身]       [静态代码安全]
  kind="cycles"       kind="race_writes" kind="contracts"  kind="dead_code"    kind="sast"
  options:{repo}      options:{repo}   options:{path}   options:{limit:30}  options:{limit:30}
```

### 1. 最高分析准则与硬性门禁 (Core Analyze Rules)

#### 准则一：【确定性引擎强制铁律】（Deterministic Analysis Mandate）
* **核心禁令**：针对架构成环、死代码、并发数据竞争、跨仓 API 契约对齐、热点瓶颈等全局属性排查，**严禁使用逐文件搜索或人工盲目推演！必须且仅能调用 `analyze(kind="...")`**；
* **执行机理**：`analyze` 运行于全图数十万节点与调用边之上，毫秒级执行拓扑排序、强连通分量（SCC）、PageRank 与污点追踪，结果 100% 具备确定性与语法闭包。

#### 准则二：【鉴别参数与默认字典契约】（Discriminator Contract）
* **核心规范**：`analyze` 门面的鉴别参数为 `kind`（字符串标识符），**必须显式声明具体分析类型**；
* **缺省保护机理**：若未传入 `kind` 或 `kind` 为空，底层默认执行 `kind="help"`，返回全量 94 种可用分析种类字典；
* **反向禁令**：`analyze` 门面不支持 `operation` 参数，**严禁向 `analyze` 传入 `operation`**（会被底层自动剥离）。

#### 准则三：【零落盘预算与范围收窄门禁】（Zero Spill Budget for Analyze）
* **高危场景**：在大中型工程中，全图分析（如 `dead_code`、`hotspots`、`sast`、`todos`）直接全量输出会产生数千条结果，体积极易突破 20KB 触发 Antigravity 磁盘转储 `output.txt`；
* **唯一法定收窄动作**：
  * **单仓收窄**：必须在 `options` 容器中传入 `repo: "Project-A"`，触发 `analyzeScopeAwareKinds` central 过滤机制；
  * **条数硬顶**：产生列表的分析种类必须显式传入 `limit: 30`（锁定在 10 ~ 30 条以内），确保单次响应体积严格锁定在 5KB ~ 9KB 黄金内存区间；
  * **稀疏投影**：必须配合 `fields: "..."` 过滤冗余字段，严禁全量转储。

#### 准则四：【写治理与管理操作绝对物理隔离】（Write Isolation Mandate）
* **底层事实**：`analyze` 门面声明为只读模式（`readOnly: true`）；
* **硬性隔离禁令**：历史涉及持久化写入或落盘分析的四个操作——`blame`、`coverage`、`sql_rebuild`、`temporal_verify`，**严禁在 `analyze` 门面中调用**！必须且仅能通过 `workspace_admin` 门面执行。

#### 准则五：【`target` 容器单选互斥契约】（Target Mutex Contract）
* 对需要指定分析目标的种类（如 `would_create_cycle`、`co_change`、`def_use`、`why`），目标必须且仅能封装于 `target: { ... }` 对象内或专属 options 字段中，严禁平铺到请求顶层。

---

### 2. 确定性宏观分析分流状态机 (Trigger-Action Matrix for Analyze)

模型面对架构审计、并发安全、系统演化与代码质量任务，必须严格匹配触发条件执行唯一法定动作：

| 触发场景 (Trigger) | 涉及核心领域 | 唯一法定操作 (Action) | 核心必填参数与防溢出规则 |
| :--- | :--- | :--- | :--- |
| **排查系统循环依赖 / 架构分层违规成环** | 拓扑图计算 | `analyze(kind="cycles")` | `options: {repo: "Project-A"}` |
| **推演新增依赖是否会导致成环** | 架构推演 | `analyze(kind="would_create_cycle")` | `options: {from_id: "Project-A/src/a.rs::A", to_id: "Project-A/src/b.rs::B"}` |
| **排查未被引用的废弃函数 / 死代码** | 质量与瘦身 | `analyze(kind="dead_code")` | `options: {repo: "Project-A", limit: 30}`（必须带 limit 防转储） |
| **多协程/多线程并发数据写入竞态排查** | 并发安全 | `analyze(kind="race_writes")` | `options: {repo: "Project-A"}` |
| **跨仓 / 跨微服务 API 接口契约对齐** | 系统边界 | `analyze(kind="contracts")` | `options: {role: "consumer", path: "/v1/auth"}`（匹配 HTTP/gRPC/Topic） |
| **定位全系统核心架构瓶颈节点** | 稳定性审计 | `analyze(kind="bottlenecks")` | `options: {repo: "Project-A", limit: 20}` |
| **静态代码安全漏洞与语法缺陷审计** | 静态安全 | `analyze(kind="sast")` | `options: {repo: "Project-A", limit: 30}`（190+ 漏洞检测规则） |
| **获取代码库结构健康度量化总分** | 质量度量 | `analyze(kind="health_score")` | `options: {repo: "Project-A"}` |
| **全景宏观架构与核心模块入口概览** | 全景鸟瞰 | `analyze(kind="architecture")` | `options: {repo: "Project-A"}` |
| **通道未关闭 / 协程泄露静态检测** | 并发安全 | `analyze(kind="unclosed_channels")` | `options: {repo: "Project-A"}` |
| **排查高频变动且扇入极高的代码热点** | 维护性审计 | `analyze(kind="hotspots")` | `options: {repo: "Project-A", limit: 25}` |
| **导出当前引擎全量分析字典** | 规范自省 | `analyze(kind="help")` | 无需附带任何附加参数 |

---

### 3. 分析调用标准模板 (完全脱敏规范)

#### 模板 1：架构循环依赖排查 (`analyze.cycles`)
```json
analyze({
  "kind": "cycles",
  "options": {"repo": "Project-A"}
})
```

#### 模板 2：依赖变更成环推演 (`analyze.would_create_cycle`)
```json
analyze({
  "kind": "would_create_cycle",
  "options": {"from_id": "Project-A/src/net/tcp.rs::TcpHandler", "to_id": "Project-A/src/auth/token.rs::TokenVerifier"}
})
```

#### 模板 3：未引用死代码受控排查 (`analyze.dead_code`)
```json
analyze({
  "kind": "dead_code",
  "options": {"repo": "Project-A", "limit": 30}
})
```

#### 模板 4：多协程并发写入竞态排查 (`analyze.race_writes`)
```json
analyze({
  "kind": "race_writes",
  "options": {"repo": "Project-A"}
})
```

#### 模板 5：跨微服务 API 契约配对排查 (`analyze.contracts`)
```json
analyze({
  "kind": "contracts",
  "options": {"role": "consumer", "path": "/api/v1/session"}
})
```

#### 模板 6：全系统架构瓶颈节点审计 (`analyze.bottlenecks`)
```json
analyze({
  "kind": "bottlenecks",
  "options": {"repo": "Project-A", "limit": 20}
})
```

#### 模板 7：跨语言静态代码安全漏洞扫描 (`analyze.sast`)
```json
analyze({
  "kind": "sast",
  "options": {"repo": "Project-A", "limit": 30}
})
```

---

## 八、 Gortex 局部拓扑与调用关系中枢规范契约 (Relations Hub Specification)

本中枢规范定义基于 AST 图谱的局部拓扑关联与语法引用调用契约（`relations` 领域门面）。以代码符号为中心锚点，精确向外探测其 1 ~ 2 层邻近关联网络，杜绝单步轮询与高扇出溢出。

### 0. relations 局部拓扑中枢全景导图

```text
                         [relations 局部拓扑中枢]
                                     |
        +--------------------+-------+----------------+--------------------+
        |                    |                        |                    |
        v                    v                        v                    v
  【调用网络拓扑】       【语法引用与反查】       【继承与接口派发】       【重构安全门禁与辅助】
  · callers            · usages                 · hierarchy              · references
  · dependencies       · declaration            · implementations        · import_path
  · dependents                                  · overrides              · cluster
```

### 1. 最高关系准则与硬性门禁 (Core Relations Rules)

#### 准则一：【单点局部辐射与路径遍历分工铁律】（Local Blast Radius vs Path Traversal）
* **核心职责**：`relations` 唯一法定职责为以单一 AST 符号为锚点探测其 1 ~ 2 层直接邻近网络（调用者、依赖项、接口实现、结构引用）；
* **Anti-Chop 禁令**：**严禁使用 `relations.callers` 顺藤摸瓜式逐层试探追问（切香肠式低效交互）**！当起点与目标明确时，必须且仅能调用 `trace(operation="path")` 单次全量计算最短执行路径。

#### 准则二：【高扇出符号零转储预算门禁】（Zero Spill Budget for Relations）
* **高危场景**：公共基础函数（如通用日志、配置获取、网络发送）调用方成百上千，使用默认 `depth: 2` 会引发调用树爆炸，返回数千节点打爆 20KB 触发 Antigravity 磁盘转储 `output.txt`；
* **唯一法定参数锁**：
  * **深度硬门禁**：对基础通用函数，`options` 中 `depth` 强制锁定为 `1`（严禁传 `depth >= 2`）；
  * **条数硬上限**：必须显式传入 `options: {limit: 30}`，锁定在 20 ~ 30 条以内；
  * **排除测试调用**：必须传入 `options: {exclude_tests: true}`，剔除单元测试干扰；
  * **强制 GCX 压缩**：必须传入 `output: {format: "gcx"}`，二维表格编码无损保留调用节点并削减 30% 体积，确保响应严格锁定在 5KB ~ 9KB 黄金区间。

#### 准则三：【`target` 容器单选互斥契约】（Target Mutex Contract）
* **容器对象化**：
  * 除 `declaration` 外，所有操作的目标参数必须且仅能封装于 `target: {symbol: "id"}` 容器对象内；
  * `declaration` 操作的目标参数必须且仅能封装于 `target: {query: "代码字面量"}`；
  * **严禁将 `symbol`、`id` 或 `query` 平铺到请求顶层**（会被底层 Schema Guard 直接拦截抛出 `unknown field` 错误）。

#### 准则四：【重构删除前 references 唯一法定裁决契约】（Pre-Delete Safety Verdict）
* **安全门禁**：准备删除或重命名符号前，**必须且仅能调用 `relations(operation="references")` 进行引用排查**；
* **一票否决权**：当且仅当响应返回 `referenced: false` 时方可执行后续安全删除；若 `referenced: true`，严禁直接落笔删除，必须首先根据 `evidence` 清理下游引用。

#### 准则五：【缺省 Operation 保护契约】
* 若未传入 `operation`，底层默认执行 `operation="usages"`；在编写调用时必须显式声明具体 operation，严禁省略。

---

### 2. 确定性关系分流状态机 (Trigger-Action Matrix for Relations)

| 触发场景 (Trigger) | 涉及领域 | 唯一法定操作 (Action) | 核心必填参数与门禁规则 |
| :--- | :--- | :--- | :--- |
| **查询具体函数的上游调用方（谁调用了我）** | 调用拓扑 | `relations(operation="callers")` | `target: {symbol: "id"}`, `options: {depth: 1, limit: 30, exclude_tests: true}`, `output: {format: "gcx"}` |
| **查询函数的正向直接依赖（我调用/引用了谁）** | 依赖拓扑 | `relations(operation="dependencies")` | `target: {symbol: "id"}`, `options: {depth: 2, limit: 30}`, `output: {format: "gcx"}` |
| **查询哪些符号在结构上依赖我** | 依赖拓扑 | `relations(operation="dependents")` | `target: {symbol: "id"}`, `options: {depth: 2, limit: 30}`, `output: {format: "gcx"}` |
| **全场景引用点精准定位（调用/传参/字段赋值）** | 语法引用 | `relations(operation="usages")` | `target: {symbol: "id"}`, `options: {context: "call", limit: 30}`, 可选 `group_by: "file"` |
| **从使用代码字面量反查其调用的目标声明点** | 语法反查 | `relations(operation="declaration")` | `target: {query: "fooBar("}`, `options: {path_prefix: "Project-A", limit: 20}` |
| **查询类/结构体/接口的双向完整继承树** | 面向对象 | `relations(operation="hierarchy")` | `target: {symbol: "id"}`, `options: {direction: "both", depth: 3}` |
| **查询接口（Interface/Trait）的所有具体落地实现** | 多态实现 | `relations(operation="implementations")` | `target: {symbol: "id"}`（单次查出整棵实现树，严禁逐文件搜 impl） |
| **查询虚方法/接口方法的重写子类实现** | 多态实现 | `relations(operation="overrides")` | `target: {symbol: "id"}`, `options: {direction: "children"}` |
| **删除或重命名符号前的悬挂引用安全审计** | 重构门禁 | `relations(operation="references")` | `target: {symbol: "id"}`, `options: {exclude_tests: true}`（唯一法定 pre-delete 门禁） |
| **计算把某符号引入当前文件所需的合法 import 路径** | 编码辅助 | `relations(operation="import_path")` | `options: {name: "SymbolName", path: "Project-A/src/target.rs"}` |
| **以符号为中心探测紧密协作代码簇子图** | 局部聚类 | `relations(operation="cluster")` | `target: {symbol: "id"}`, `options: {radius: 2, limit: 30}` |

---

### 3. 关系调用标准模板 (完全脱敏规范)

#### 模板 1：函数调用方防爆排查 (`relations.callers`)
```json
relations({
  "operation": "callers",
  "target": {"symbol": "Project-A/src/net/tcp.rs::TcpHandler::handshake"},
  "options": {"depth": 1, "limit": 30, "exclude_tests": true},
  "output": {"format": "gcx"}
})
```

#### 模板 2：全场景多维引用点排查 (`relations.usages`)
```json
relations({
  "operation": "usages",
  "target": {"symbol": "Project-A/src/net/tcp.rs::TcpHandler"},
  "options": {"context": "call", "limit": 30, "group_by": "file"},
  "output": {"format": "gcx"}
})
```

#### 模板 3：接口全量具体实现查找 (`relations.implementations`)
```json
relations({
  "operation": "implementations",
  "target": {"symbol": "Project-A/src/auth/mod.rs::TokenValidator"}
})
```

#### 模板 4：面向对象类继承体系查询 (`relations.hierarchy`)
```json
relations({
  "operation": "hierarchy",
  "target": {"symbol": "Project-A/src/models/user.rs::BaseAccount"},
  "options": {"direction": "both", "depth": 3}
})
```

#### 模板 5：删除/重命名安全门禁排查 (`relations.references`)
```json
relations({
  "operation": "references",
  "target": {"symbol": "Project-A/src/legacy.rs::deprecated_helper"},
  "options": {"exclude_tests": true}
})
```

#### 模板 6：使用点反查目标函数声明 (`relations.declaration`)
```json
relations({
  "operation": "declaration",
  "target": {"query": "handle_connection("},
  "options": {"path_prefix": "Project-A", "limit": 20}
})
```

#### 模板 7：跨文件合法导入路径求解 (`relations.import_path`)
```json
relations({
  "operation": "import_path",
  "options": {"name": "TokenValidator", "path": "Project-A/src/server.rs"}
})
```
---

## 九、 Gortex 调用链与路径追踪中枢规范契约 (Trace Hub Specification)

本中枢规范定义基于代码拓扑图谱与静态分析引擎的跨符号深度调用路径遍历、数据流传播分析与过程内控制流审计契约（`trace` 领域门面）。为模型提供从单点扩散到定向端到端连通性计算的最高法定路径，彻底杜绝切香肠式逐层试探。

### 0. trace 全链路追踪门面全景导图

```text
                      ┌─── 1. path (两点间定向最短调用链，带断链 Gap 诊断)
                      ├─── 2. flow (跨函数/跨模块数据流追踪：变量值的流动轨迹)
                      ├─── 3. cfg (单个函数内控制流图分析，支持 Mermaid 流程图)
                      ├─── 4. call_chain (单向正向调用树，无目标向下辐射)
trace (全链路追踪门面) ──┼─── 5. taint (安全污点分析：源头 Source 到危险宿点 Sink)
                      ├─── 6. walk (图谱自由游走，按指定边类型有预算扩散)
                      └─── 7. graph (只读管道 DSL 查询，5级流水线逃生通道)
```

### 1. 最高追踪准则与硬性门禁 (Core Trace Rules)

#### 准则一：【两点间端到端路径定向直达铁律】（Anti-Chop Direct Path Mandate）
- **核心职责**：当排查目标具有明确的调用源头（Source）与调用终点（Sink）时，**必须且仅能调用 `trace(operation="path")` 单次全量计算最短执行路径**。
- **Anti-Chop 绝对禁令**：**严禁使用 `relations.callers` 或 `relations.dependencies` 进行多轮单步逐层推演试探（切香肠式低效交互）**！
- **断链诊断机制 (Gap Diagnosis)**：当源与宿之间拓扑不可达时，底层引擎自动返回 `trace_path.gap` 诊断，明确披露链路中断的真实物理原因：
  - `furthest_from_source`：从源头顺向能到达的最远函数集；
  - `nearest_to_sink`：能反向到达宿节点的最靠近函数集；
  - `boundary_hits`：终止探索的动态分发或外部库边界；
  - `reason` 分类断因：`crosses_dynamic_dispatch`（遭遇接口虚调用未解析）、`crosses_external_boundary`（穿入第三方库/标准库）、`depth_exceeded`（深度超限）、`disconnected`（图拓扑不连通）、`src_no_out_edges`（源函数无出度调用）、`sink_no_in_edges`（宿函数无入度调用）。
  - **断链处理法定动作**：读取 `gap` 诊断中的 `furthest_from_source` 符号，直接对其下发 `read(operation="source")` 精读断点源码，严禁盲目发起全局模糊检索。

#### 准则二：【跨过程数据流 (flow) 与过程内控制流 (cfg) 正交分工】
- **跨过程变量数据流追踪 (`trace.flow`)**：当需要追踪变量、入参或返回值在多个函数/模块之间的传递扩散轨迹时，调用 `trace(operation="flow")`。
- **过程内控制流与分支审计 (`trace.cfg`)**：当需要审计单个函数内部的执行分支、循环回边、异常处理及局部变量的定义-使用链（def-use chains）时，调用 `trace(operation="cfg")`。支持通过 `options: {mermaid: true}` 生成 Mermaid 流程图。
- **跨符号安全污点扫描 (`trace.taint`)**：当执行安全审计或架构合规排查（例如用户外部输入源 Source 是否未经清洗流向危险执行宿 Sink）时，调用模式匹配驱动的 `trace(operation="taint")`。

#### 准则三：【正向调用展开零落盘预算门禁】（Zero Spill Budget for Call Chain）
- **单向无目标正向遍历 (`trace.call_chain`)**：仅用于评估某一函数向下级联触发的整个调用子树。
- **防爆参数锁**：
  - **深度硬门禁**：`options` 中 `depth` 锁定为 `2`（高扇出根函数）至 `3`（普通业务函数），严禁使用默认值 `4`；
  - **节点数量上限**：必须显式传入 `options: {limit: 30}`，严禁超过 30 节点；
  - **压缩单行输出**：必须传入 `options: {compact: true}`，节省 50%~70% 响应体积；
  - **强制 GCX 编码**：必须显式传入 `output: {format: "gcx"}`，彻底消除 20KB 磁盘转储风险。

#### 准则四：【容器分层与目标双向互斥契约】（Container Boundaries & Mutex Contract）
- **双向端点容器契约 (`path`, `flow`, `taint`)**：
  - 起始源头必须且仅能封装于 `target` 容器内：`trace.path` / `trace.flow` 传入 `target: {symbol: "id"}`；`trace.taint` 传入 `target: {query: "pattern"}`；
  - 终止宿点必须且仅能封装于 `to` 容器内：`trace.path` / `trace.flow` 传入 `to: {symbol: "id"}`；`trace.taint` 传入 `to: {query: "pattern"}`；
  - **严禁向非端点操作注入 `to`**：`call_chain`, `cfg`, `walk`, `graph` 严禁传入 `to` 容器（会被底层判为无效或污染）。
- **单点锚定容器契约 (`call_chain`, `cfg`, `walk`)**：
  - 起始符号必须且仅能封装于 `target: {symbol: "id"}`；
- **DSL 表达式容器契约 (`graph`)**：
  - 查询表达式必须且仅能封装于 `target: {query: "pipeline_dsl"}`。

#### 准则五：【缺省 Operation 保护契约】
- 若未声明 `operation`，底层默认执行 `operation="call_chain"`；在编写调用时必须显式声明具体 operation，严禁省略。

---

### 2. 确定性追踪分流状态机 (Trigger-Action Matrix for Trace)

| 触发场景 (Trigger) | 涉及领域 | 唯一法定操作 (Action) | 核心必填参数与门禁规则 |
| :--- | :--- | :--- | :--- |
| **已知起止两点，排查 A 如何调用到 B（最短调用链路）** | 定向调用链 | `trace(operation="path")` | `target: {symbol: "src"}`, `to: {symbol: "dst"}`, `options: {max_depth: 16, k: 1}`, `output: {format: "gcx"}` |
| **追踪变量/入参/数据在跨函数间的流动传递路径** | 跨过程数据流 | `trace(operation="flow")` | `target: {symbol: "src"}`, `to: {symbol: "dst"}`, `options: {max_depth: 12, max_paths: 5}`, `output: {format: "gcx"}` |
| **审计单个复杂函数内部的控制流分支与 def-use 链** | 过程内控制流 | `trace(operation="cfg")` | `target: {symbol: "id"}`, `options: {mermaid: true}`, `output: {format: "gcx"}` |
| **无特定终点，排查某函数向下触发的所有下级调用子树** | 正向调用子树 | `trace(operation="call_chain")` | `target: {symbol: "id"}`, `options: {depth: 2, limit: 30, compact: true}`, `output: {format: "gcx"}` |
| **跨系统排查未校验输入源是否扩散至危险执行点** | 安全污点分析 | `trace(operation="taint")` | `target: {query: "source_pat"}`, `to: {query: "sink_pat"}`, `options: {min_tier: "ast_resolved"}`, `output: {format: "gcx"}` |
| **沿特定边类型（如 calls/references）有预算自由图探索** | 自由拓扑游走 | `trace(operation="walk")` | `target: {symbol: "id"}`, `options: {edge_kinds: "calls", direction: "out", token_budget: 1500, max_depth: 3}`, `output: {format: "gcx"}` |
| **执行 1~5 级管道只读 DSL 查询特定复杂图拓扑** | DSL 结构查询 | `trace(operation="graph")` | `target: {query: "nodes kind=function | traverse calls out | filter path=Project-A"}`, `options: {limit: 30}`, `output: {format: "gcx"}` |

---

### 3. 路径追踪标准模板 (完全脱敏规范)

#### 模板 1：两点间最短调用链路计算 (`trace.path`)
```json
trace({
  "operation": "path",
  "target": {"symbol": "Project-A/src/api/handler.rs::handle_request"},
  "to": {"symbol": "Project-A/src/db/pool.rs::acquire_connection"},
  "options": {"max_depth": 16, "k": 1, "include_references": true},
  "output": {"format": "gcx"}
})
```

#### 模板 2：跨过程参数/返回值数据流追踪 (`trace.flow`)
```json
trace({
  "operation": "flow",
  "target": {"symbol": "Project-A/src/auth/session.rs::parse_header"},
  "to": {"symbol": "Project-A/src/service/user.rs::authenticate"},
  "options": {"max_depth": 12, "max_paths": 5},
  "output": {"format": "gcx"}
})
```

#### 模板 3：过程内控制流图与 Mermaid 流程图生成 (`trace.cfg`)
```json
trace({
  "operation": "cfg",
  "target": {"symbol": "Project-A/src/core/router.rs::dispatch_packet"},
  "options": {"mermaid": true},
  "output": {"format": "gcx"}
})
```

#### 模板 4：正向级联调用子树受控探测 (`trace.call_chain`)
```json
trace({
  "operation": "call_chain",
  "target": {"symbol": "Project-A/src/controller/order.rs::create_order"},
  "options": {"depth": 2, "limit": 30, "compact": true},
  "output": {"format": "gcx"}
})
```

#### 模板 5：输入到敏感操作的安全污点分析 (`trace.taint`)
```json
trace({
  "operation": "taint",
  "target": {"query": "path:Project-A/src/api/ kind:parameter"},
  "to": {"query": "exact:execute_raw_sql"},
  "options": {"min_tier": "ast_resolved", "max_depth": 10},
  "output": {"format": "gcx"}
})
```

#### 模板 6：沿特定边类型的局部游走探索 (`trace.walk`)
```json
trace({
  "operation": "walk",
  "target": {"symbol": "Project-A/src/core/engine.rs::Engine::start"},
  "options": {"edge_kinds": "calls,references", "direction": "out", "token_budget": 1500, "max_depth": 3},
  "output": {"format": "gcx"}
})
```

#### 模板 7：管道 DSL 高级拓扑筛选 (`trace.graph`)
```json
trace({
  "operation": "graph",
  "target": {"query": "nodes kind=interface name~Service | traverse implements in | filter path=Project-A/src/"},
  "options": {"limit": 30},
  "output": {"format": "gcx"}
})
```
---

## 十、 Gortex 代码修改与重构中枢规范契约 (Edit & Refactor Hub Specification)

本中枢规范定义基于 AST 图谱增量同步与 ACID 事务保障的代码写入、单次输入输出多处聚合编辑与语义重构契约（`edit` 与 `refactor` 领域门面）。为模型提供从单点修补到复杂跨文件重塑的最高法定写入位阶，彻底消除切香肠式试探、图谱失步与中间态脏写。

### 0. 单轮全量交付位阶、写故障核验与语义重构全景导图

#### 导图 1：单轮全量交付执行位阶架构 (Single-Turn Delivery Hierarchy)
```text
               ┌── [Level 1: 绝对法定顶层] edit(operation="batch")
               │    单次输入输出聚合多处修改，跨文件/多原子动作单轮全量原子提交（日常修改唯一法定入口）
               │
               ├── [Level 2: 全量覆写] edit(operation="write")
               │    全新源文件创建 或 结构彻底颠覆时的整文件单次全量覆盖写入
               │
               ├── [Level 3: 语义重构] refactor(operation="rename"|"move"|"delete"|"inline"|"fix_all")
单轮全量交付位阶 ──┼    全局符号改名、符号跨文件迁移、安全删除，需全 AST 引用网穿透联动
               │
               ├── [Level 4: 虚拟推演] overlay(operation="merge") -> edit.apply_overlay
               │    高危/不确定性修改，先在内存虚拟图层推演验证，最终显式物理落盘
               │
               └── [Level 5: 降级禁区] edit(operation="file" | "symbol")
                    日常业务修改绝对禁用！仅当底层遭遇未知 Schema 故障时的极端降级兜底
```

#### 导图 2：写操作异常与共享冲突退避核验状态机 (Write Failure & Shared-Lock Backoff Protocol)
```text
[写操作失败 / 外部文件锁与共享冲突报错]
         │
         ▼
【Step 1: 凭据核验】调用 change(operation="receipt")
         │ 检查上次 mutation_status，确认变更是否实际上已经在底层物理入库
         ├─已入库 ──► 终止重写，推进到验证环节
         └─未入库
              │
              ▼
【Step 2: 状态检视】调用 change(operation="detect")
              │ 核验当前未提交变动的真实符号范围
              │
              ▼
【Step 3: 指数退避与重新精读】
              └─► 等待 3 秒退避，调用 read(operation="file" | "source") 重新读取最新文件基线，
                  重新构造完整的单次输入输出多处修改批次后二次提交，彻底杜绝盲目重放造成雪崩。
```

#### 导图 3：refactor 语义重构全景导图 (Refactor Semantic Engine Architecture)
```text
                         ┌── 1. rename (跨文件全局符号协同重命名：定义点 + 跨文件调用/引用/实例化 + 接收者 + 测试函数联动)
                         ├── 2. move (符号跨文件/跨包迁移：自动改写限定符 pkg.Sym、原文件加 import、新文件剥离 import)
                         ├── 3. delete (带图谱安全门禁的符号删除：活引用一票否决拒绝删除；cascade="apply" 级联清理孤儿死代码)
refactor (语义重构门面) ──┼── 4. inline (简单单行/单返回值函数内联展开消除：自动替换所有调用处，delete_after 自动删除原定义)
                         ├── 5. apply_code_action (精准应用单项 LSP 代码动作：按 index 落地语言服务器给出的特定快速修复)
                         └── 6. fix_all (单文件 LSP 诊断自动循环收敛：循环执行 codeAction->apply 直至无报错，自动 organizeImports)
```

---

### 1. 最高修改准则与硬性门禁 (Core Edit & Refactor Rules)

#### 准则一：【单次输入输出多处聚合修改铁律】（Single-Turn Multi-Site Batch Mutation Mandate）
- **单任务原则上仅允许 1 次批量写交互**：彻底消除偷懒轮询。模型在动笔前必须完成全局通盘规划，严禁“边走边改、逐处轮询、试探推演”的切香肠式低效交互。
- **严禁以“修改仅 1 处”为借口退回单入单出**：无论是修改 1 处还是 N 处，无论是单文件还是跨文件，所有日常业务代码修改**强制且仅能使用 `edit(operation="batch")` 单次全量提交**！
- **4 大原子操作 (`op`) 混排支持**：
  - `edit_file`：`{"op": "edit_file", "path": "...", "old_string": "...", "new_string": "...", "replace_all": true}`（用于普通文本、配置项、导入声明替换）；
  - `edit_symbol`：`{"op": "edit_symbol", "id": "...", "old_source": "...", "new_source": "..."}`（用于精准替换 AST 符号体内部局部代码）；
  - `move_file`：`{"op": "move_file", "source": "...", "destination": "...", "expected_sha256": "..."}`（用于原子重命名或移动物理文件）；
  - `delete_file`：`{"op": "delete_file", "path": "...", "expected_sha256": "..."}`（用于原子删除废弃物理文件）。
- **ACID 锁定快照预检（Locked Snapshot Preflight）**：底层在统一静态快照上对全量待替换文本和哈希进行一次性预检。只要批次中任意一处旧文本不匹配或哈希校验失败，整个批次 100% 拒绝，零脏写；发生提交故障时自动还原所有触碰文件。
- **幂等事务锁 (`transaction_id`)**：支持传入调用方稳定业务标识 `transaction_id`。重试时复用相同 `transaction_id` 自动去重，杜绝二次重复写入。

#### 准则二：【全量文件生成与覆盖重写契约 (`edit.write`)】
- **触发条件**：创建全新源文件，或者文件结构发生彻底重构需要整文件单次全量重写时。
- **参数契约**：必须传入 `target: {file: "路径"}`，全量文本置于顶层 `content: "..."`。
- **落地门禁**：必须显式传入 `dry_run: false` 且附带 `options: {physical_evidence: true}`。

#### 准则三：【物理落地与沙箱预览互斥门禁】（Physical Evidence Mutex Contract）
- **沙箱模拟预览阶段**：显式传入 `dry_run: true`，获取依赖排序后的执行计划，磁盘文件保持不变。
- **真实物理落地阶段**：必须同时传入 `dry_run: false` 与 `options: {physical_evidence: true}`。缺少物理证据标记时，高危修改将被底层硬拦截。

#### 准则四：【AST 语法门禁与防脏写校验】
- **默认 AST 语法门禁**：写操作默认开启 AST 解析检查。若故意编写尚未闭合的中间草稿代码，必须显式传入 `options: {allow_parse_errors: true}`。
- **防脏写 Guard 门禁**：支持传入 `guard: {base_sha: "..."}` 或在批操作中传入 `expected_sha256`。若磁盘文件已被外部改动，底层硬拦截并要求重新读取基线。
- **UTF-16 编码硬拦截**：Gortex 底层绝对拦截对 UTF-16 文件的直接写入，抛出 `ErrUnsupportedEncoding`。遇 UTF-16 文件必须外部转码为 UTF-8。

#### 准则五：【外部共享锁冲突与写故障退避核验闭环】
- 当写操作遇到外部文件锁竞争（如构建工具、IDE 保存、后台索引器抢占）或超时报错时，**严禁盲目无脑重复发起重写**！
- 必须严格按序执行：
  1. 调用 `change(operation="receipt")` 核验上次修改是否实际上已经入库；
  2. 若未入库，调用 `change(operation="detect")` 检视未提交变动符号范围；
  3. 执行 3 秒指数退避，重新调用 `read` 读取最新基线，重新组织完整的单次输入输出多处修改批次后再次提交。

#### 准则六：【语义级重构专用契约 (`refactor`)】
- **核心定位与穿透联动**：不同于 `edit` 的纯文本/符号替换，`refactor` 是基于 AST 图谱的全图协同重构引擎：
  - **全局协同重命名 (`rename`)**：传入 `target: {symbol: "id"}` 与 `options: {new_name: "新名"}`，底层自动穿透更新定义点、全图谱跨文件调用处（calls）、实例化处（instantiates）、接收者（receiver）及测试用例；自带 Pre-write Parse Gate，若改动导致语法报错则整体拒绝落盘；
  - **跨包迁移与导入修正 (`move`)**：传入 `target: {symbol: "id"}` 与 `options: {destination: "新路径"}`，跨包移动自动改写全量调用限定符（`pkga.Foo` -> `pkgb.Foo`），并在旧文件补齐新包 import、在新文件剥离无用 import；
  - **安全符号删除与孤儿级联 (`delete`)**：传入 `target: {symbol: "id"}`，底层自动执行图谱活引用检查，凡存在调用/实现边直接拒绝删除；支持传入 `options: {cascade: "apply"}` 一同级联清除因目标删除而衍生的孤儿死代码；
  - **单表达式函数内联消除 (`inline`)**：传入 `target: {symbol: "id"}`，针对单语句/单返回值微小函数，自动将所有调用点替换展开，并设置 `options: {delete_after: true}` 自动清理原函数定义；
  - **单文件 LSP 循环自愈修复 (`fix_all`)**：传入 `target: {file: "路径"}`，自动进入 `codeAction -> apply -> re-diagnose` 循环直至错误警告完全收敛，自动整理 imports（`source.organizeImports`）与快速修复（`quickfix`）；
  - **单项精确 LSP 动作落地 (`apply_code_action`)**：传入 `target: {file: "路径"}` 与 `options: {index: 序号}` 精准落地指定代码动作。

---

### 2. 确定性修改与重构分流状态机 (Trigger-Action Matrix for Edit & Refactor)

| 触发场景 (Trigger) | 执行位阶 | 唯一法定操作 (Action) | 核心必填参数与门禁规则 |
| :--- | :--- | :--- | :--- |
| **日常业务代码修改（无论 1 处还是多处、单文件或跨文件）** | Level 1 绝对顶层 | `edit(operation="batch")` | `changes: [...]`, `dry_run: false`, `options: {physical_evidence: true}`, 可选 `transaction_id` |
| **创建全新源码文件或配置文件** | Level 2 全量覆写 | `edit(operation="write")` | `target: {file: "path"}`, `content: "全量内容"`, `dry_run: false`, `options: {physical_evidence: true}` |
| **整文件结构发生颠覆性改动需全量替换** | Level 2 全量覆写 | `edit(operation="write")` | `target: {file: "path"}`, `content: "全量内容"`, `dry_run: false`, `options: {physical_evidence: true}` |
| **跨文件全局符号安全重命名（穿透所有引用点）** | Level 3 语义重构 | `refactor(operation="rename")` | `target: {symbol: "id"}`, `options: {new_name: "NewName"}`, `dry_run: false`, `options: {physical_evidence: true}` |
| **将函数/类型迁移至另一个文件并自动修正 import** | Level 3 语义重构 | `refactor(operation="move")` | `target: {symbol: "id"}`, `options: {destination: "path"}`, `dry_run: false`, `options: {physical_evidence: true}` |
| **符号安全删除（带图谱引用检查与孤儿死代码清理）** | Level 3 语义重构 | `refactor(operation="delete")` | `target: {symbol: "id"}`, 必须无外部活引用，可选 `options: {cascade: "apply"}` |
| **单表达式函数内联消除（展开至所有调用处）** | Level 3 语义重构 | `refactor(operation="inline")` | `target: {symbol: "id"}`, `options: {delete_after: true}`, `dry_run: false` |
| **精准应用语言服务器诊断生成的特定单项代码动作** | Level 3 语义重构 | `refactor(operation="apply_code_action")` | `target: {file: "path"}`, `options: {index: 0, start_line: 10}` |
| **对单文件执行 LSP 诊断问题的全量自动修复与整理 imports** | Level 3 语义重构 | `refactor(operation="fix_all")` | `target: {file: "path"}`, `dry_run: false`, `options: {physical_evidence: true}` |
| **高危或探索性推演的内存虚拟修改落盘** | Level 4 虚拟推演 | `edit(operation="apply_overlay")` | `options: {to_disk: true}` |
| **底层遭遇未知极端 Schema 故障时的单一降级** | Level 5 降级禁区 | `edit(operation="file")` | 仅作为应急兜底，日常业务修改绝对禁用！ |

---

### 3. 代码修改与语义重构标准模板 (完全脱敏规范)

#### 模板 1：日常多文件/多处修改单次输入输出原子提交 (`edit.batch`)
```json
edit({
  "operation": "batch",
  "changes": [
    {
      "op": "edit_file",
      "path": "Project-A/src/api/router.rs",
      "old_string": "use crate::handlers::old_handler;",
      "new_string": "use crate::handlers::new_handler;"
    },
    {
      "op": "edit_file",
      "path": "Project-A/src/api/router.rs",
      "old_string": ".route(\"/v1/users\", get(old_handler))",
      "new_string": ".route(\"/v1/users\", get(new_handler))"
    },
    {
      "op": "edit_symbol",
      "id": "Project-A/src/handlers/user.rs::new_handler",
      "old_source": "pub async fn new_handler() -> HttpResponse {\n    HttpResponse::Ok().finish()\n}",
      "new_source": "pub async fn new_handler(db: State<Pool>) -> HttpResponse {\n    HttpResponse::Ok().json(db.get_users().await)\n}"
    }
  ],
  "transaction_id": "tx_update_user_api_001",
  "dry_run": false,
  "options": {"physical_evidence": true}
})
```

#### 模板 2：全新源文件单次创建 (`edit.write`)
```json
edit({
  "operation": "write",
  "target": {"file": "Project-A/src/models/user.rs"},
  "content": "use serde::{Deserialize, Serialize};\n\n#[derive(Debug, Serialize, Deserialize)]\npub struct UserProfile {\n    pub id: u64,\n    pub username: String,\n}\n",
  "dry_run": false,
  "options": {"physical_evidence": true}
})
```

#### 模板 3：全局符号安全重命名与全图谱穿透 (`refactor.rename`)
```json
refactor({
  "operation": "rename",
  "target": {"symbol": "Project-A/src/auth/token.rs::validate_jwt"},
  "options": {"new_name": "verify_jwt_token", "physical_evidence": true},
  "dry_run": false
})
```

#### 模板 4：符号跨文件/跨包迁移并自动重写限定符 (`refactor.move`)
```json
refactor({
  "operation": "move",
  "target": {"symbol": "Project-A/src/utils/crypto.rs::hash_password"},
  "options": {"destination": "Project-A/src/security/hash.rs", "physical_evidence": true},
  "dry_run": false
})
```

#### 模板 5：安全符号删除与孤儿死代码级联清理 (`refactor.delete`)
```json
refactor({
  "operation": "delete",
  "target": {"symbol": "Project-A/src/legacy/auth.rs::obsolete_token_check"},
  "options": {"cascade": "apply"},
  "dry_run": false
})
```

#### 模板 6：微小单表达式函数内联消除 (`refactor.inline`)
```json
refactor({
  "operation": "inline",
  "target": {"symbol": "Project-A/src/helpers.rs::is_empty_string"},
  "options": {"delete_after": true},
  "dry_run": false
})
```

#### 模板 7：复合操作单次打包提交（代码修改 + 文件重命名 + 废弃文件删除）(`edit.batch`)
```json
edit({
  "operation": "batch",
  "changes": [
    {
      "op": "move_file",
      "source": "Project-A/src/legacy_config.rs",
      "destination": "Project-A/src/config.rs"
    },
    {
      "op": "edit_file",
      "path": "Project-A/src/main.rs",
      "old_string": "mod legacy_config;",
      "new_string": "mod config;"
    },
    {
      "op": "delete_file",
      "path": "Project-A/src/unused_stub.rs"
    }
  ],
  "dry_run": false,
  "options": {"physical_evidence": true}
})
```

#### 模板 8：单文件 LSP 诊断自动循环修复与整理 Imports (`refactor.fix_all`)
```json
refactor({
  "operation": "fix_all",
  "target": {"file": "Project-A/src/services/payment.rs"},
  "dry_run": false,
  "options": {"physical_evidence": true}
})
```

#### 模板 9：写故障/共享冲突后的状态凭据核验 (`change.receipt`)
```json
change({
  "operation": "receipt"
})
```
---

## 十一、 Gortex 变动评估与闭环验证中枢规范契约 (Change Hub Specification)

本中枢规范定义基于 AST 图谱、LSP 诊断与调用网络的全生命周期变动评估与闭环质检契约（`change` 领域门面）。本门面为纯只读/分析评估属性（`facadeEffectRead`，对代码库零副作用），为模型提供在改动前预测破坏、改动时核验凭据、改动后精准靶向测试的完整闭环防线，彻底消除盲目全量跑测试与重复脏写。

### 0. 变动评估与闭环验证全景导图

#### 导图 1：change 四大功能集群全景导图 (Change Architecture Clusters)
```text
                      ┌── 【集群 1: 落地凭据与变动检视】
                      │    ├── receipt (核查 mutation_status：物理落盘状态一票裁决，防重复写入)
                      │    └── detect (检视未提交 git diff 对应的图谱符号与真实变动范围)
                      │
                      ├── 【集群 2: 改前契约预检与影响预测】
                      │    ├── verify (预检公共签名修改：动笔前查出所有破坏下游调用的契约违规)
                      │    ├── impact (计算修改符号的风险分层爆炸半径：d=1必坏, d=2波及, d=3需测)
                      │    ├── contract (全流水线变动风险评分与策略裁决，固定 ack=false)
                      │    └── api_impact (聚焦公开 API 暴露面的漂移与受影响外部消费者)
change (评估与验证中枢) ─┼
                      ├── 【集群 3: 改后精准靶向测试与代码卫士】
                      │    ├── tests (精准靶向单测定位：沿调用链反查哪些测试文件/函数覆盖了改动)
                      │    ├── guards (架构守护规则检查：检查跨模块越权调用与协同修改违规)
                      │    ├── diagnostics (获取语言服务器 LSP 返回的最新实时错误与警告)
                      │    └── code_actions (获取针对当前诊断错误的精准修复动作)
                      │
                      └── 【集群 4: 虚拟图层与分支差异推演】
                           ├── compare_branches (对比两个 Git 分支之间的图谱符号差异)
                           ├── compare_overlay (对比内存虚拟图层与物理磁盘代码的符号差异)
                           └── simulate (内存沙箱模拟执行链推演，固定 keep=false，磁盘零污染)
```

#### 导图 2：修改后全生命周期闭环质检状态机 (Post-Edit Verification Lifecycle)
```text
【Step 1: 代码写入落地】
         │ 执行 edit(operation="batch")
         ▼
【Step 2: 落地状态核验】
         │ 调用 change(operation="receipt") 确认 mutation_status=committed
         │ 调用 change(operation="detect") 确认变动的物理符号范围
         ▼
【Step 3: 靶向单测与守卫验证】
         │ 调用 change(operation="tests") 单次聚合定位所有受波及的单元测试
         │ 调用 change(operation="guards") 聚合核验架构合规性
         ▼
【Step 4: 宿主真实执行验证】
         │ 宿主终端仅运行 Step 3 找出的针对性单测命令（如 cargo test <target>）
         ▼
【Step 5: 语法自愈（若有警告）】
         └─若有残留报错，调用 change(operation="diagnostics") 检视，或由 refactor.fix_all 一键收敛
```

---

### 1. 最高验证准则与硬性门禁 (Core Change Rules)

#### 准则一：【改动落地瞬时凭据核验铁律】（Mutation Status Verdict）
- **核心职责**：当调用 `edit.batch` 或其它写操作遇到超时、断开或报错（如“调用在截止时间放弃，后台可能仍在进行”）时，**必须且仅能调用 `change(operation="receipt")` 核查写入确认凭据**！
- **一票裁决权**：底层明确分离 `disk_status`（磁盘是否已写入）与 `graph_status`（图谱是否追上）：
  - 只要 `disk_status="committed"`，表明物理代码已 100% 写入磁盘，**严禁再次重复发起重写**！直接推进到下一步闭环验证；
  - 若 `disk_status="not_applied"` 或 `"failed"`，方可执行回退重构。

#### 准则二：【精准靶向单测定位与测试禁令】（Targeted Test Locator & Blanket Test Prohibition）
- **盲目全量测试绝对禁令**：**严禁在修改代码后盲目运行全量测试套件（如无条件执行 `cargo test` 或 `npm test`）**，这会带来严重的编译延时与上下文污染！
- **唯一法定靶向单测定位**：在代码修改落盘后，**必须且仅能调用 `change(operation="tests")`**，将本次改动的符号聚合传入 `target: {symbols: ["id1", "id2"]}`；
- 底层沿调用链反向精准计算覆盖该符号的测试文件与测试函数，直接输出针对性执行命令；宿主终端仅允许执行该靶向命令。

#### 准则三：【公共签名改动前契约预检】（Pre-Edit Contract Verification）
- **触发条件**：凡涉及公开函数、结构体方法、trait/interface 签名的改动（增加形参、修改返回值类型、调整泛型）；
- **法定动作**：在动笔前必须调用 `change(operation="verify")`，传入 `source: {changes: [{"symbol_id": "...", "new_signature": "..."}]}`；
- 底层遍历全图谱 callers 与 interface implementors，提前暴露所有破坏性违规，确保在单次批量修改前将下游所有受损调用方一并收拢进 `changes`。

#### 准则四：【架构守卫多符号聚合核查】（Multi-Symbol Guard Aggregation）
- **聚合校验契约**：调用 `change(operation="guards")` 检查协同修改规则（co-change）与架构边界规则时，必须将所有受影响的符号一次性聚合传入 `target: {symbols: ["id1", "id2"]}`，严禁逐个符号发起单步轮询。

#### 准则五：【固定参数安全锁（Fixed Arguments）】
- `change.contract` 固定参数 `ack: false`（变动风险确认走 `remember.risk_ack`）；
- `change.simulate` 固定参数 `keep: false`（模拟推演保持沙箱瞬态，持久化图层走 `overlay`）。

---

### 2. 确定性变动评估与质检分流状态机 (Trigger-Action Matrix for Change)

| 触发场景 (Trigger) | 质检阶段 | 唯一法定操作 (Action) | 核心必填参数与门禁规则 |
| :--- | :--- | :--- | :--- |
| **写操作超时/断开/报错后，核验物理落盘结果** | 落地即核验 | `change(operation="receipt")` | 可选参数 `receipt: "id"` 或 `path: "path"`，无参查询最近记录 |
| **检视当前 Git 工作区未提交改动的具体符号与爆炸半径** | 变动检视 | `change(operation="detect")` | `options: {scope: "unstaged", limit: 30}`, `output: {format: "gcx"}` |
| **修改公共函数签名或方法前，预检下游调用破坏** | 改动前预检 | `change(operation="verify")` | `source: {changes: [{"symbol_id": "id", "new_signature": "sig"}]}` |
| **评估修改一批符号的层级爆炸半径（d=1, d=2, d=3）** | 改动前预检 | `change(operation="impact")` | `target: {symbols: ["id1", "id2"]}`, `options: {limit: 30}` |
| **全流水线评估代码变更风险分数与裁决策略** | 风险评估 | `change(operation="contract")` | `source: {scope: "unstaged"}`, 固定 `ack: false` |
| **改动落盘后，精准定位覆盖了本次改动的单元测试** | 改后质检 | `change(operation="tests")` | `target: {symbols: ["id1", "id2"]}`, `options: {compact: true, depth: 3}` |
| **核查变动符号是否违背项目的架构守卫规则与边界禁令** | 改后质检 | `change(operation="guards")` | `target: {symbols: ["id1", "id2"]}`, `options: {compact: true}` |
| **获取语言服务器返回的当前文件实时语法/类型诊断问题** | 改后质检 | `change(operation="diagnostics")` | `target: {file: "path"}`, `options: {wait: true}` |
| **获取针对当前诊断错误的精准语言服务器修复动作** | 改后质检 | `change(operation="code_actions")` | `target: {file: "path"}`, `options: {start_line: 10}` |
| **对比两个 Git 分支之间的全量图谱符号差异** | 分支比对 | `change(operation="compare_branches")` | `target: {symbol: "id"}`, `options: {base_ref: "main", head_ref: "feature"}` |
| **内存沙箱模拟推演步骤，不污染物理磁盘** | 沙箱推演 | `change(operation="simulate")` | `source: {steps: "[...]"}`, 固定 `keep: false` |

---

### 3. 变动评估与质检标准模板 (完全脱敏规范)

#### 模板 1：写操作异常后的物理落盘状态核验 (`change.receipt`)
```json
change({
  "operation": "receipt"
})
```

#### 模板 2：改动落盘后精准反查靶向测试用例 (`change.tests`)
```json
change({
  "operation": "tests",
  "target": {"symbols": ["Project-A/src/auth/token.rs::verify_jwt_token"]},
  "options": {"compact": true, "depth": 3}
})
```

#### 模板 3：公共方法改动前契约破坏预检 (`change.verify`)
```json
change({
  "operation": "verify",
  "source": {
    "changes": [
      {
        "symbol_id": "Project-A/src/db/pool.rs::acquire_connection",
        "new_signature": "pub async fn acquire_connection(timeout_ms: u64) -> Result<DbConn, DbError>"
      }
    ]
  }
})
```

#### 模板 4：架构守卫规则与跨模块调用核查 (`change.guards`)
```json
change({
  "operation": "guards",
  "target": {"symbols": ["Project-A/src/api/handler.rs::process_order", "Project-A/src/models/order.rs::OrderState"]},
  "options": {"compact": true}
})
```

#### 模板 5：未提交变动符号范围检视 (`change.detect`)
```json
change({
  "operation": "detect",
  "options": {"scope": "unstaged", "limit": 30},
  "output": {"format": "gcx"}
})
```

#### 模板 6：修改前符号影响层级爆炸半径分析 (`change.impact`)
```json
change({
  "operation": "impact",
  "target": {"symbols": ["Project-A/src/core/engine.rs::Engine::stop"]},
  "options": {"limit": 30},
  "output": {"format": "gcx"}
})
```

#### 模板 7：文件当前 LSP 实时诊断问题排查 (`change.diagnostics`)
```json
change({
  "operation": "diagnostics",
  "target": {"file": "Project-A/src/services/payment.rs"},
  "options": {"wait": true}
})
```
---

## 十二、 Gortex 代码审查与协作发布中枢规范契约 (Review & Publish Hub Specification)

本中枢规范定义基于 AST 图谱分析、静态规则集与远程协作平台的代码变更审查与行级批注发布契约（`review` 与 `publish_review` 领域门面）。为模型提供在本地单轮全量质检评定、以及在远程 GitHub/GitLab 安全防误投外发的完整协同通道，彻底消除切香肠式逐文件索要 diff 与凭据外泄隐患。

### 0. 代码审查与协同发布全景导图

#### 导图 1：review 本地审阅门面全景导图 (Review Operations Architecture)
```text
                      ┌── 1. run (核心默认入口：全量扫描变更集，输出行锚定批注与 BLOCK/REVIEW/APPROVE 裁决)
                      ├── 2. pack (PR 审阅大礼包：打包行级批注 + 语义分类 + 风险评分 + 靶向测试 + 执行命令)
                      ├── 3. diff_context (富集图谱上下文：提取受 diff 波及符号的调用者、被调用者与社区风险)
review (本地审阅门面) ─┼── 4. sibling_context (兄弟文件上下文：在评审单文件时拉取并按关联度排序其它联动文件的 diff)
                      ├── 5. pr_context (确定性无 LLM 的 PR 审阅上下文汇总，输出最坏裁决)
                      ├── 6. critique (审阅批判：对生成的审阅报告进行二阶置信度反思与降噪)
                      └── 7. questions (智能生成审阅提问：为人工 Reviewer 生成最关键的架构审计问题)
```

#### 导图 2：代码审查与远程协同发布联动状态机 (Review to Publish Flow & Safety Gates)
```text
【Step 1: 本地全量审阅】
         │ 调用 review(operation="run", options: {scope: "unstaged"})
         │ 单轮全量生成带行锚点的 findings 列表与 BLOCK/REVIEW/APPROVE 裁决
         ▼
【Step 2: 裁决判定】
         ├─存在致命阻断 (BLOCK) ──► 终止发布，打回由 edit.batch 修复
         └─通过或警告 (APPROVE/WARN)
              │
              ▼
【Step 3: 远程批量内联发布 (冷门面 arguments 容器)】
              │ 调用 publish_review(operation="post", arguments: {
              │     number: 101,
              │     findings: <Step 1 生成的 findings>,
              │     summary: "自动化代码图谱审查报告: PASS",
              │     confirm_public: true,
              │     dry_run: false
              │ })
              └─► Gortex 自动触发密钥脱敏硬过滤，在 GitHub/GitLab 对应代码行一键挂载批注！
```

---

### 1. 最高审查与发布准则与硬性门禁 (Core Review & Publish Rules)

#### 准则一：【审查单轮全量交付铁律】（Single-Turn Full Review Delivery）
- **单轮全量交付**：针对本地未提交修改或 PR 分支比对，**强制且仅能调用 `review(operation="run" | "pack")` 单次全量生成报告**！
- **严禁行为**：严禁分步切香肠式讨要 diff、逐文件轮询评审；必须一次性获取带行级锚点的 findings 列表与整体验收裁决（`BLOCK` / `REVIEW` / `APPROVE`）。

#### 准则二：【确定性图谱审查强制门禁】
- 审查引擎基于 AST 图谱、静态分析规则集与语义边界运行。即使未配置外部大模型（`use_llm: false`），确定性规则包依然强制执行，精准捕捉语法陷阱、API 破坏与契约失效，彻底杜绝大模型随机幻觉。

#### 准则三：【publish_review 冷门面 arguments 容器契约】（Cold Facade Container Boundary）
- `publish_review` 为典型的冷门面（Cold Facade），其所有业务参数（`number`, `repo`, `findings`, `summary`, `confirm_public`, `dry_run` 等）**必须且仅能封装在 `arguments: { ... }` 容器对象内**！
- 严禁平铺到请求顶层（平铺将被 Schema Guard 直接判定为未知字段拦截）。

#### 准则四：【密钥脱敏硬过滤与防泄漏门禁】（Secret Redaction Guard）
- 在组装网络请求 Payload 之前，底层引擎对所有审阅批注执行密钥脱敏扫描；
- 凡引用到 API Key、Token、私钥证书块或密码明文的批注，底层自动脱敏；
- 默认遵循 `refuse_on_secret: true`：存在无法安全脱敏的敏感信息时直接丢弃该条批注，绝不让凭据泄露至外部网络。

#### 准则五：【公开仓库与 Fork 防误投门禁】（Public / Fork Target Guard）
- 若目标 PR 属于公开开源仓库（Public Repo）或 Fork 仓库（全球可访问），**必须显式传入 `arguments: {confirm_public: true}`**；
- 缺少该确认标记时，底层一票否决、硬拦截拒绝外发网络请求。

---

### 2. 确定性审查与发布分流状态机 (Trigger-Action Matrix for Review & Publish)

| 触发场景 (Trigger) | 涉及领域 | 唯一法定操作 (Action) | 核心必填参数与门禁规则 |
| :--- | :--- | :--- | :--- |
| **对当前工作区未提交修改执行全量代码审阅** | 本地代码审阅 | `review(operation="run")` | `options: {scope: "unstaged", limit: 30}`, `output: {format: "gcx"}` |
| **对两个分支比对执行全量打包审阅（CI 门禁大礼包）** | 本地代码审阅 | `review(operation="pack")` | `options: {scope: "compare", base_ref: "main", limit: 30}`, `output: {format: "gcx"}` |
| **提取 Git diff 波及符号的图谱富集调用上下文** | 审阅上下文 | `review(operation="diff_context")` | `options: {scope: "unstaged", compact: true}` |
| **评审单文件时拉取并排序其余关联文件的协同 diff** | 兄弟上下文 | `review(operation="sibling_context")` | `options: {focus_file: "Project-A/src/lib.rs", scope: "unstaged"}` |
| **对生成的审阅报告进行二阶置信度反思与降噪** | 报告批判 | `review(operation="critique")` | `options: {scope: "unstaged"}` |
| **为人工 Reviewer 生成针对本改动的核心架构提问** | 审阅提议 | `review(operation="questions")` | `options: {scope: "unstaged"}` |
| **将审查 Findings 批量发布为 GitHub/GitLab 行级评论** | 远程协作发布 | `publish_review(operation="post")` | 强制容器: `arguments: {number: 101, findings: [...], confirm_public: true}` |
| **远程发布前安全预览外发 Payload（不发真实网络请求）** | 远程发布预览 | `publish_review(operation="post")` | 强制容器: `arguments: {number: 101, findings: [...], dry_run: true}` |

---

### 3. 代码审查与发布标准模板 (完全脱敏规范)

#### 模板 1：当前工作区未提交修改全量单轮审阅 (`review.run`)
```json
review({
  "operation": "run",
  "options": {"scope": "unstaged", "limit": 30},
  "output": {"format": "gcx"}
})
```

#### 模板 2：分支合并前打包审阅大礼包 (`review.pack`)
```json
review({
  "operation": "pack",
  "options": {"scope": "compare", "base_ref": "main", "limit": 30},
  "output": {"format": "gcx"}
})
```

#### 模板 3：评审焦点文件时拉取协同关联文件 Diff (`review.sibling_context`)
```json
review({
  "operation": "sibling_context",
  "options": {
    "focus_file": "Project-A/src/core/router.rs",
    "scope": "unstaged"
  }
})
```

#### 模板 4：远程 GitHub PR 批量挂载行级内联批注 (`publish_review.post`)
```json
publish_review({
  "operation": "post",
  "arguments": {
    "number": 108,
    "summary": "Gortex 确定性架构图谱审查结论：PASS。已校验契约与安全规则。",
    "findings": "[{\"file\":\"Project-A/src/api/auth.rs\",\"line\":42,\"severity\":\"warning\",\"message\":\"未处理的 Token 过期分支\",\"rule\":\"error_handling\"}]",
    "confirm_public": true,
    "dry_run": false
  }
})
```

#### 模板 5：远程发布前沙箱脱敏安全预览 (`publish_review.post` dry_run)
```json
publish_review({
  "operation": "post",
  "arguments": {
    "number": 108,
    "findings": "[{\"file\":\"Project-A/src/api/auth.rs\",\"line\":42,\"severity\":\"warning\",\"message\":\"未处理的 Token 过期分支\"}]",
    "dry_run": true
  }
})
```
---
---

## 十三、 Gortex 虚拟图层与持久化记忆中枢规范契约 (Overlay & Memory Hub Specification)

本中枢规范定义基于内存阴影图谱（Shadow Graph View）的代码沙箱推演、以及跨会话持久认知的沉淀与召回契约（`overlay`、`remember` 与 `recall` 领域门面）。为模型提供在零磁盘污染前提下的多方案分支试错、以及在会话压缩或跨任务演变时不可违逆的规约护栏，彻底消除脏代码污染与上下文遗忘断层。

### 0. 虚拟图层与持久化认知全景导图

#### 导图 1：四中枢联动全景协同总线架构图 (Collaborative Spine Architecture)
```text
┌──────────────────────────────────────────────────────────────────────────────────┐
│                         Gortex 跨时空代码演化与认知协同总线                               │
└──────────────────────────────────────────────────────────────────────────────────┘
                                        │
           ┌────────────────────────────┼────────────────────────────┐
           ▼                            ▼                            ▼
   【虚拟沙箱推演层】           【持久化认知中枢】           【团队协作与合并雷达】
       overlay                      remember / recall                    pr
   (零磁盘污染的内存阴影图谱)     (三级记忆金字塔: 跨会话规约)    (多 PR 爆炸半径与冲突拓扑)
           │                            │                            │
           │ 1.推演高危重构              │ 2.沉淀 Invariant 规约      │ 3.跨 PR 预测拓扑冲突
           │   fork 多分支比对          │   新任务主动 surface 召回   │   计算 Merge Train 顺序
           │                            │   notebook 随 git 提交     │   CODEOWNERS 匹配审阅人
           └────────────────────────────┼────────────────────────────┘
                                        ▼
             【统一冷门面协议契约: 强制封装 arguments: { ... } 容器】
```

#### 导图 2：冷门面统一 arguments 容器与三级记忆金字塔架构 (Three-Tier Memory Pyramid)
```text
┌──────────────────────────────────────────────────────────────────────────────────┐
│              冷门面统一契约: 顶层仅接收 operation, arguments, output, view        │
└──────────────────────────────────────────────────────────────────────────────────┘
                                        │
                                        ▼
┌──────────────────────────────────────────────────────────────────────────────────┐
│ 【Layer 1: 仓库级笔记本】 remember.notebook ──► .gortex/notebook/<id>.md          │
│  - 随 Git 版本控制提交入库，跨团队共享，PR 审查全员可见，30天未活跃自动 TTL 老化 │
├──────────────────────────────────────────────────────────────────────────────────┤
│ 【Layer 2: 跨会话持久记忆】 remember.memory ──► ~/.gortex/memories 数据库        │
│  - 永久锚定 Symbol ID / 文件，包含 Invariant 架构死规约，新任务由 recall.surface 召回 │
├──────────────────────────────────────────────────────────────────────────────────┤
│ 【Layer 3: 会话级临时笔记】 remember.note ──► 当前 Session 内存缓存              │
│  - 记录单次推演决策与草稿，上下文压缩 (Compaction) 后由 recall.distill 一键提炼自愈 │
└──────────────────────────────────────────────────────────────────────────────────┘
```

#### 导图 3：跨时空代码演化全生命周期协同状态机 (End-to-End Speculative Lifecycle)
```text
【Step 1: 任务开启主动认知召回】
         │ 调用 recall(operation="surface", arguments: {task: "...", symbol_ids: "..."})
         │ 固定 mark_accessed: false，装配 Invariant 架构规约与高危禁忌
         ▼
【Step 2: 内存沙箱多分支安全推演】
         │ 调用 overlay(operation="push", arguments: {path: "...", content: "..."})
         │ 调用 overlay(operation="fork", arguments: {name: "branch-b", activate: true})
         │ 结合 change(operation="compare_branches") 对比图谱破坏度，锁定胜出方案
         ▼
【Step 3: 方案固化与经验沉淀】
         │ 调用 remember(operation="note", arguments: {body: "...", pinned: true})
         │ 架构决议调用 remember(operation="notebook", arguments: {title: "...", body: "..."})
         ▼
【Step 4: 物理原子落盘与协同合流】
         │ 调用 edit(operation="apply_overlay") 显式落盘 (to_disk: true)
         │ 推进至 pr(operation="conflicts" | "impact") 校验合并风险
```

---

### 1. 最高架构准则与硬性门禁 (Core Overlay & Memory Rules)

#### 准则一：【冷门面 arguments 容器绝对封装契约】（Cold Facade Container Boundary）
- `overlay`、`remember`、`recall` 同属于典型的冷门面（Cold Facades）；
- **容器铁律**：所有业务参数**必须且仅能封装在 `arguments: { ... }` 容器对象内**！顶层仅接收 `operation`, `arguments`, `output`, `view`；
- 严禁向顶层平铺或使用 `target`、`source` 等热容器，违者直接被底层 Schema Guard 判定为未知字段拦截。

#### 准则二：【零磁盘污染沙箱推演门禁】（Zero-Pollution Speculative Sandbox）
- 针对多方案架构选型、破坏性重构或未定型修改，**严禁向物理磁盘写脏代码**；
- 必须通过 `overlay.push` 与 `overlay.fork` 在内存中构建虚拟阴影图谱（Shadow Graph View）；
- 结合 `change.compare_branches` 比对调用链与依赖扩散，完成验证前保持磁盘物理隔离。

#### 准则三：【overlay 物理落盘显式分工门禁】
- `overlay.merge` 默认且仅限内存分支合并（**固定参数**：`to_disk: false`）；
- **物理落盘唯一法定入口**：当方案推演成熟需要落地至文件系统时，**必须且仅能调用 `edit(operation="apply_overlay", target: {file: "..."})`**（底层固定 `to_disk: true`）执行带漂移检测的原子化物理写入。

#### 准则四：【固定参数硬锁契约】（Fixed Arguments Lock）
- `overlay(operation="simulate")`：底层固定参数 `keep: true`，持久化推演结果至虚拟图层；
- `remember(operation="risk_ack")`：底层固定参数 `ack: true`，作为高危变更合同的唯一法定确认凭据；
- `recall(operation="surface")`：底层固定参数 `mark_accessed: false`，消除只读探测对记忆热度计数的脏修改。

#### 准则五：【任务开局主动召回与压缩提炼自愈铁律】
- **任务开局主动召回**：在执行核心业务代码精读或修改前，**必须调用 `recall.surface`** 主动调阅绑定的 `invariant` 规约；
- **压缩后强制提炼自愈**：在遭遇长对话上下文压缩（Compaction）后，**首动必须且仅能调用 `recall.distill`**，以极低 Token 恢复会话核心决策与顶级符号上下文。

---

### 2. 虚拟图层与记忆中枢分流状态机 (Trigger-Action Matrix)

| 触发场景 (Trigger) | 涉及领域 | 唯一法定操作 (Action) | 核心必填参数与门禁规则 |
| :--- | :--- | :--- | :--- |
| **高危或多方案修改前在内存注入虚拟文件** | 沙箱推演 | `overlay(operation="push")` | 强制容器: `arguments: {path: "...", content: "...", base_sha: "..."}` |
| **创建独立的推演具名分支以比对不同方案** | 沙箱推演 | `overlay(operation="fork")` | 强制容器: `arguments: {name: "strategy-b", activate: true}` |
| **在不同推演分支间无缝切换激活视角** | 沙箱推演 | `overlay(operation="switch")` | 强制容器: `arguments: {name: "strategy-b"}` |
| **将推演分支合并入主内存图层（不落物理盘）** | 沙箱推演 | `overlay(operation="merge")` | 强制容器: `arguments: {from: "strategy-b", into: "main"}` (底层固定 `to_disk: false`) |
| **销毁当前会话全部虚拟图层恢复纯净磁盘基线** | 沙箱推演 | `overlay(operation="drop")` | 强制容器: `arguments: {}` |
| **任务开局根据当前工作集主动召回架构规约** | 记忆召回 | `recall(operation="surface")` | 强制容器: `arguments: {task: "...", symbol_ids: "...", limit: 10}` |
| **上下文压缩 (Compaction) 后提取会话决策摘要** | 记忆提炼 | `recall(operation="distill")` | 强制容器: `arguments: {max_symbols: 10, max_files: 10}` |
| **永久固化不可违逆的跨会话架构事实规约** | 记忆沉淀 | `remember(operation="memory")` | 强制容器: `arguments: {kind: "invariant", title: "...", body: "...", symbol_ids: "...", importance: 5}` |
| **记录当前会话临时设计决策与踩坑笔记** | 会话笔记 | `remember(operation="note")` | 强制容器: `arguments: {body: "...", tags: "decision", pinned: true}` |
| **对高危变更合同执行免责与风险确认写入** | 风险确认 | `remember(operation="risk_ack")` | 强制容器: `arguments: {source: "symbols", symbols: "..."}` (底层固定 `ack: true`) |
| **沉淀随 Git 代码库提交由团队审查的架构决议** | 仓库记事本 | `remember(operation="notebook")` | 强制容器: `arguments: {title: "...", body: "...", tags: "..."}` (写入 `.gortex/notebook`) |
| **调阅仓库 Git 记事本完整 Markdown 正文** | 记事本调阅 | `recall(operation="notebook_show")` | 强制容器: `arguments: {id: "..."}` |

---

### 3. 虚拟图层与记忆中枢标准模板 (完全脱敏规范)

#### 模板 1：内存虚拟文件推送与沙箱分支分叉 (`overlay.push` & `overlay.fork`)
```json
overlay({
  "operation": "push",
  "arguments": {
    "path": "Project-A/src/net/connection_pool.rs",
    "content": "pub struct ConnectionPool { max_conns: usize, idle_timeout: u64 }",
    "base_sha": "a1b2c3d4e5f678901234567890abcdef12345678"
  }
})
```
```json
overlay({
  "operation": "fork",
  "arguments": {
    "name": "refactor-async-pool",
    "activate": true
  }
})
```

#### 模板 2：任务开局确定性架构规约主动召回 (`recall.surface`)
```json
recall({
  "operation": "surface",
  "arguments": {
    "task": "重构底层网络连接池与异步调度",
    "symbol_ids": "Project-A/src/net/connection_pool.rs::ConnectionPool",
    "limit": 10
  }
})
```

#### 模板 3：上下文压缩后核心会话摘要提炼自愈 (`recall.distill`)
```json
recall({
  "operation": "distill",
  "arguments": {
    "max_symbols": 10,
    "max_files": 10,
    "max_recent": 6
  }
})
```

#### 模板 4：永久固化不可违逆的架构规约 (`remember.memory` kind=invariant)
```json
remember({
  "operation": "memory",
  "arguments": {
    "kind": "invariant",
    "title": "连接池空闲回收锁顺序规约",
    "body": "在执行 idle_conns 回收时，必须先持有读锁校验超时，再升级为写锁截断，严禁直接加写锁造成死锁。",
    "symbol_ids": "Project-A/src/net/connection_pool.rs::ConnectionPool::reap",
    "importance": 5,
    "pinned": true
  }
})
```

#### 模板 5：记录仓库级架构决议笔记本随 Git 提交 (`remember.notebook`)
```json
remember({
  "operation": "notebook",
  "arguments": {
    "title": "网络连接池状态机演进决议",
    "body": "## 决议内容\n为避免多线程惊群效应，所有连接唤醒由单调度线程托管，详见 PR 设计。",
    "tags": "architecture,net,decision"
  }
})
```

---

## 十四、 Gortex PR 协同与合并风险中枢规范契约 (PR Hub Specification)

本中枢规范定义基于 AST 图谱、Git Forge 平台以及代码所有权体系的 Pull Request 协同评估、爆炸半径计算与合并冲突排期契约（`pr` 领域门面）。为模型提供在合并前跨多个开放 PR 预测社区拓扑碰撞、量化系统性破坏风险的确定性审计通道，彻底消除多分支无序合入导致的级联构建破坏与代码回退。

### 0. PR 协同与合并风险雷达全景导图

#### 导图 1：PR 协同与合并风险雷达架构导图 (PR Radar Architecture)
```text
                 ┌── 1. list (批量拉取 open/closed PR，产出状态标签、CI 汇总与合并阻断判定)
                 ├── 2. impact (核心入口：基于 AST 图谱计算单 PR 爆炸半径，输出 5 维加权风险分与 Review 凭据)
                 ├── 3. conflicts (合并冲突雷达：扫描多 PR 共同触及的 AST 社区，计算安全合流顺序 Merge Train)
pr (协同雷达门面)─┼── 4. risk (针对已映射符号或 base diff 执行 5 维加权风险打分: LOW/MEDIUM/HIGH/CRITICAL)
                 ├── 5. triage (海量 PR 分流排期：基于图谱风险与中心度降序排列，输出审查排期队列)
                 └── 6. reviewers (最佳审阅人责任匹配：按 CODEOWNERS(权重3)>作者(权重2)>协同变更专家(权重1) 匹配)
```

#### 导图 2：跨 PR 社区重叠与 Merge Train 安全合流排期状态机 (Conflict Groups & Safe Merge Train)
```text
【Step 1: 冲突雷达扫描】
         │ 调用 pr(operation="conflicts", arguments: {limit: 20})
         │ 底层反查: Open PRs ──► 变动文件 ──► 符号节点 ──► 属于哪个 Graph Community
         ▼
【Step 2: 碰撞检测判定】
         ├─存在多个 PR 触及同一 AST 社区 ──► 标记为 ConflictGroup (碰撞高危区)
         └─无社区重叠 ──► 独立分支可并行合入
              │
              ▼
【Step 3: 确定性安全合流序列执行 (Suggested Safe Merge Order)】
              │ 获取 conflicts 计算出的 suggested_order: [PR #102, PR #108, PR #105]
              │ 规则：风险极低、扇出极小、无破坏性签名的 PR 前置合并入主干；
              └─► 消除后续大 PR 的反复 rebase 成本与级联冲突！
```

---

### 1. 最高 PR 协同准则与硬性门禁 (Core PR Rules)

#### 准则一：【PR 门面 arguments 容器绝对封装契约】（Cold Facade Container Boundary）
- `pr` 为典型的冷门面（Cold Facade）；
- **容器铁律**：所有业务参数（`number`, `state`, `limit`, `ids`, `base`, `repo`, `prs`, `files`, `receipt`, `scrub` 等）**必须且仅能封装在 `arguments: { ... }` 容器对象内**！
- 严禁顶层平铺，顶层仅接收 `operation`, `arguments`, `output`, `view`。

#### 准则二：【PR 爆炸半径图谱前置分析门禁】（Blast Radius Pre-Audit Guard）
- 在深入阅读 PR 具体 diff 代码之前，**必须调用 `pr.impact` 或 `pr.risk`**；
- 严禁盲目直接读文件；必须依托 AST 图谱，从 5 维加权体系（Blast-radius flow, Caller fan-in, Test gap, Security keywords, Community span）获取客观风险等级（`LOW` / `MEDIUM` / `HIGH` / `CRITICAL`）。

#### 准则三：【跨 PR 拓扑冲突雷达与合流排期铁律】（Merge Train Order Mandate）
- 在多分支或团队并发提交场景下，**严禁凭经验随意决定合入次序**；
- 必须且仅能调用 `pr.conflicts` 获取基于图谱社区重叠计算出的 `suggested_order`（安全合流序列），严格遵循该顺序推进合并，最大程度消除团队 rebase 摩擦。

#### 准则四：【审阅人三级加权匹配铁律】（Three-Tier Reviewer Blend Mandate）
- 调用 `pr.reviewers` 时，底层引擎严格执行三级加权仲裁体系，严禁人为臆测审查人：
  1. **第一位阶（权重 3）**：`CODEOWNERS` 规则明确指定维护职责的个人或团队（最高法定效力）；
  2. **第二位阶（权重 2）**：变动符号历史代码的最近核心作者（Recent Authorship）；
  3. **第三位阶（权重 1）**：协同变更专家（Co-change Experts，历史上常联动修改该区域的开发者）。

#### 准则五：【零落盘与 GCX 格式契约】
- 针对 `pr.list`、`pr.impact`、`pr.triage`、`pr.conflicts`、`pr.risk`，必须配置 `limit: 20~30` 并显式传入 `output: {format: "gcx"}`，将响应体积锁定在 **5KB~9KB** 黄金内存区间，杜绝触发磁盘转储。

---

### 2. PR 协同中枢确定性分流状态机 (Trigger-Action Matrix for PR Hub)

| 触发场景 (Trigger) | 涉及领域 | 唯一法定操作 (Action) | 核心必填参数与门禁规则 |
| :--- | :--- | :--- | :--- |
| **检索开放 PR 列表并获取状态标签与 CI 阻断判定** | PR 列表分类 | `pr(operation="list")` | 强制容器: `arguments: {state: "open", limit: 30}`, `output: {format: "gcx"}` |
| **对指定 PR 开展基于图谱的爆炸半径与风险评分** | 爆炸半径分析 | `pr(operation="impact")` | 强制容器: `arguments: {number: 101, receipt: true}`, `output: {format: "gcx"}` |
| **跨多个开放 PR 检测 AST 社区碰撞并规划合流顺序** | 冲突雷达排期 | `pr(operation="conflicts")` | 强制容器: `arguments: {limit: 20}`, `output: {format: "gcx"}` |
| **对分支比对或已映射符号集执行 5 维加权风险评分** | 综合风险评估 | `pr(operation="risk")` | 强制容器: `arguments: {base: "main"}` 或 `arguments: {ids: "id1,id2"}` |
| **海量开放 PR 审查工作量智能分流与排期** | PR 审查分流 | `pr(operation="triage")` | 强制容器: `arguments: {limit: 20}`, `output: {format: "gcx"}` |
| **为待提交或已开启 PR 匹配高置信度人工审查人** | 审查人匹配 | `pr(operation="reviewers")` | 强制容器: `arguments: {number: 101}` 或 `arguments: {base: "main", limit: 10}` |

---

### 3. PR 协同中枢标准模板 (完全脱敏规范)

#### 模板 1：当前开放 PR 全景状态与 CI 阻断汇总 (`pr.list`)
```json
pr({
  "operation": "list",
  "arguments": {
    "state": "open",
    "limit": 30
  },
  "output": {"format": "gcx"}
})
```

#### 模板 2：指定 PR 爆炸半径图谱评估与安全凭据生成 (`pr.impact`)
```json
pr({
  "operation": "impact",
  "arguments": {
    "number": 105,
    "receipt": true,
    "scrub": true
  },
  "output": {"format": "gcx"}
})
```

#### 模板 3：跨 PR 拓扑社区碰撞检测与安全合流序列排期 (`pr.conflicts`)
```json
pr({
  "operation": "conflicts",
  "arguments": {
    "limit": 20
  },
  "output": {"format": "gcx"}
})
```

#### 模板 4：基于分支比对的 5 维加权综合风险评估 (`pr.risk`)
```json
pr({
  "operation": "risk",
  "arguments": {
    "base": "main"
  },
  "output": {"format": "gcx"}
})
```

#### 模板 5：基于 CODEOWNERS 与作者图谱的最佳审阅人匹配 (`pr.reviewers`)
```json
pr({
  "operation": "reviewers",
  "arguments": {
    "number": 105,
    "limit": 10
  }
})
```
---
---

## 十五、 Gortex 会话控制、截断自愈与元内省中枢规范契约 (Session, Response & Capabilities Hub Specification)

本中枢规范定义基于会话状态机、内存环形缓冲区后置切片与运行时元内省契约（`session`、`response` 与 `capabilities` 领域门面）。为模型提供在面对大输出截断时的零开销原地切片提取能力、在架构探索阶段的硬性只读安全围栏、以及动态透视底层工具真实 Schema 的自检通道，彻底消除诱发模型脱轨的 20KB 磁盘溢出（`output.txt`）死循环与字段幻觉。

### 0. 会话控制、截断自愈与元内省全景导图

#### 导图 1：运行时控制、元内省与截断自愈全景架构导图 (Session, Response & Capabilities Triad Architecture)
```text
┌──────────────────────────────────────────────────────────────────────────────────┐
│                   Gortex 运行时控制、元内省与截断自愈支撑中枢                       │
└──────────────────────────────────────────────────────────────────────────────────┘
                                        │
           ┌────────────────────────────┼────────────────────────────┐
           ▼                            ▼                            ▼
   【元内省与契约指南】         【会话状态与协同控制】         【环形缓冲与截断自愈】
      capabilities                    session                      response
   (零幻觉探索工具与 Schema)     (安全写围栏/游标/工作流/多Agent) (大响应切片/正则抽取/防落盘)
           │                            │                            │
           ├─ 1. domain 列表探查        ├─ 1. planning_mode 安全围栏  ├─ 1. stats 环形缓冲区检视
           ├─ 2. operation 规范查询     ├─ 2. workflow 阶段机约束     ├─ 2. peek 首尾折叠速览
           └─ 3. detail: "schema" 校验  ├─ 3. cursor 游标图谱单步跳跃 ├─ 3. slice 行号精准切片
                                        ├─ 4. agents 多 Agent 协同锁 ├─ 4. grep RE2 正则精准抽取
                                        └─ 5. subscribe/unsubscribe  └─ 5. export_context 导出
```

#### 导图 2：截断自愈与环形缓冲区后置切片状态机 (Anti-Spill Post-Filter Protocol)
```text
【前置调用触发截断】
  任意检索或分析工具返回: _truncated_by_limit=true 或 内容极长
         │
         ├───► 严禁行为：严禁盲目调大 limit 重新发起查询！（否则 100% 触发 20KB 磁盘转储 output.txt 导致脱轨）
         │
         ▼
【唯一法定动作: 调用 response 门面原地切片】
         │
         ├─► Step 1: 调用 response(operation="peek", arguments: {lines: 20})
         │          首尾快速折叠预览，获取目标代码行号范围
         │
         ├─► Step 2: 调用 response(operation="slice", arguments: {from: 40, to: 90})
         │          从内存环形缓冲区精确提取目标切片，消耗极低 Token，零磁盘落盘
         │
         └─► Step 3: 若目标符号明确，调用 response(operation="grep", arguments: {pattern: "fn process"})
                    精准抽取匹配行及上下文，彻底消除模型失控！
```

#### 导图 3：Session 状态机、图谱游标与多 Agent 协同生命周期导图 (Planning Mode, Nav Cursor & Agent Advisory Lock)
```text
┌──────────────────────────────────────────────────────────────────────────────────┐
│ session(operation="planning_mode", arguments: {mode: "planning"})                │
│  - 严格硬锁定：底层将 edit/refactor/write 工具从门面彻底摘除并硬拦截，保证只读探索 │
└──────────────────────────────────────────────────────────────────────────────────┘
                                        │
                                        ▼ (方案完备后)
┌──────────────────────────────────────────────────────────────────────────────────┐
│ session(operation="planning_mode", arguments: {mode: "editing"})                 │
│  - 解除只读安全锁，进入唯一法定修改流程 (edit.batch 单轮全量交付)                │
└──────────────────────────────────────────────────────────────────────────────────┘
                                        │
     ┌──────────────────────────────────┴──────────────────────────────────┐
     ▼                                                                     ▼
【单步图谱游标追踪 (cursor)】                                    【多 Agent 协同路径锁 (agents)】
 session(operation="cursor", arguments: {                        session(operation="agents", arguments: {
     action: "goto", id: "Project-A/lib.rs::Eng"                      action: "lock",
 })                                                                  lock_paths: ["Project-A/src/core/"]
 支持: goto ──► into(跳入被调用) ──► up(跳到调用方)               })
       ──► sibling(同级跳转) ──► back(历史回退)                  - 协同协商路径锁，防范多智能体写冲突！
```

---

### 1. 最高架构准则与硬性门禁 (Core Session, Response & Capabilities Rules)

#### 准则一：【截断切片铁律与禁盲目重查门禁】（Anti-Rerun & Slicing Mandate）
- 当任何检索、精读或分析工具响应中出现 `_truncated_by_limit: true` 或单次响应规模庞大时，**严禁盲目调大 limit 重新发起查询**！
- 盲目加大 limit 重跑大查询将直接导致单次响应突破 20KB~25KB 临界线，触发 Google Antigravity 环境自动转储至磁盘 `output.txt`，诱发模型调用宿主原生工具脱轨；
- **唯一法定动作**：**必须且仅能调用 `response.slice`、`response.grep` 或 `response.peek`**，直接对当前会话内存环形缓冲区中的数据进行原地切片提取，以极低 Token 消耗直达关键行。

#### 准则二：【planning_mode 只读安全围栏门禁】（Planning Mode Write Gate）
- 在任务开启、架构研判、调用链追踪与修改方案设计阶段，**必须将当前会话置于 `planning` 模式**；
- 在 `planning` 模式下，底层硬性将所有写入与重构工具（`edit_file`, `edit_symbol`, `write_file`, `rename_symbol` 等）从工具暴露面彻底摘除并实施硬拦截；
- 方案通盘查清并验证完备后，方可调用 `session(operation="planning_mode", arguments: {mode: "editing"})` 显式解锁写权限。

#### 准则三：【冷热门面容器边界物理隔离契约】（Container Boundary Separation）
- **冷门面契约（`response` 与 `session`）**：
  - 属于底层 `facadeColdDomain` 判定的冷门面，所有业务参数**必须且仅能封装在 `arguments: { ... }` 容器对象内**！
  - `session` 的事件监听专属接收顶层 `channel` 参数（如 `channel: "diagnostics"`）；
- **热门面契约（`capabilities`）**：
  - 属于系统级自省热门面，顶层直接接收 `domain`、`operation` 与 `detail`，**严禁使用 `arguments` 容器嵌套**。

#### 准则四：【元内省自查防幻觉契约】（Introspection Zero-Hallucination Mandate）
- 严禁凭记忆臆测或编造未返回的参数字段、枚举值或操作名；
- 当遭遇参数格式不确定、或怀疑底层存在特性扩展时，**必须且仅能调用 `capabilities` 门面进行运行时自查**；
- 传入 `detail: "schema"` 可获取目标操作最精确的 JSON Schema 契约定义。

#### 准则五：【多 Agent 协同与协商式路径锁契约】（Multi-Agent Advisory Lock Guard）
- 在多智能体并发开发场景下，必须通过 `session(operation="agents")` 维护活跃心跳并广播聚焦游标；
- 在对公共模块进行批量重构前，必须申请登记路径锁（`lock_paths`），防止多 Agent 并发写入产生覆盖破坏。

#### 准则六：【会话生效工具内省与按需提拔契约】（Session Tool Profile & On-Demand Promotion）
在复杂运行环境下（例如配置了 `GORTEX_TOOLS=core`、`agent`、`edit` 或裁剪模式时），模型严禁盲目猜测底层命令是否可用：
1. **工具就绪状态内省 (`tool_profile`)**：
   当调用某底层工具报错或规划使用冷门工具前，可调用 `tool_profile`（或门面 `capabilities(operation="legacy_profile")`）。服务端精准返回该工具在当前会话的绝对状态（`live` 活跃、`deferred` 延迟、`blocked` 策略屏蔽、`absent` 绝对不存在），消除调用盲区。
2. **延迟目录检索与动态提拔 (`tools_search`)**：
   隐藏在延迟目录（Deferred Catalog）中的高级底层工具，支持通过 `tools_search`（或门面 `capabilities(operation="legacy_search")`）进行关键词检索，并传入 `promote: true` 触发动态提拔（Promote-on-demand），无需重启守护进程即可将其挂载至当前会话活跃列表。

---

### 2. 运行时控制与截断自愈分流状态机 (Trigger-Action Matrix)

| 触发场景 (Trigger) | 涉及领域 | 唯一法定操作 (Action) | 核心必填参数与门禁规则 |
| :--- | :--- | :--- | :--- |
| **前置输出过长触发截断，提取特定行号代码切片** | 截断切片 | `response(operation="slice")` | 强制容器: `arguments: {from: 50, to: 100}` (缺省作用于最新缓冲) |
| **前置输出过长触发截断，通过正则表达式精准抽取** | 正则匹配 | `response(operation="grep")` | 强制容器: `arguments: {pattern: "fn handle", ignore_case: true, context: 2}` |
| **大响应首尾折叠速览以定位目标行号界限** | 首尾速览 | `response(operation="peek")` | 强制容器: `arguments: {lines: 20}` |
| **核验当前会话环形缓冲区所有暂存响应状态** | 缓冲检视 | `response(operation="stats")` | 强制容器: `arguments: {}` |
| **开启只读安全围栏，硬拦截所有写工具** | 会话安全 | `session(operation="planning_mode")` | 强制容器: `arguments: {mode: "planning"}` |
| **规划完成，解锁进入唯一法定单轮全量批修改** | 会话安全 | `session(operation="planning_mode")` | 强制容器: `arguments: {mode: "editing"}` |
| **单步图谱跳转：将游标重置到目标符号** | 游标追踪 | `session(operation="cursor")` | 强制容器: `arguments: {action: "goto", id: "Project-A/src/lib.rs::Engine"}` |
| **单步图谱跳转：跳入当前符号调用的下游函数** | 游标追踪 | `session(operation="cursor")` | 强制容器: `arguments: {action: "into"}` |
| **单步图谱跳转：跳到调用当前符号的上游函数** | 游标追踪 | `session(operation="cursor")` | 强制容器: `arguments: {action: "up"}` |
| **多 Agent 并发协同前登记心跳并申请模块路径锁** | 协同锁 | `session(operation="agents")` | 强制容器: `arguments: {action: "lock", lock_paths: ["Project-A/src/net/"]}` |
| **订阅底层实时诊断或图谱失效通知** | 事件监听 | `session(operation="subscribe")` | 顶层参数: `channel: "diagnostics"` 或 `"graph_invalidated"` |
| **内省当前会话特定工具的真实生效状态** | 工具自省 | `tool_profile` (或 `capabilities`) | 顶层参数: `tool: "verify_change"` (返回 live / deferred / blocked / absent) |
| **在延迟目录中检索底层工具并动态提拔** | 动态提拔 | `tools_search` (或 `capabilities`) | 顶层参数: `query: "cfg"`, `promote: true` (自动挂载至当前会话活跃列表) |
| **全面探查当前 MCP 服务支持的 21 大门面清单** | 元内省 | `capabilities()` | 顶层参数: `{}` (省略 domain 罗列全部门面) |
| **透视特定门面操作的权威 JSON Schema 规范** | 元内省 | `capabilities()` | 顶层参数: `domain: "trace", operation: "path", detail: "schema"` |

---

### 3. 会话控制、截断自愈与元内省标准模板 (完全脱敏规范)

#### 模板 1：输出截断后精准行号范围切片提取 (`response.slice`)
```json
response({
  "operation": "slice",
  "arguments": {
    "from": 120,
    "to": 175
  }
})
```

#### 模板 2：输出截断后 RE2 正则模式匹配与上下文提取 (`response.grep`)
```json
response({
  "operation": "grep",
  "arguments": {
    "pattern": "struct ConnectionPool",
    "ignore_case": true,
    "context": 3
  }
})
```

#### 模板 3：架构探索开局启用规划模式只读安全锁 (`session.planning_mode`)
```json
session({
  "operation": "planning_mode",
  "arguments": {
    "mode": "planning"
  }
})
```

#### 模板 4：图谱游标单步跳跃与调用关系遍历 (`session.cursor`)
```json
session({
  "operation": "cursor",
  "arguments": {
    "action": "goto",
    "id": "Project-A/src/net/connection_pool.rs::ConnectionPool::acquire"
  }
})
```
```json
session({
  "operation": "cursor",
  "arguments": {
    "action": "into"
  }
})
```

#### 模板 5：运行时权威 JSON Schema 透视校验 (`capabilities`)
```json
capabilities({
  "domain": "change",
  "operation": "verify",
  "detail": "schema"
})
```


#### 模板 6：会话工具生效状态与策略内省 (\`tool_profile\`)
\`\`\`json
tool_profile({
  "tool": "verify_change"
})
\`\`\`

#### 模板 7：延迟目录检索与底层工具按需提拔 (\`tools_search\`)
\`\`\`json
tools_search({
  "query": "cfg",
  "promote": true
})
\`\`\`


---

## 十六、 附录：Gortex 原生 MCP 资产与预制协议契约 (Native MCP Resources & Prompts Specification)

本附录规范 Gortex 0.64.11 原生实现的 MCP Resources 与 Prompts 标准协议契约。在支持该协议特性的宿主环境中，模型必须遵循以下开箱零开销读取与预制流水线调用准则。

### 1. 原生 MCP Resources 资产体系与上下文自愈契约

Gortex 服务端维护了一组 URI 寻址的只读状态资产（支持 \`notifications/resources/updated\` 推送），具有零参数、轻量级与免轮询特性：

| Resource URI | 描述与载荷形态 | 触发与使用规约 |
| :--- | :--- | :--- |
| **\`gortex://session\`** | **会话工作记忆快照**：包含近期浏览的符号列表、修改文件集以及检索查询历史。 | **【上下文压缩后强制读取】**：当会话发生长上下文压缩（Context Compaction）后，首动强制读取此 Resource，单次恢复近期操作记忆，严禁重新发起多轮探索读搜。 |
| **\`gortex://report\`** | **陌生仓库架构全局综述**：图谱规模、语言分布、热点数量、社区分块与坏味道统计。 | 陌生项目开局单次拉取，建立宏观架构心智，替代多工具拼凑。 |
| **\`gortex://god-nodes\`** | **核心上帝节点排查**：按复杂度分值（入度*2 + 出度*1.5 + 跨社区边*3）归一化排序的高风险符号列表。 | 架构重构前识别过耦合中心点。 |
| **\`gortex://stats\`** | **图谱底层统计**：节点/边按类型与语言的汇总计数、Token 节省比率。 | 环境基准校验。 |
| **\`gortex://index-health\`** | **索引健康度快照**：解析失败率、陈旧文件数、语言覆盖率。 | 等同于 \`workspace(operation="index")\`。 |
| **\`gortex://schema\`** | **图谱本体模式定义**：AST 节点类型、边关系类型（Call, Inherit, Override 等）全量规范。 | 编写高阶 AST 查询或图谱关系研判时的元模式字典。 |

### 2. 原生 MCP Prompts 预制流水线契约

服务端预置了 3 组标准研发流水线提示词，支持通过 \`prompts/get\` 单次执行标准化研判：

| Prompt 名称 | 预置参数 | 自动化流水线输出 |
| :--- | :--- | :--- |
| **\`pre_commit\`** | \`scope\` (\`unstaged\` \| \`staged\` \| \`all\`)<br>\`repo\` (可选) | 自动对比 Git Diff，单次输出变动符号列表、爆炸半径（Blast Radius）、风险评级、受影响单测集合与必须执行的本地验证命令。 |
| **\`safe_to_change\`** | \`ids\` (必填，逗号分隔的符号 ID) | 自动分析变动该符号的全局下游波及面、架构守卫规则约束、多文件变更计划（Edit Plan）与必须回归的单测靶向名单。 |
| **\`orientation\`** | 无 | 陌生仓库开局导航，单次输出拓扑组件、核心业务流与核心骨干符号。 |

### 3. 原生协议调用标准模板

#### 模板 1：上下文压缩后读取会话工作记忆 (\`gortex://session\`)
\`\`\`text
// 协议级 Resource 请求：
READ_RESOURCE gortex://session
\`\`\`

#### 模板 2：开局零开销读取架构概览与上帝节点 (\`gortex://report\`)
\`\`\`text
// 协议级 Resource 请求：
READ_RESOURCE gortex://report
\`\`\`

#### 模板 3：提交前未提交变更爆炸半径审查提示词 (\`pre_commit\`)
\`\`\`text
// 协议级 Prompt 请求：
GET_PROMPT pre_commit with arguments: {"scope": "unstaged"}
\`\`\`

---

## 十七、 运行时权威裁决位阶体系 (Conflict Resolution Hierarchy)

当规则规范、源码实现与运行时表现产生歧义或冲突时，模型必须严格按以下五大层级自顶向下裁决执行，高位阶效力绝对压制低位阶：
1. **第一位阶**：当前工具返回的真实运行时响应（包含 error 报错信息、completion 状态、view 视图、guard 拦截守卫与 effect 作用类别）；
2. **第二位阶**：当前运行时 \`capabilities\` 与 Schema 返回的权威规范（包含 request_shape 容器结构、fixed_arguments 固定参数、available 激活状态）；
3. **第三位阶**：真实物理文件系统与代码仓库的当前状态（workspace 绑定状态、index 索引就绪度、repository 物理路径）；
4. **第四位阶**：当前 Gortex 服务端守护进程源码实现逻辑；
5. **第五位阶**：本文档书面规则。
