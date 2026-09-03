# RTK：Codex 执行规则、源码审计与完整命令参考

> 本文件是给 Codex 的执行规则，不是 RTK 的安装教程。
>
> 审计依据是用户指定的官方源码目录
> `rtk-dev-0.47.0-rc.391` 中的
> `src/main.rs`、`src/discover/rules.rs`、`src/discover/registry.rs`、
> `src/filters/*.toml`、`src/hooks/rewrite_cmd.rs` 和 `src/hooks/init.rs`，以及官方
> `docs/guide/getting-started/supported-agents.md`。本文件不据此推断本机已安装的
> RTK 版本；运行时的实际 CLI 仍以 `rtk --help`、`rtk <正式子命令> --help` 与
> `rtk rewrite` 的输出为准。

## 0. 必读：文本编码与换行约束

以下规则适用于本文件以及本任务中读取或修改的所有**文本文件**（代码、配置、Markdown、
README、脚本等；二进制文件不按此规则直接改写）：

1. 全程按 **UTF-8** 读取和写入；不得依赖系统默认代码页、ANSI 编码或不明编码的自动转换。
2. 写入结果必须是 **UTF-8 无 BOM**，并统一使用 **LF**（`\n`）换行；不得写入 CRLF（`\r\n`）。
3. 严禁引入乱码、Unicode 替换字符 `U+FFFD` 或 NUL 字节（`0x00`）。中文、英文、
   正则、反斜杠、反引号和代码块必须逐字保持可读、可解析。
4. 编码校验按风险触发，不是每次小范围修改后的固定回读步骤。已完成编码确认的文本文件，
   若只通过小范围 `apply_patch` 修改，默认沿用原编码和换行，不额外读取全文件校验。
5. 出现下列任一情况时才做严格校验：新建或重建文件、转码、批量机械改写、格式化/生成工具写入、
   原文件编码未知，或发现乱码、异常换行、NUL 等迹象。严格校验项为：UTF-8 解码成功、无 UTF-8 BOM、
   无 `U+FFFD`、无 NUL 字节、仅 LF 换行。
6. 发现原文件编码异常、解码失败或校验失败时，立即停止继续覆盖；先保留现有内容并报告异常，
   不得通过猜测编码或批量重写掩盖问题。

## 1. 不可含糊的结论

1. **Codex 没有 RTK 的内置自动改写 Hook。** 当前源码没有 `rtk hook codex`，
   `rtk hook` 的正式处理器只有 `claude`、`cursor`、`gemini`、`copilot`、`droid`、
   `vibe`。对 Codex，`rtk init --codex` / `rtk init --global --codex` 写入的是
   `AGENTS.md` 与 `RTK.md` 规则文件，不是 Hook。因此 Codex 只能依靠本文件主动选择
   正确的 RTK 命令。
2. **禁止“任意命令前面都加 `rtk`”。** `rtk list`、`rtk read --lines 1:20`、
   `rtk grep --file-type rust`、任意位置的通用 `--max` 都不是默认合法语法。
3. **“`rtk <名称>` 能被执行”不等于“`<名称>` 是正式 RTK 子命令”。** 未识别的顶层
   参数会进入 fallback：若命中内置 TOML 过滤器则过滤；未命中时 RTK 仍会尝试原样执行该
   外部程序。因此 `rtk list --max 300` 会尝试启动名为 `list` 的程序，而不是列出目录。
4. **`rtk rewrite` 是映射判断器，不是目标命令的完整语法验证器。** 它会匹配 89 条静态
   rewrite 规则，也会匹配内置 TOML 过滤器；但它不验证改写结果是否是正式子命令、是否会走
   专用过滤、底层可执行文件是否存在、或 Windows Shell 语义是否相同。遇到本文件列出的
   “映射后透传/语法陷阱”时，按正式语法执行，不能盲从字符串替换。
5. **本项目的 `npm.cmd` 构建命令必须原生执行。** `npm.cmd` 不匹配源码中以
   `^npm\s+` 开头的规则；对 `npm.cmd run build`、`npm.cmd run <脚本>`，不要加 `rtk`，
   不要先跑 `rtk rewrite`，直接按项目要求执行 `npm.cmd`。

## 2. RTK 的四层机制

| 层级 | 源码入口 | 含义 | 对 Codex 的规则 |
| --- | --- | --- | --- |
| 正式 CLI | `Commands` / Clap | `rtk --help` 可列出的顶层子命令及其正式参数。 | 仅按本文件给出的精确语法使用。 |
| 专用处理器 | `src/cmds/**` | Rust 代码会重组、过滤或压缩特定工具输出。 | 优先使用；但写操作不会因此变成 dry-run。 |
| TOML fallback 过滤器 | `src/filters/*.toml` | 解析不到正式顶层命令后，命中 `match_command` 的工具仍可被过滤。 | 仅限第 8 节列出的精确模式；绝不能推广为通用前缀。 |
| 原样 fallback | `run_fallback()` | 无正式命令、无 TOML 命中时，直接启动原外部程序。 | 这不是“RTK 支持”。未知命令不要加 `rtk`。 |

正式 CLI 也可能含有**透传分支**。例如 `rtk cargo fmt` 可被解析，但源码会原样运行
`cargo fmt`；它不是专用输出压缩。第 9 节单列这类情况。

## 3. Codex 的强制执行顺序

1. 先识别 PowerShell cmdlet、PowerShell 表达式、变量、子表达式、管道、重定向、分号组合、
   文件写入流程、交互式会话和用户明确指定的原生命令。这些由 PowerShell 或原程序直接执行，
   不先调用 `rtk rewrite`，不机械加 `rtk`。
2. `npm.cmd ...`（尤其构建前端或后端）直接执行；`gofmt` 直接执行。二者不使用 RTK 前缀。
3. 若命令严格属于第 5 节的正式专用语法，使用列出的 `rtk` 形式。
4. 若命令严格命中第 7 节的 rewrite 规则，或严格命中第 8 节的 TOML 模式，并且不在第 9 节的
   透传/语法陷阱中，可使用对应的 `rtk` 形式。
5. 对其余**单一、非 PowerShell 复合语法**的外部命令，可只探测一次：

   ```powershell
   rtk rewrite '<原命令>'
   ```

   - stdout 非空且退出码为 `0` 或 `3`：stdout 是候选改写命令；先核对第 5 节的正式语法和
     第 9 节陷阱，再执行。
   - stdout 为空且退出码为 `1`：当前源码没有可用改写，执行原生命令。
   - 退出码为 `2`：命中 RTK 读取到的 Claude 权限拒绝规则；不把它当成可执行改写结果，按
     宿主审批/用户要求处理。
   - stdout 为空且退出码不是 `1` 或 `2`：不要猜测，停止把它当映射依据。
6. 对命令语义或平台行为有疑问时，先看对应正式帮助，例如 `rtk grep --help`、
   `rtk prisma --help`；**不要**尝试构造 `rtk list`、`rtk read --lines` 一类不存在的语法。

`rtk rewrite` 接受多个参数并以空格拼接，因此 `rtk rewrite git status` 与
`rtk rewrite 'git status'` 等价。为了避免 PowerShell 展开变量，探测普通字面命令时优先使用
单引号；含 `$`、`` ` ``、`$()`、重定向、PowerShell 管道或分号的命令不应交给它。

## 4. 明确禁止和已知纠错

| 错误写法 | 为什么错误 | 正确处理 |
| --- | --- | --- |
| `rtk list --max 300` | `list` 不是顶层子命令；会 fallback 到外部 `list`。 | 目录查看用 `rtk ls ...`，PowerShell 中无真实 `ls.exe` 时用 `Get-ChildItem`。 |
| `rtk read --lines 901:1150 file` | `read` 没有 `--lines` 或 `--skip`；解析失败可退回外部 `read`。 | 使用 `rtk read file --max-lines N` / `--tail-lines N`，任意行区间用 `Get-Content -Encoding utf8 | Select-Object -Skip ... -First ...`。 |
| `rtk grep pattern file --max 50` | RTK 自己的 grep 选项落在第一个原生参数之后，会被透传给底层 grep。 | `rtk grep --max 50 --max-len 120 -n 'pattern' file`。RTK 自己的三项选项必须最先写。 |
| `rtk grep --file-type rust pattern .` | `grep` 子命令没有 `--file-type`。 | 按类型搜索用 `rtk rg -t rust 'pattern' .`。 |
| `rtk diff --stat` | `rtk diff` 是文件/标准输入 diff，不是 Git diff。 | `rtk git diff --stat`。 |
| `rtk json file` “不支持 stdin” | 错误；源码将单个 `-` 解释为 stdin。 | `rtk json - --depth 2` 可以读 stdin。 |
| `rtk run` 在 Windows 上走 `sh -c` | 错误；当前源码在 Windows 用 `cmd /C`，Unix 才用 `sh -c`。 | 它仍不是 PowerShell 解释器；PowerShell 命令直接执行。 |
| `rtk hook codex` | 不存在。 | Codex 只使用 `AGENTS.md` / `RTK.md` 提示词。 |
| “`rtk hook check --agent codex` 可检查 Codex Hook” | `check` 确有 `--agent` 参数，但源码接收后未参与分支；没有 Codex Hook。 | 它只预览 rewrite registry，不能证明 Codex 存在 Hook。 |

## 5. 正式 CLI：完整顶层命令与精确语法

本节覆盖 `src/main.rs` 的 **81 个**正式顶层子命令：`ls`、`tree`、`read`、`smart`、`git`、
`gh`、`glab`、`aws`、`psql`、`pnpm`、`err`、`test`、`json`、`deps`、`env`、`find`、`diff`、
`log`、`dotnet`、`docker`、`kubectl`、`oc`、`summary`、`grep`、`rg`、`init`、`wget`、`wc`、
`gain`、`cc-economics`、`config`、`jest`、`vitest`、`ctest`、`prisma`、`tsc`、`next`、`lint`、
`prettier`、`format`、`playwright`、`cargo`、`npm`、`npx`、`curl`、`discover`、`session`、
`telemetry`、`learn`、`run`、`proxy`、`pipe`、`trust`、`untrust`、`verify`、`ruff`、`pytest`、
`mypy`、`php`、`phpunit`、`phpstan`、`pest`、`paratest`、`ecs`、`pint`、`phpt`、`rake`、
`rubocop`、`rspec`、`pip`、`uv`、`go`、`sbt`、`gt`、`golangci-lint`、`gradlew`、`mvn`、`mvnd`、
`hook-audit`、`rewrite`、`hook`。本列表以源码枚举为准，不把第 8 节的 fallback 名称混进来。

符号约定：`<...>` 是必填位置参数，`[...]` 是可选参数，`|` 表示互斥候选。
本文写作“外部子命令透传”时，特指源码的 `#[command(external_subcommand)]` 分支：它接收任意
OS 参数并调用原程序，不代表该外部子命令获得 RTK 的专用过滤或得到静态 rewrite 支持。

### 5.1 全局格式和全局选项

```text
rtk [-v|-vv|-vvv] [--ultra-compact] [--skip-env] <子命令> ...
```

- `-v` / `--verbose` 是计数选项；源码明确要求放在顶层子命令前。
- `--ultra-compact`、`--skip-env` 是全局选项。为避免与下游工具参数混淆，也放在顶层子命令前。
- `rtk --help`、`rtk help <子命令>`、`rtk --version` 是 Clap 帮助/版本入口。
- `rtk ctest` 特意禁用了 RTK 自己的 help/version flag；`rtk ctest --help` 会作为 ctest 参数。

### 5.2 文件、文本、搜索、结构化数据和通用输出

- `rtk ls [<ls 参数>...]`：调用真实 `ls`，并压缩目录列表。PowerShell 的 `ls` 是别名，不是
  `ls.exe`；没有真实二进制时使用 `Get-ChildItem`。
- `rtk tree [<tree 参数>...]`：调用真实 `tree`。Unix 的 `-L`、`-d`、`-a` 不能假定可用于
  Windows 的 `tree.com`；按底层程序语法使用。
- `rtk read <文件|->... [--level none|minimal|aggressive] [--max-lines N] [--tail-lines N] [--line-numbers]`：
  `<文件>` 至少一个；`-` 读 stdin，重复 `-` 会警告。`--max-lines` 与 `--tail-lines` 互斥；
  没有 `--lines`、`--skip` 或任意行区间语法。
- `rtk read` 的 RTK 自有短选项分别是 `-l`（`--level`）、`-m`（`--max-lines`）和
  `-n`（`--line-numbers`）；`--tail-lines` 没有短选项。`-m` 在这里是读取前 N 行，不能与
  `rtk grep --max` 的“显示结果数”混为一谈。
- `rtk smart <文件> [--model heuristic] [--force-download]`：只接受一个文件。
- `rtk json <JSON 文件|-> [--depth N] [--keys-only]`：`-` 读 stdin；默认深度 `5`。
- `rtk deps [路径]`：路径默认 `.`。
- `rtk env [-f|--filter <名称>]`：没有 `rtk env PATH` 位置参数形式。
- `rtk find [参数...]`：兼容 find 风格参数；额外支持 legacy 形式
  `rtk find <pattern> [path] [-m|--max N] [-t|--file-type f|d]`。`--file-type` 在这里存在，
  但不属于 `rtk grep`。
- `rtk diff <文件1> <文件2>`：两个文件时比较两者。源码对“只给一个位置参数”的分支直接读取
  stdin，忽略该单一参数；因此需要处理 unified diff 时使用 stdin，不能把它理解为单文件 diff。
- `rtk log [日志文件]`：无文件参数时读 stdin；它不执行 `log` 程序。
- `rtk grep [--max-len N] [--max N] [--context-only] <原生 grep 参数...>`：默认
  `--max-len 80`、`--max 200`。这三个 RTK 选项只能写在第一个 grep/rg 参数前；`-l`、`-m`
  仍是原生 grep 参数，会透传。没有 `--file-type`。
- `rtk rg <原生 rg 参数...>`：调用真实 `rg` 并使用同一搜索输出过滤器；文件类型用原生
  `-t <type>` / `--type <type>`，glob 用原生 `--glob`。
- `rtk wc [<wc 参数>...]`：调用真实 `wc`；没有文件参数时将 stdin 交给 `wc`。
- `rtk err <程序> [参数...]`：运行程序，只突出错误/警告。
- `rtk test <程序> [参数...]`：运行测试命令，只突出失败。
- `rtk summary <程序> [参数...]`：运行程序后做启发式摘要。
- `rtk pipe [--filter <名称>] [--passthrough]`：读 stdin。`--passthrough` 原样转发；可选 filter
  的完整名称为：`cargo-test`、`cargo`、`pytest`、`go-test`、`go-build`、`ctest`、`tsc`、
  `vitest`、`grep`、`rg`、`find`、`fd`、`git-log`、`git-diff`、`git-status`、`log`、`mypy`、
  `ruff-check`、`ruff-format`、`prettier`、`phpunit`、`pest`、`paratest`、`php-test`、`ecs`、
  `phpstan`、`pint`。

#### 5.2.1 `cat`、`head`、`tail` 的实际 rewrite 条件

第 7 节的 `^(cat|head|tail)\\s+` 是静态候选规则，不能把它理解为三个程序的所有参数都等价。
`src/discover/registry.rs` 在进入静态规则前还实施以下精确限制：

- `cat <文件...>` -> `rtk read <文件...>`；`cat -n <文件...>` -> `rtk read -n <文件...>`。
  若 `cat` 的第一个参数以 `-` 开头，但不是精确的 `-n ` 或 `-n<TAB>` 前缀，则 rewrite 返回无映射。
  因此 `cat -A`、`cat -v`、`cat -e`、`cat -t`、`cat -s`、`cat -b`、`cat --show-all` 都必须保留原生命令。
- `head -<N> <单个文件>` -> `rtk read <文件> --max-lines <N>`；
  `head --lines=<N> <单个文件>` 也会改写。`head -n <N> <文件>`、`head --lines <N> <文件>`、
  `head -c ...` 和带多个文件的 `head` 均不改写。普通 `head <文件>` 会被改成 `rtk read <文件>`，
  这**不会保留原生 `head` 默认只显示十行的语义**；需要精确十行时使用原生 `head`，或直接写
  `rtk read <文件> --max-lines 10`。
- `tail -<N> <单个文件>`、`tail -n <N> <单个文件>`、`tail --lines=<N> <单个文件>`、
  `tail --lines <N> <单个文件>` 分别改写为 `rtk read <文件> --tail-lines <N>`。
  普通 `tail <文件>`、`tail -c ...` 和任何多文件 `tail` 都不改写。

### 5.3 Git、代码托管和变更栈

- `rtk git [-C <path>]... [-c <key=value>]... [--git-dir <dir>] [--work-tree <dir>]`
  `[--no-pager] [--no-optional-locks] [--bare] [--literal-pathspecs]`
  `<diff|log|status|show|add|commit|checkout|push|pull|branch|fetch|stash|worktree|其它 git 子命令> [参数...]`。
  前 12 项有专用分支；其它 Git 子命令由 `GitCommands::Other` 原样透传。RTK 不会改变
  `add`、`commit`、`push`、`pull` 的副作用。
- `rtk gh <pr|issue|run|repo|api|release|其它 gh 子命令> [参数...]`：rewrite 正式覆盖前六项；
  顶层解析器接受字符串子命令，但这不承诺每个 `gh` 子命令都有专用过滤。
- `rtk glab [-R|--repo <owner/repo>] [-g|--group <group>]`
  `<mr|issue|ci|pipeline|api|release|其它 glab 子命令> [参数...]`。
- `rtk gt <log|submit|sync|restack|create|branch|其它 gt 子命令> [参数...]`：未列出的 `gt`
  子命令走透传。

### 5.4 JavaScript、TypeScript、Node.js 与前端工具

- `rtk npm [npm 参数...]`：源码会把第一个未知词视为脚本名并注入 `npm run`。明确写出
  `run` 时不重复注入；第一个参数以 `-` 开头时也不注入。源码中不注入 `run` 的 npm 子命令白名单
  **完整为**：`install`、`i`、`ci`、`uninstall`、`remove`、`rm`、`update`、`up`、`list`、`ls`、
  `outdated`、`init`、`create`、`publish`、`pack`、`link`、`audit`、`fund`、`exec`、`explain`、
  `why`、`search`、`view`、`info`、`show`、`config`、`set`、`get`、`cache`、`prune`、`dedupe`、
  `doctor`、`help`、`version`、`prefix`、`root`、`bin`、`bugs`、`docs`、`home`、`repo`、`ping`、
  `whoami`、`token`、`profile`、`team`、`access`、`owner`、`deprecate`、`dist-tag`、`star`、`stars`、
  `login`、`logout`、`adduser`、`unpublish`、`pkg`、`diff`、`rebuild`、`test`、`t`、`start`、`stop`、
  `restart`。除此名单及 `run`/选项外的首词才被当作脚本名。该正式包装器会过滤 npm 输出。
  **但本项目要求的 `npm.cmd ...` 保持原生执行。**
- `rtk npx <命令> [参数...]`：`tsc` / `typescript`、`eslint`、`prisma generate`、
  `prisma db push`、`next`、`prettier`、`playwright` 会路由到相应处理器；其它 npx 工具走
  npx 输出过滤路径。
- `rtk pnpm [-F|--filter <包>]... <list|outdated|install|typecheck|其它 pnpm 子命令> [参数...]`：
  `list` 可用 `-d|--depth N`；`list`、`outdated`、`install`、`typecheck` 有专用路径，
  其余 pnpm 子命令是原样透传。
- `rtk jest [参数...]`、`rtk vitest [参数...]`、`rtk playwright [参数...]`：测试输出过滤。
- `rtk tsc [参数...]`：TypeScript 编译错误分组。
- `rtk lint [参数...]`：ESLint/Biome/lint 路由目标；不存在 `rtk eslint`。
- `rtk prettier [参数...]`：Prettier 输出过滤；`--write` 仍会写文件。
- `rtk format [参数...]`：统一格式入口，会在项目中检测 Prettier、Black 或 Ruff format；它不是
  任意格式化工具的通用替代。
- `rtk next [参数...]`：Next.js 输出处理；rewrite 规则只识别 `next build` 形式。
- `rtk prisma generate [参数...]`。
- `rtk prisma migrate dev [-n|--name <名称>] [参数...]`。
- `rtk prisma migrate status [参数...]`。
- `rtk prisma migrate deploy [参数...]`。
- `rtk prisma db-push [参数...]`：正式名称是连字符 `db-push`，不是 `db push`。处理器会优先
  使用全局 `prisma`，否则调用 `npx prisma`。

### 5.5 Python

- `rtk pytest [参数...]`：pytest 输出过滤。
- `rtk ruff [参数...]`：Ruff `check` / `format` 等参数透传到 Ruff。
- `rtk mypy [参数...]`：mypy 错误分组。
- `rtk pip [参数...]`：pip 输出过滤并可识别 uv 环境；rewrite 规则仅自动覆盖
  `pip` / `pip3` / `uv pip` 的 `list`、`outdated`、`install`、`show`。
- `rtk uv [参数...]`：uv 命令入口；rewrite 规则自动覆盖 `uv run`、`uv sync`、
  `uv pip install`。

### 5.6 Rust、Go、.NET、C/C++、JVM、Scala、Swift 和构建工具

- `rtk cargo <build|test|clippy|check|install|nextest|其它 cargo 子命令> [参数...]`：前六项专用；
  其它子命令（包括 `fmt`、`run`）由正式外部子命令分支原样透传。
- `rtk go <test|build|vet|其它 go 子命令> [参数...]`：前三项专用；`go fmt`、`go mod`、
  `go run` 透传。`gofmt` 没有 RTK 包装，直接执行。
- `rtk golangci-lint [参数...]`：`run` 有专用路径；其它调用透传。
- `rtk dotnet <build|test|restore|format|其它 dotnet 子命令> [参数...]`：前四项专用，其他透传。
- `rtk ctest [参数...]`：CTest 输出过滤。
- `rtk gradlew [Gradle task 或参数...]`：优先运行 Windows 的 `.\gradlew.bat`、Unix 的
  `./gradlew`，没有 wrapper 才运行 `gradle`。构建、测试、connected test、lint、依赖任务有
  处理；其它任务透传。
- `rtk mvn [Maven goal 或参数...]`：优先 Windows `.\mvnw.cmd` / Unix `./mvnw`，否则 `mvn`；
  `test`、`integration-test`、`compile`、`test-compile`、`package`、`install`、`verify`、
  `deploy` 有过滤，`clean`、`site`、插件 goal、版本/help 等透传。
- `rtk mvnd [Maven goal 或参数...]`：运行 `mvnd`，过滤阶段与 `rtk mvn` 相同。
- `rtk sbt <test|compile|run|其它 sbt 子命令> [参数...]`：前三项专用，其他透传。
- `gcc`、`g++`、`xcodebuild`、`swift build` 等正式顶层命令不存在；它们属于第 8 节 TOML
  fallback。`swift test` 虽有 rewrite 规则，但没有对应 TOML 过滤器，详见第 9 节。

### 5.7 PHP、Laravel 与 Ruby

- `rtk php [参数...]`：处理 `php artisan ...` 和 `php -l ...` 等 PHP 输出。
- `rtk phpunit [参数...]`、`rtk phpstan [参数...]`、`rtk pest [参数...]`、
  `rtk paratest [参数...]`、`rtk ecs [参数...]`、`rtk pint [参数...]`、
  `rtk phpt [参数...]`：分别是 PHPUnit、PHPStan、Pest、ParaTest、ECS、Pint、
  `php run-tests.php` 的处理器。
- `rtk rake [参数...]`、`rtk rubocop [参数...]`、`rtk rspec [参数...]`：Ruby/Rails 工具处理器。
- `bundle install|update` 没有正式顶层 `bundle`，但在第 8 节 TOML fallback 中受支持。

### 5.8 容器、Kubernetes、云、数据库和网络

- `rtk docker ps [-a|--all]`。
- `rtk docker images`。
- `rtk docker logs <container>`：正式语法只定义一个容器位置参数；带额外 `docker logs` 选项时
  不要假定仍走专用分支。
- `rtk docker compose ps [-a|--all]`。
- `rtk docker compose logs [service] [--tail N]`：默认 `--tail 100`。
- `rtk docker compose build [service]`。
- 其它 `rtk docker <子命令>` 可被外部子命令接受，但走原样透传。
- `rtk kubectl get [参数...]`、`rtk kubectl pods [-n|--namespace <名称>] [-A|--all]`、
  `rtk kubectl services [-n|--namespace <名称>] [-A|--all]`、
  `rtk kubectl logs <pod> [-c|--container <名称>]`；其它 kubectl 子命令透传。
- `rtk oc get [参数...]`、`rtk oc pods [-n|--namespace <名称>] [-A|--all]`、
  `rtk oc services [-n|--namespace <名称>] [-A|--all]`、
  `rtk oc logs <pod> [-c|--container <名称>]`；其它 oc 子命令透传。
- `rtk aws <服务> [参数...]`：服务位置参数必填；会以 JSON 为中心压缩输出。
- `rtk psql [参数...]`：psql 表格压缩；交互式会话、密码提示或必须保留完整格式时用原生 psql。
- `rtk curl [参数...]`：面向真实 cURL；PowerShell `curl` 可能是别名，不能把
  `Invoke-WebRequest` 当作 cURL 改写。
- `rtk wget [-O|--output-document <文件|->] <URL> [额外 wget 参数...]`：`-O -` 走 stdout
  路径；依赖真实 wget。

### 5.9 管理、分析、信任与 Hook 命令

- `rtk init [-g|--global] [--opencode] [--gemini]`
  `[--agent claude|cursor|windsurf|cline|kilocode|antigravity|kimi|pi|hermes|droid|vibe]`
  `[--show] [--claude-md|--hook-only] [--auto-patch|--no-patch]`
  `[--trust-filters|--no-trust-filters] [--uninstall] [--codex] [--copilot] [--dry-run]`。
  `--codex` 是规则文件模式，且源码禁止它与 `--opencode`、`--claude-md`、`--hook-only`、
  `--auto-patch`、`--no-patch` 组合。
- `rtk gain [--project] [--graph] [-H|--history] [--quota --tier pro|5x|20x]`
  `[--daily] [--weekly] [--monthly] [--all] [--format text|json|csv] [--failures]`
  `[--reset --yes]`。`--reset` 有副作用。
- `rtk cc-economics [--daily] [--weekly] [--monthly] [--all] [--format text|json|csv]`。
- `rtk config [--create]`：`--create` 写默认配置。
- `rtk discover [--project <路径片段>] [--limit N] [--all] [--since 天数] [--format text|json]`。
- `rtk session`：分析 Claude Code 会话采用情况。
- `rtk telemetry <status|enable|disable|forget>`：`enable` 需要交互终端；`forget` 会删除本地
  tracking 数据并尝试发送擦除请求。
- `rtk learn [--project <路径片段>] [--all] [--since 天数] [--format text|json]`
  `[--write-rules] [--min-confidence 0..1] [--min-occurrences N]`。`--write-rules` 会写文件。
- `rtk run [-c|--command <命令字符串>] [命令参数...]`：无过滤、无 tracking。Windows 使用
  `cmd /C`，Unix 使用 `sh -c`；两者都不是 PowerShell 解析器。
- `rtk proxy <程序> [参数...]`：不经 Shell 直接启动程序、保留原始 stdout/stderr 并记录使用量。
  若要运行 PowerShell 语法，只能显式给 `pwsh -Command`，且确有保留完整输出的需要时才使用。
- `rtk trust [--list] [-y|--yes]`、`rtk untrust`：管理项目/用户 TOML 自定义过滤器的信任。
- `rtk verify [--filter <名称>] [--require-all]`：校验 Hook 完整性并运行 TOML inline tests。
- `rtk hook-audit [--since 天数]`：默认 `7` 天，需要 `RTK_HOOK_AUDIT=1` 才有审计数据。
- `rtk rewrite <原始命令参数...>`：第 3 节的映射探测器，退出码含义见第 6 节。
- `rtk hook <claude|cursor|gemini|copilot|droid|vibe>`：供这些 Agent 的 stdin JSON Hook 协议调用。
- `rtk hook check [--agent <任意字符串>] <原始命令参数...>`：只预览 rewrite；当前源码忽略
  `--agent` 值，不存在 `codex` 处理器。

## 6. `rtk rewrite` 的确切边界

`src/hooks/rewrite_cmd.rs` 的退出协议如下：

| 退出码 | stdout | 源码含义 |
| --- | --- | --- |
| `0` | 改写后的 `rtk ...` | 有映射，且 Claude 权限规则明确允许。 |
| `1` | 空 | 无 RTK 映射，或输入含不可证明的 Shell 构造。 |
| `2` | 空 | Claude 权限 deny 命中；不输出改写。 |
| `3` | 改写后的 `rtk ...` | 有映射，但 Claude 权限为 ask/default。 |

源码会拒绝或保留原样的典型输入包括：反引号命令替换、`$()`、`$((`、here-document、文件
重定向 `>` / `>>` / `<`，以及 Shell 多行控制结构。尾部 `2>&1` 可被保留并改写，但这属于
POSIX/Bash 语义，不要把它推广到 PowerShell 复合表达式。

rewrite 引擎还会识别 POSIX 的 `sudo`、`env`、`NAME=value` 前缀，内建透明包装词
`uv run`、`noglob`、`command`、`builtin`、`exec`、`nocorrect`，以及配置中的
`[hooks].transparent_prefixes`。这些是源码能力说明，不是要求在 Windows PowerShell 中模拟它们。

下列前缀/精确命令被 registry 明确忽略，不会得到 rewrite：`cd`、`echo`、`printf`、`export`、
`source`、`mkdir`、`rm`、`mv`、`cp`、`chmod`、`chown`、`touch`、`which`、`type`、`test`、
`true`、`false`、`sleep`、`wait`、`kill`、`set`、`unset`、`sort`、`uniq`、`tr`、`cut`、`awk`、
`sed`、`python3 -c`、`python -c`、`node -e`、`ruby -e`、`pwd`、`bash`、`sh` 及 Shell 控制词。

## 7. 静态 rewrite 注册表：89 条完整源码规则

下面的正则是 `src/discover/rules.rs` 的 `pattern` 原文；左侧是输入，右侧是该规则的
`rtk_cmd`。正则命中不自动表示专用过滤，需同时看第 5、8、9 节。

### Git、托管与文件

- `^(?:git|yadm)\s+(?:-[Cc]\s+\S+\s+)*(status|log|diff|show|add|commit|checkout|push|pull|branch|fetch|stash|worktree)` -> `rtk git`
- `^gh\s+(pr|issue|run|repo|api|release)` -> `rtk gh`
- `^glab\s+(mr|issue|ci|pipeline|api|release)` -> `rtk glab`
- `^gt\s+` -> `rtk gt`
- `^(cat|head|tail)\s+` -> `rtk read`
- `^grep\s+` -> `rtk grep`
- `^rg\s+` -> `rtk rg`
- `^ls(\s|$)` -> `rtk ls`
- `^find\s+` -> `rtk find`
- `^tree(\s|$)` -> `rtk tree`
- `^diff\s+` -> `rtk diff`
- `^wc(\s|$)` -> `rtk wc`

### JavaScript、TypeScript 与测试

- `^pnpm\s+(exec|i|install|list|ls|outdated|run|run-script)` -> `rtk pnpm`
- `^npm\s+(exec|run|run-script|rum|urn|x)(\s|$)` -> `rtk npm`
- `^npx\s+` -> `rtk npx`
- `^((p?np(m|x)|p?npm\s+(exec|run|run-script)|npm\s+(rum|urn|x)|pnpm\s+dlx)\s+)?tsc(\s|$)` -> `rtk tsc`
- `^((p?np(m|x)|p?npm\s+(exec|run|run-script)|npm\s+(rum|urn|x)|pnpm\s+dlx)\s+)?(biome|eslint|lint)(\s|$)` -> `rtk lint`
- `^((p?np(m|x)|p?npm\s+(exec|run|run-script)|npm\s+(rum|urn|x)|pnpm\s+dlx)\s+)?prettier` -> `rtk prettier`
- `^((p?np(m|x)|p?npm\s+(exec|run|run-script)|npm\s+(rum|urn|x)|pnpm\s+dlx)\s+)?next\s+build` -> `rtk next`
- `^((p?np(m|x)|p?npm\s+(exec|run|run-script)|npm\s+(rum|urn|x)|pnpm\s+dlx)\s+)?jest(\s+run)?(\s|$)` -> `rtk jest`
- `^((p?np(m|x)|p?npm\s+(exec|run|run-script)|npm\s+(rum|urn|x)|pnpm\s+dlx)\s+)?vitest(\s+run)?(\s|$)` -> `rtk vitest`
- `^ctest(?:\s|$)` -> `rtk ctest`
- `^((p?np(m|x)|p?npm\s+(exec|run|run-script)|npm\s+(rum|urn|x)|pnpm\s+dlx)\s+)?playwright` -> `rtk playwright`
- `^((p?np(m|x)|p?npm\s+(exec|run|run-script)|npm\s+(rum|urn|x)|pnpm\s+dlx)\s+)?prisma` -> `rtk prisma`

### Rust、Python、Go、.NET、JVM、Scala、Ruby 与 PHP

- `^cargo\s+(build|test|clippy|check|fmt|install)` -> `rtk cargo`
- `^(python3?\s+-m\s+)?mypy(\s|$)` -> `rtk mypy`
- `^ruff\s+(check|format)` -> `rtk ruff`
- `^(python[0-9.]*\s+-m\s+)?pytest(\s|$)` -> `rtk pytest`
- `^(pip3?|uv\s+pip)\s+(list|outdated|install|show)` -> `rtk pip`
- `^uv\s+run(?:\s|$)` -> `rtk uv`
- `^uv\s+(sync|pip\s+install)\b` -> `rtk uv`
- `^go\s+(test|build|vet)` -> `rtk go`
- `^(?:golangci-lint|golangci)\s+(run)(?:\s|$)` -> `rtk golangci-lint run`
- `^dotnet\s+build\b` -> `rtk dotnet`
- `^sbt\s+["']?(testOnly|testQuick|test|compile|run|clean|assembly|package)(?:[\s"']|$)` -> `rtk sbt`
- `^(?:\./gradlew|gradlew\.bat|gradlew|gradle)(?:\s+(test|build|clean|assemble\w*|install\w*|check|lint\w*|dependencies))?(\s|$)` -> `rtk gradlew`
- `^(?:\./mvnw|mvnw\.cmd|mvnw|mvn)\b(?:\s+\S+)*?\s+(compile|test|integration-test|package|install|verify|deploy)\b` -> `rtk mvn`
- `^(?:mvnd\.cmd|mvnd)\b(?:\s+\S+)*?\s+(compile|test|integration-test|package|install|verify|deploy)\b` -> `rtk mvnd`
- `^bundle\s+(install|update)\b` -> `rtk bundle`
- `^(?:bundle\s+exec\s+)?(?:bin/)?(?:rake|rails)\s+test` -> `rtk rake`
- `^(?:bundle\s+exec\s+)?rspec(?:\s|$)` -> `rtk rspec`
- `^(?:bundle\s+exec\s+)?rubocop(?:\s|$)` -> `rtk rubocop`
- `^php\s+artisan(?:\s|$)` -> `rtk php`
- `^php\s+-l(?:\s|$)` -> `rtk php`
- `^php\s+run-tests\.php(?:\s|$)` -> `rtk phpt`
- `^(?:php\s+)?(?:\./)?(?:(?:vendor/)?bin/)?phpunit(?:\s|$)` -> `rtk phpunit`
- `^(?:php\s+)?(?:\./)?(?:(?:vendor/)?bin/)?phpstan\s+analy[sz]e\b` -> `rtk phpstan`
- `^(?:\./)?(?:vendor/bin/)?pest(?:\s|$)` -> `rtk pest`
- `^(?:\./)?(?:vendor/bin/)?paratest(?:\s|$)` -> `rtk paratest`
- `^(?:\./)?(?:vendor/bin/)?ecs(?:\s|$)` -> `rtk ecs`
- `^(?:\./)?(?:vendor/bin/)?pint(?:\s|$)` -> `rtk pint`
- `^mix\s+(compile|format)(\s|$)` -> `rtk mix`
- `^pio\s+run` -> `rtk pio`
- `^poetry\s+(install|lock|update)\b` -> `rtk poetry`
- `^swift\s+(build|test)\b` -> `rtk swift`
- `^trunk\s+build` -> `rtk trunk`

### 容器、云、基础设施、网络和系统

- `^docker\s+(ps|images|logs|run|exec|build|compose\s+(ps|logs|build))` -> `rtk docker`
- `^kubectl\s+(get|logs|describe|apply)` -> `rtk kubectl`
- `^oc\s+(get|logs|describe|apply|status|adm)` -> `rtk oc`
- `^curl\s+` -> `rtk curl`
- `^wget\s+` -> `rtk wget`
- `^aws\s+` -> `rtk aws`
- `^psql(\s|$)` -> `rtk psql`
- `^ansible-playbook\b` -> `rtk ansible-playbook`
- `^brew\s+(install|upgrade)\b` -> `rtk brew`
- `^composer\s+(install|update|require)\b` -> `rtk composer`
- `^df(\s|$)` -> `rtk df`
- `^du\b` -> `rtk du`
- `^fail2ban-client\b` -> `rtk fail2ban-client`
- `^gcloud\b` -> `rtk gcloud`
- `^hadolint\b` -> `rtk hadolint`
- `^helm\b` -> `rtk helm`
- `^iptables\b` -> `rtk iptables`
- `^liquibase(?:\s|$)` -> `rtk liquibase`
- `^make\b` -> `rtk make`
- `^markdownlint\b` -> `rtk markdownlint`
- `^ping\b` -> `rtk ping`
- `^pre-commit\b` -> `rtk pre-commit`
- `^ps(\s|$)` -> `rtk ps`
- `^pulumi\s+(preview|up|destroy|refresh|stack)(\s|$)` -> `rtk pulumi`
- `^quarto\s+render` -> `rtk quarto`
- `^rsync\b` -> `rtk rsync`
- `^shellcheck\b` -> `rtk shellcheck`
- `^shopify\s+theme\s+(push|pull)` -> `rtk shopify`
- `^sops\b` -> `rtk sops`
- `^systemctl\s+status\b` -> `rtk systemctl`
- `^terraform\s+plan` -> `rtk terraform`
- `^tofu\s+(fmt|init|plan|validate)(\s|$)` -> `rtk tofu`
- `^yamllint\b` -> `rtk yamllint`

## 8. 内置 TOML fallback：63 个完整过滤器

以下是 `src/filters/*.toml` 的全部内置 `match_command` 字面值。它们没有都出现在
`rtk --help` 中，但源码的 fallback 和 rewrite 引擎会在严格命中时使用它们。`rtk <名称>`
只应在下面的精确模式内使用；例如 `rtk basedpyright ...` 有依据，`rtk list ...` 没有。

- `ansible-playbook.toml` / `ansible-playbook`：`^ansible-playbook\\b`
- `basedpyright.toml` / `basedpyright`：`^basedpyright\\b`
- `biome.toml` / `biome`：`^biome\\b`
- `brew-install.toml` / `brew-install`：`^brew\\s+(install|upgrade)\\b`
- `bundle-install.toml` / `bundle-install`：`^bundle\\s+(install|update)\\b`
- `composer-install.toml` / `composer-install`：`^composer\\s+(install|update|require)\\b`
- `df.toml` / `df`：`^df(\\s|$)`
- `dotnet-build.toml` / `dotnet-build`：`^dotnet\\s+build\\b`
- `du.toml` / `du`：`^du\\b`
- `fail2ban-client.toml` / `fail2ban-client`：`^fail2ban-client\\b`
- `gcc.toml` / `gcc`：`^g(cc|\\+\\+)\\b`
- `gcloud.toml` / `gcloud`：`^gcloud\\b`
- `gradle.toml` / `gradle`：`^(gradle|gradlew|\\./)gradlew?\\b`
- `hadolint.toml` / `hadolint`：`^hadolint\\b`
- `helm.toml` / `helm`：`^helm\\b`
- `iptables.toml` / `iptables`：`^iptables\\b`
- `jira.toml` / `jira`：`^jira\\b`
- `jj.toml` / `jj`：`^jj\\b`
- `jq.toml` / `jq`：`^jq\\b`
- `just.toml` / `just`：`^just\\b`
- `liquibase.toml` / `liquibase`：`^liquibase(?:\\s|$)`
- `make.toml` / `make`：`^make\\b`
- `markdownlint.toml` / `markdownlint`：`^markdownlint\\b`
- `mise.toml` / `mise`：`^mise\\s+(run|exec|install|upgrade)\\b`
- `mix-compile.toml` / `mix-compile`：`^mix\\s+compile(\\s|$)`
- `mix-format.toml` / `mix-format`：`^mix\\s+format(\\s|$)`
- `nx.toml` / `nx`：`^(pnpm\\s+)?nx\\b`
- `ollama.toml` / `ollama`：`^ollama\\s+run\\b`
- `oxlint.toml` / `oxlint`：`^oxlint\\b`
- `ping.toml` / `ping`：`^ping\\b`
- `pio-run.toml` / `pio-run`：`^pio\\s+run`
- `poetry-install.toml` / `poetry-install`：`^poetry\\s+(install|lock|update)\\b`
- `pre-commit.toml` / `pre-commit`：`^pre-commit\\b`
- `ps.toml` / `ps`：`^ps(\\s|$)`
- `pulumi-destroy.toml` / `pulumi-destroy`：`^pulumi\\s+destroy(\\s|$)`
- `pulumi-preview.toml` / `pulumi-preview`：`^pulumi\\s+preview(\\s|$)`
- `pulumi-refresh.toml` / `pulumi-refresh`：`^pulumi\\s+refresh(\\s|$)`
- `pulumi-stack.toml` / `pulumi-stack`：`^pulumi\\s+stack(\\s+(ls|output|history|select|init|rm|rename|tag|unselect|change-secrets-provider)\\b|\\s*$)`
- `pulumi-up.toml` / `pulumi-up`：`^pulumi\\s+up(\\s|$)`
- `quarto-render.toml` / `quarto-render`：`^quarto\\s+render`
- `rsync.toml` / `rsync`：`^rsync\\b`
- `shellcheck.toml` / `shellcheck`：`^shellcheck\\b`
- `shopify-theme.toml` / `shopify-theme`：`^shopify\\s+theme\\s+(push|pull)`
- `skopeo.toml` / `skopeo`：`^skopeo\\b`
- `sops.toml` / `sops`：`^sops\\b`
- `spring-boot.toml` / `spring-boot`：`^(mvn\\s+spring-boot:run|java\\s+-jar\\s+(?:\\S*[/\\\\])?[^/\\\\\\s]*(?i:spring)[^/\\\\\\s]*\\.jar|gradle\\s+.*bootRun)`
- `ssh.toml` / `ssh`：`^ssh(?:\\s|$)`
- `stat.toml` / `stat`：`^stat\\b`
- `swift-build.toml` / `swift-build`：`^swift\\s+build\\b`
- `systemctl-status.toml` / `systemctl-status`：`^systemctl\\s+status\\b`
- `task.toml` / `task`：`^task\\b`
- `terraform-plan.toml` / `terraform-plan`：`^terraform\\s+plan`
- `tofu-fmt.toml` / `tofu-fmt`：`^tofu\\s+fmt(\\s|$)`
- `tofu-init.toml` / `tofu-init`：`^tofu\\s+init(\\s|$)`
- `tofu-plan.toml` / `tofu-plan`：`^tofu\\s+plan(\\s|$)`
- `tofu-validate.toml` / `tofu-validate`：`^tofu\\s+validate(\\s|$)`
- `trunk-build.toml` / `trunk-build`：`^trunk\\s+build`
- `turbo.toml` / `turbo`：`^turbo\\b`
- `ty.toml` / `ty`：`^ty\\b`
- `uv-sync.toml` / `uv-sync`：`^uv\\s+(sync|pip\\s+install)\\b`
- `xcodebuild.toml` / `xcodebuild`：`^xcodebuild\\b`
- `yadm.toml` / `yadm`：`^yadm\\b`
- `yamllint.toml` / `yamllint`：`^yamllint\\b`

TOML 查找优先级是：项目 `.rtk/filters.toml`，用户 `~/.config/rtk/filters.toml`，内置过滤器，
最后才是原样 fallback。项目/用户自定义过滤器需要 `rtk trust` 信任；编辑已信任文件后需重新
信任。`RTK_NO_TOML=1` 会跳过 TOML 过滤器。

## 9. 源码中“有映射”但不能误解为“专用过滤”的情况

| 输入或输出 | 实际源码行为 | Codex 规则 |
| --- | --- | --- |
| `cargo fmt` -> `rtk cargo fmt` | `cargo fmt` 落入 `CargoCommands::Other`，原样透传。 | 可原生执行；不要把它描述为压缩过滤。 |
| `docker run` / `docker exec` / `docker build` -> `rtk docker ...` | 仅 `ps`、`images`、`logs`、特定 compose 分支专用；这些命令走 docker 透传。 | 不能宣称已压缩；保留原生命令也可。 |
| `kubectl describe` / `kubectl apply` -> `rtk kubectl ...` | formal `Other` 透传。 | 不能宣称已压缩。 |
| `oc describe` / `oc apply` / `oc status` / `oc adm` -> `rtk oc ...` | formal `Other` 透传。 | 不能宣称已压缩。 |
| `pnpm exec` / `pnpm run` / `pnpm run-script` -> `rtk pnpm ...` | `pnpm` 只有 list/outdated/install/typecheck 专用；其余透传。 | 不要把所有 pnpm 说成专用过滤。 |
| `swift test` -> `rtk swift test` | 有 static rule，但无正式 `swift` 子命令、也没有 `swift test` TOML filter；最终会原样 fallback。 | 使用原生 `swift test`，不要期待 RTK 压缩。 |
| `yadm status` -> `rtk git status` | 规则把 yadm 路由到 git；另有 `yadm.toml` 可用 fallback。 | 需要保持 yadm 二进制语义时不要机械改成 git。 |
| `npx prisma db push` -> `rtk prisma db push` | 正式 Prisma 语法是 `rtk prisma db-push`；空格形式会错过正式 parser 并 fallback。 | 直接使用 `rtk prisma db-push`。 |
| `prisma migrate reset` -> `rtk prisma migrate reset` | 正式 migrate 仅有 `dev`、`status`、`deploy`。 | `reset` 这类未列出的 Prisma 子命令使用原生命令或 `rtk npx prisma reset` 的透传路径，不把 rewrite 输出当语法保证。 |
| `gh ... --json ...`、`gh ... --jq ...`、`gh ... --template ...` | rewrite 在不区分大小写的参数文本中发现这三个片段之一时，明确返回无映射，避免破坏结构化输出。 | 直接使用原生 `gh`；不要强行改为 `rtk gh`。 |
| `dotnet test` | 正式 `rtk dotnet test` 有专用处理，但 static rewrite 只列 `dotnet build`。 | “正式支持”和“rewrite 自动覆盖”是两套表；可直接用正式语法。 |
| `rtk git rebase`、`rtk go mod` 等 | 正式外部子命令可解析为透传，但 registry 未承诺自动映射。 | 只有需要 RTK 统一入口时才使用；不要声称得到输出压缩。 |

## 10. Windows PowerShell、CMD、POSIX 与底层依赖

### Windows PowerShell

- `Get-ChildItem`、`Get-Content`、`Select-String`、`Test-Path`、`Set-Location`、`New-Item`、
  `Remove-Item`、`Copy-Item`、`Move-Item`、`Start-Process` 等是 PowerShell cmdlet，直接原生执行。
- 包含 `$变量`、`$()`、`` ` ``、`;`、`|`、`>`、`>>`、`2>`、脚本块、hashtable 或 cmdlet 的表达式
  由 PowerShell 原生解释。不要用 `rtk run` 取代 PowerShell。
- 指定 UTF-8 文本范围的正确形式示例：

  ```powershell
  Get-Content -LiteralPath .\websocket.go -Encoding utf8 |
    Select-Object -Skip 900 -First 250
  ```

- `curl` / `wget` 在 PowerShell 中可能解析为别名；只有目标确为真实 `curl.exe` / `wget.exe`
  时才使用相应 RTK 包装。不要将 `Invoke-WebRequest` 改为 `rtk wget`。
- `rtk run` 在 Windows 使用 `cmd /C`，不能解释 PowerShell cmdlet。

### Windows 依赖与 POSIX 工具

- `rtk ls`、`rtk tree`、`rtk grep`、`rtk rg`、`rtk find`、`rtk wc` 依赖相应真实底层程序。
  PowerShell 别名不提供 `ls.exe`、`grep.exe`、`wc.exe`、`find.exe` 的等价语义。
- `rtk tree` 的参数必须符合实际 `tree` 二进制；Windows `tree.com` 常用 `/F`、`/A`，不要照搬
  Unix `-L`、`-d`、`-a`。
- Linux、macOS、Git Bash、WSL 的 POSIX 命令与源码 regex 更贴近；这不改变 Codex 无 Hook 的事实。

## 11. 自检与维护

升级 RTK 后，不要复制旧表格结论。按以下顺序更新本文件：

```powershell
rtk --version
rtk --help
rtk grep --help
rtk read --help
rtk prisma --help
rtk rewrite 'git status'
rtk rewrite 'npm.cmd run build'
rtk rewrite 'basedpyright .'
```

核对原则：

1. `rtk --help` 对应正式 CLI 表。
2. `rtk rewrite` 对应静态规则和 TOML 匹配表。
3. `rtk <子命令> --help` 对应参数位置和嵌套语法。
4. 底层程序能否运行、PowerShell 是否把名称当别名、以及本项目的 `npm.cmd` 规则，必须单独确认。
5. 新增或删除正式子命令、rewrite 规则、TOML 文件、Windows fallback 行为时，同步更新本文件；不要用
   “通常”“可能”“或许”代替源码中的明确清单。
