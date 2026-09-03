# RTK：Codex 常驻高密度命令规则

> 本文件是 Codex 的常驻命令决策表，不是 RTK 安装教程。它从同目录
> `RTK-Codex-commands.md` 的完整源码审计参考提炼而来，保留决定“是否使用 RTK、使用哪个
> 子命令、参数应放在哪里、何时必须原生执行”的规则。
>
> 不要假定完整参考会自动进入上下文。本文件已覆盖常见和可判定的命令选择；只有遇到未列参数、
> 罕见工具、版本升级或本文件与实际 CLI 不一致时，才定位读取完整参考并以
> `rtk --help`、`rtk <正式子命令> --help`、`rtk rewrite` 的运行时结果为准。

## 0. 文本编码与换行

以下规则适用于本任务中读取或修改的所有文本文件（代码、配置、Markdown、README、脚本等；
二进制文件不直接改写）：

1. 全程按 UTF-8 读取和写入，不依赖系统默认代码页、ANSI 或不明编码。
2. 写入必须是 UTF-8 无 BOM，且只使用 LF（`\n`）换行。
3. 不得引入乱码、Unicode 替换字符 `U+FFFD`、NUL 字节或不可读的反斜杠、正则、反引号。
4. 已确认编码且只做小范围 `apply_patch` 时可沿用原格式；新建、重建、转码、批量改写、格式化/生成
   写入、原编码未知或发现异常时，严格验证 UTF-8、无 BOM、无 `U+FFFD`、无 NUL、仅 LF。
5. 解码或校验失败时停止覆盖并报告；不得猜测编码或以批量重写掩盖问题。

## 1. 基础事实与执行顺序

1. Codex 没有 RTK 自动改写 Hook。`rtk hook codex` 不存在；`rtk init --codex` 只写
   `AGENTS.md`/`RTK.md` 规则，不会拦截 Codex 工具调用。
2. `apply_patch` 是 Codex 原生补丁入口，不是 RTK 命令，不能加 `rtk` 前缀。
3. RTK 有四层：正式 CLI、专用输出处理器、严格匹配的 TOML fallback、原样 fallback。未知的
   `rtk <名称>` 可能直接启动同名外部程序，不等于 RTK 支持；绝不能给任意命令机械加 `rtk`。
4. PowerShell cmdlet/表达式、变量、`$()`、反引号、管道、重定向、分号组合、脚本块、hashtable、
   文件写入流程、交互会话、用户明确指定的原生命令，直接由 PowerShell 或原程序执行；不先跑
   `rtk rewrite`，也不用 `rtk run`。
5. 本项目的 `npm.cmd ...`（尤其 `npm.cmd run build` 与 `npm.cmd run <脚本>`）必须原生执行；
   `npm.cmd` 不匹配 RTK 的 `^npm\s+` 规则。`gofmt` 也直接执行。
6. 对于本表明确列出的正式语法，直接使用对应 `rtk` 命令；对严格命中第 4、5 节且没有第 6 节陷阱的
   单一外部命令，可使用映射后的形式；其余命令原生执行或最多探测一次 `rtk rewrite '<原命令>'`。
7. `rtk rewrite` 只给候选映射，不验证正式参数、专用处理器、底层二进制或 Windows Shell 语义。
   退出码：`0`=允许映射，`3`=ask/default 映射，`1`=无映射，`2`=Claude deny，其他异常不得猜测。
   含反引号、`$()`、`$((`、here-document、`>`/`>>`/`<`、多行 Shell 控制结构的输入不交给它。
8. 需要完整参考时，用 PowerShell 原生读取（不加 `rtk`）：

```powershell
$rtkExe = (Get-Command rtk -CommandType Application -ErrorAction Stop).Source
$rtkGuide = Join-Path (Split-Path -Parent $rtkExe) 'RTK-Codex-commands.md'
Get-Content -LiteralPath $rtkGuide -Encoding utf8
```

## 2. 正式 CLI 完整索引与精确语法

全局形式：`rtk [-v|-vv|-vvv] [--ultra-compact] [--skip-env] <子命令> ...`。全局选项必须在
顶层子命令之前。正式顶层命令完整索引为：

`ls`、`tree`、`read`、`smart`、`git`、`gh`、`glab`、`aws`、`psql`、`pnpm`、`err`、`test`、
`json`、`deps`、`env`、`find`、`diff`、`log`、`dotnet`、`docker`、`kubectl`、`oc`、`summary`、
`grep`、`rg`、`init`、`wget`、`wc`、`gain`、`cc-economics`、`config`、`jest`、`vitest`、`ctest`、
`prisma`、`tsc`、`next`、`lint`、`prettier`、`format`、`playwright`、`cargo`、`npm`、`npx`、`curl`、
`discover`、`session`、`telemetry`、`learn`、`run`、`proxy`、`pipe`、`trust`、`untrust`、`verify`、
`ruff`、`pytest`、`mypy`、`php`、`phpunit`、`phpstan`、`pest`、`paratest`、`ecs`、`pint`、`phpt`、
`rake`、`rubocop`、`rspec`、`pip`、`uv`、`go`、`sbt`、`gt`、`golangci-lint`、`gradlew`、`mvn`、`mvnd`、
`hook-audit`、`rewrite`、`hook`。

### 文件、文本、搜索和输出

- `rtk ls [<ls 参数>...]`、`rtk tree [<tree 参数>...]`、`rtk wc [<wc 参数>...]` 依赖真实底层程序。
  PowerShell 的 `ls` 不是 `ls.exe`；无真实程序时用 `Get-ChildItem`。Windows `tree.com` 用 `/F`、`/A`，
  不照搬 Unix `-L`、`-d`、`-a`。
- `rtk read <文件|->... [--level none|minimal|aggressive] [--max-lines N|--tail-lines N] [--line-numbers]`：
  至少一个文件；`-` 读 stdin；`--max-lines` 与 `--tail-lines` 互斥；短选项为 `-l`、`-m`、`-n`。
  没有 `--lines`、`--skip` 或任意行区间。任意 UTF-8 范围用
  `Get-Content -LiteralPath <文件> -Encoding utf8 | Select-Object -Skip N -First N`。
- `rtk smart <文件> [--model heuristic] [--force-download]` 只接收一个文件；
  `rtk json <JSON 文件|-> [--depth N] [--keys-only]` 的 `-` 是 stdin，默认深度 `5`；
  `rtk deps [路径]` 默认 `.`；`rtk env [-f|--filter <名称>]` 没有位置参数。
- `rtk find [find 参数...]`；legacy 形式为 `rtk find <pattern> [path] [-m|--max N] [-t|--file-type f|d]`。
  `--file-type` 只属于 `find`，不属于 `grep`。
- `rtk diff <文件1> <文件2>` 只用于两个文件；只给一个位置参数时源码读 stdin 并忽略该参数。
  Git diff 必须是 `rtk git diff ...`。`rtk log [日志文件]` 无参数读 stdin，不执行 `log` 程序。
- `rtk grep [--max-len N] [--max N] [--context-only] <原生 grep 参数...>`：默认 `80`、`200`；这三项
  RTK 选项必须排在第一个原生 grep/rg 参数之前，`-l`/`-m` 是底层 grep 参数。没有 `--file-type`；
  按类型用 `rtk rg -t <type> <pattern> <path>`。`rtk rg <原生 rg 参数...>` 使用原生 `-t/--type`、`--glob`。
- `rtk err <程序> [参数...]`、`rtk test <程序> [参数...]`、`rtk summary <程序> [参数...]` 分别突出错误、失败、
  或摘要。`rtk pipe [--filter <名称>] [--passthrough]` 读 stdin；filter 只能是
  `cargo-test`、`cargo`、`pytest`、`go-test`、`go-build`、`ctest`、`tsc`、`vitest`、`grep`、`rg`、`find`、
  `fd`、`git-log`、`git-diff`、`git-status`、`log`、`mypy`、`ruff-check`、`ruff-format`、`prettier`、
  `phpunit`、`pest`、`paratest`、`php-test`、`ecs`、`phpstan`、`pint`。
- `cat`/`head`/`tail` 不能概括为任意改写：`cat <文件...>` 与 `cat -n <文件...>` 可分别改为
  `rtk read`、`rtk read -n`；其它以 `-` 开头的 `cat` 参数原生执行。`head -<N> <单文件>`、
  `head --lines=<N> <单文件>` 可改为 `rtk read <文件> --max-lines N`；`head -n N`、`--lines N`、
  `-c`、多文件不改。普通 `head <文件>` 会变成全文件读取，若要原生默认十行，使用原生 `head` 或
  `rtk read <文件> --max-lines 10`。`tail -<N>`、`tail -n <N>`、`tail --lines=<N>`、
  `tail --lines <N>` 且单文件可改为 `--tail-lines N`；普通/`-c`/多文件 tail 不改。

### Git、托管与 Node.js

- `rtk git [-C <path>]... [-c <key=value>]... [--git-dir <dir>] [--work-tree <dir>] [--no-pager]
  [--no-optional-locks] [--bare] [--literal-pathspecs] <子命令> [参数...]`。`diff`、`log`、`status`、`show`、
  `add`、`commit`、`checkout`、`push`、`pull`、`branch`、`fetch`、`stash`、`worktree` 有分支，其他 Git
  子命令透传；写操作仍有副作用。
- `rtk gh <pr|issue|run|repo|api|release|其它> [参数...]`；`rtk glab [-R|--repo <owner/repo>]
  [-g|--group <group>] <mr|issue|ci|pipeline|api|release|其它> [参数...]`；
  `rtk gt <log|submit|sync|restack|create|branch|其它> [参数...]`。未列子命令不承诺专用过滤。
- `rtk npm [npm 参数...]` 会把第一个未知词当脚本并注入 `npm run`；明确 `run` 和首个选项不注入。
  不注入的完整首词是 `install,i,ci,uninstall,remove,rm,update,up,list,ls,outdated,init,create,publish,pack,link,`
  `audit,fund,exec,explain,why,search,view,info,show,config,set,get,cache,prune,dedupe,doctor,help,version,prefix,`
  `root,bin,bugs,docs,home,repo,ping,whoami,token,profile,team,access,owner,deprecate,dist-tag,star,stars,login,`
  `logout,adduser,unpublish,pkg,diff,rebuild,test,t,start,stop,restart`。但本项目 `npm.cmd` 始终原生。
- `rtk npx <命令> [参数...]` 会路由 `tsc/typescript`、`eslint`、`prisma generate`、`prisma db push`、
  `next`、`prettier`、`playwright`；其他 npx 走 npx 输出过滤。`rtk pnpm [-F|--filter <包>]... <子命令>` 中
  `list`（可 `-d|--depth N`）、`outdated`、`install`、`typecheck` 专用，其余透传。
- `rtk jest`、`rtk vitest`、`rtk playwright`、`rtk tsc`、`rtk next`、`rtk prettier`（`--write` 仍写文件）
  有相应处理；`rtk lint` 是 ESLint/Biome/lint 入口，不存在 `rtk eslint`；`rtk format` 只检测
  Prettier、Black、Ruff format，不是通用格式化器；`rtk ctest --help` 会传给 ctest。
- Prisma 正式形式仅为 `rtk prisma generate`、`rtk prisma migrate dev [-n|--name <名称>]`、
  `rtk prisma migrate status`、`rtk prisma migrate deploy`、`rtk prisma db-push`。特别是
  `npx prisma db push` 不能照 rewrite 输出写成 `rtk prisma db push`；必须是连字符 `db-push`。

### Python、编译生态、PHP/Ruby

- `rtk pytest [参数...]`、`rtk ruff [参数...]`、`rtk mypy [参数...]`；`rtk pip [参数...]` 可识别 uv，
  自动改写只覆盖 `pip/pip3/uv pip` 的 `list|outdated|install|show`；`rtk uv [参数...]` 自动覆盖
  `uv run`、`uv sync`、`uv pip install`。
- `rtk cargo <build|test|clippy|check|install|nextest|其它> [参数...]`：前六项专用，`fmt`、`run` 等透传。
  `rtk go <test|build|vet|其它> [参数...]`：前三项专用，`go fmt/mod/run` 透传；`gofmt` 原生。
  `rtk golangci-lint [参数...]` 只有 `run` 专用；`rtk dotnet <build|test|restore|format|其它>` 前四项专用；
  `rtk sbt <test|compile|run|其它>` 前三项专用。
- `rtk gradlew [任务/参数...]` 优先 `./gradlew.bat`（Windows）或 `./gradlew`；
  `rtk mvn [goal/参数...]` 优先 `./mvnw.cmd`（Windows）或 `./mvnw`，否则 `mvn`；
  `test,integration-test,compile,test-compile,package,install,verify,deploy` 有过滤，`clean/site/插件/help` 透传；
  `rtk mvnd` 同 Maven 阶段。`gcc`、`g++`、`xcodebuild`、`swift build` 不是正式顶层命令，只能按 fallback。
- `rtk php`（`php artisan`、`php -l`）、`rtk phpunit`、`rtk phpstan`、`rtk pest`、`rtk paratest`、
  `rtk ecs`、`rtk pint`、`rtk phpt`，以及 `rtk rake`、`rtk rubocop`、`rtk rspec` 是正式入口；
  `bundle install|update` 仅是 fallback，不是正式顶层 `bundle`。

### 容器、网络、管理

- 专用 Docker 形式：`rtk docker ps [-a|--all]`、`rtk docker images`、`rtk docker logs <container>`、
  `rtk docker compose ps [-a|--all]`、`rtk docker compose logs [service] [--tail N]`、
  `rtk docker compose build [service]`。其它 Docker 子命令透传；`docker logs` 带额外选项不保证专用。
- Kubernetes/OC 专用：`rtk kubectl get [参数...]`、`pods/services [-n|--namespace <名称>] [-A|--all]`、
  `logs <pod> [-c|--container <名称>]`；`rtk oc` 对应 `get/pods/services/logs`。其余子命令透传。
- `rtk aws <服务> [参数...]`；`rtk psql [参数...]`（交互、密码提示或必须保留完整表格时原生）；
  `rtk curl [参数...]`、`rtk wget [-O|--output-document <文件|->] <URL> [参数...]` 依赖真实二进制。
- `rtk init [-g|--global] ... [--codex] [--copilot] [--dry-run]` 的 `--codex` 不能与 `--opencode`、
  `--claude-md`、`--hook-only`、`--auto-patch`、`--no-patch` 组合。`rtk gain ... --reset --yes`、
  `rtk config --create`、`rtk learn --write-rules`、`rtk telemetry forget` 都会写入/删除状态；
  `telemetry enable` 需要交互终端。
- `rtk discover [--project <路径片段>] [--limit N] [--all] [--since 天数] [--format text|json]`、
  `rtk session`、`rtk cc-economics [--daily|--weekly|--monthly|--all] [--format text|json|csv]`、
  `rtk trust [--list] [-y|--yes]`、`rtk untrust`、`rtk verify [--filter <名称>] [--require-all]`、
  `rtk hook-audit [--since 天数]`（需 `RTK_HOOK_AUDIT=1`）按正式帮助执行。
- `rtk run [-c|--command <命令字符串>] [参数...]` 无过滤/追踪；Windows 用 `cmd /C`，不是 PowerShell。
  `rtk proxy <程序> [参数...]` 不经 Shell、保留原始 stdout/stderr；仅在确需时显式 `pwsh -Command`。
  `rtk hook <claude|cursor|gemini|copilot|droid|vibe>` 没有 Codex；`rtk hook check [--agent <任意>] ...`
  只预览 rewrite，不能证明 Codex Hook 存在。

## 3. 高风险语法纠错

- 不存在 `rtk list`；目录查看是 `rtk ls ...`，或者 Windows PowerShell `Get-ChildItem`。
- 不存在 `rtk read --lines`、`rtk read --skip`、`rtk grep --file-type`、通用任意位置的 `--max`。
- `rtk grep --max 50 --max-len 120 -n 'pattern' file` 中 RTK 选项必须最前；类型搜索是
  `rtk rg -t rust 'pattern' .`。
- `rtk diff` 不是 `git diff`；使用 `rtk git diff --stat`。`rtk json - --depth N` 支持 stdin。
- `rtk hook check --agent codex` 不能创建或验证 Codex Hook；`rtk hook codex` 不存在。
- PowerShell 的 `curl`/`wget` 可为别名；只有确定目标是 `curl.exe`/`wget.exe` 才可按 RTK 规则包装，
  不得把 `Invoke-WebRequest` 改为 `rtk wget`。

## 4. 静态 rewrite 决策表

下列是“原命令严格命中时可获得候选 RTK 映射”的完整类别；仍须遵守第 1、6 节。括号表示允许的
首个子命令或条件，未列参数不能据此外推。

- 文件与托管：`git|yadm` 的 `status|log|diff|show|add|commit|checkout|push|pull|branch|fetch|stash|worktree`
  -> `rtk git`；`gh (pr|issue|run|repo|api|release)` -> `rtk gh`；
  `glab (mr|issue|ci|pipeline|api|release)` -> `rtk glab`；`gt ...` -> `rtk gt`；
  `cat|head|tail ...` -> `rtk read`（仅第 2 节精确限制）；`grep` -> `rtk grep`；`rg` -> `rtk rg`；
  `ls` -> `rtk ls`；`find` -> `rtk find`；`tree` -> `rtk tree`；`diff` -> `rtk diff`；`wc` -> `rtk wc`。
- JavaScript：`pnpm (exec|i|install|list|ls|outdated|run|run-script)` -> `rtk pnpm`；
  `npm (exec|run|run-script|rum|urn|x)` -> `rtk npm`；`npx ...` -> `rtk npx`；
  前缀可为 `npm|npx|pnpm|pnpx`，或 `npm|pnpm exec|run|run-script`、`npm rum|urn|x`、`pnpm dlx` 的
  `tsc` -> `rtk tsc`、`biome|eslint|lint` -> `rtk lint`、`prettier` -> `rtk prettier`、
  `next build` -> `rtk next`、`jest [run]` -> `rtk jest`、`vitest [run]` -> `rtk vitest`、
  `playwright` -> `rtk playwright`、`prisma` -> `rtk prisma`；`ctest` -> `rtk ctest`。
- Python/编译：`cargo (build|test|clippy|check|fmt|install)` -> `rtk cargo`；
  `python -m mypy|mypy` -> `rtk mypy`；`ruff (check|format)` -> `rtk ruff`；
  `python -m pytest|pytest` -> `rtk pytest`；`pip|pip3|uv pip (list|outdated|install|show)` -> `rtk pip`；
  `uv run|uv sync|uv pip install` -> `rtk uv`；`go (test|build|vet)` -> `rtk go`；
  `golangci-lint|golangci run` -> `rtk golangci-lint run`；`dotnet build` -> `rtk dotnet`；
  `sbt (testOnly|testQuick|test|compile|run|clean|assembly|package)` -> `rtk sbt`；
  `gradlew|gradle` 的 `test|build|clean|assemble*|install*|check|lint*|dependencies` -> `rtk gradlew`；
  `mvn|mvnw` 含 `compile|test|integration-test|package|install|verify|deploy` -> `rtk mvn`；
  `mvnd` 同阶段 -> `rtk mvnd`；`bundle install|update` -> `rtk bundle`；
  Rails/rake test -> `rtk rake`；`rspec` -> `rtk rspec`；`rubocop` -> `rtk rubocop`。
- PHP 与系统：`php artisan`、`php -l` -> `rtk php`；`php run-tests.php` -> `rtk phpt`；
  `phpunit`、`phpstan analyse/analyze`、`pest`、`paratest`、`ecs`、`pint` 分别映射同名 RTK 入口；
  `mix compile|format`、`pio run`、`poetry install|lock|update`、`swift build|test`、`trunk build` 分别有候选。
- 容器/云：`docker (ps|images|logs|run|exec|build|compose ps|logs|build)` -> `rtk docker`；
  `kubectl (get|logs|describe|apply)`、`oc (get|logs|describe|apply|status|adm)`、`curl`、`wget`、`aws`、`psql`
  有候选。`ansible-playbook`、`brew install|upgrade`、`composer install|update|require`、`df`、`du`、
  `fail2ban-client`、`gcloud`、`hadolint`、`helm`、`iptables`、`liquibase`、`make`、`markdownlint`、
  `ping`、`pre-commit`、`ps`、`pulumi preview|up|destroy|refresh|stack`、`quarto render`、`rsync`、
  `shellcheck`、`shopify theme push|pull`、`sops`、`systemctl status`、`terraform plan`、
  `tofu fmt|init|plan|validate`、`yamllint` 也有候选。

### 89 条原始静态模式

以下正则来自完整参考；命中只代表候选映射，仍须遵守第 1、6 节：

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

## 5. TOML fallback 精确边界

下列不是正式 CLI；仅当原始命令严格符合右侧条件时，RTK fallback/rewrite 才可能处理。可用的
`rtk <原可执行名> ...` 仅限这些条件，不能从名称推广到其他参数或工具。自定义过滤器优先级为
项目 `.rtk/filters.toml`、用户 `~/.config/rtk/filters.toml`、内置、原样 fallback；自定义文件须
`rtk trust`，编辑后重信任；`RTK_NO_TOML=1` 禁用 TOML。

- 任意参数：`ansible-playbook`、`basedpyright`、`biome`、`df`、`du`、`fail2ban-client`、`gcc|g++`、
  `gcloud`、`hadolint`、`helm`、`iptables`、`jira`、`jj`、`jq`、`just`、`make`、`markdownlint`、
  `oxlint`、`ping`、`pre-commit`、`ps`、`rsync`、`shellcheck`、`skopeo`、`sops`、`stat`、`task`、
  `turbo`、`ty`、`xcodebuild`、`yadm`、`yamllint`。
- 受限子命令：`brew install|upgrade`；`bundle install|update`；`composer install|update|require`；
  `dotnet build`；`gradle|gradlew|./gradlew`；`liquibase`；`mise run|exec|install|upgrade`；
  `mix compile`、`mix format`；`pnpm nx|nx`；`ollama run`；`pio run`；`poetry install|lock|update`；
  `pulumi destroy|preview|refresh|up`，以及 `pulumi stack` 后仅 `ls|output|history|select|init|rm|rename|tag|unselect|change-secrets-provider`
  或无后续词；`quarto render`；`shopify theme push|pull`；`swift build`；`systemctl status`；
  `terraform plan`；`tofu fmt|init|plan|validate`；`trunk build`；`uv sync|uv pip install`。
- Spring Boot fallback 只匹配 `mvn spring-boot:run`、包含 `spring` 的 `java -jar ...jar`，或
  `gradle ...bootRun`；`ssh` 只匹配 `ssh` 加空白/结尾。

### 63 条原始 fallback 模式

名称左侧是内置过滤器名；正则是精确 `match_command`，不要删除条件后推广使用：

- `ansible-playbook`：`^ansible-playbook\b`
- `basedpyright`：`^basedpyright\b`
- `biome`：`^biome\b`
- `brew-install`：`^brew\s+(install|upgrade)\b`
- `bundle-install`：`^bundle\s+(install|update)\b`
- `composer-install`：`^composer\s+(install|update|require)\b`
- `df`：`^df(\s|$)`
- `dotnet-build`：`^dotnet\s+build\b`
- `du`：`^du\b`
- `fail2ban-client`：`^fail2ban-client\b`
- `gcc`：`^g(cc|\+\+)\b`
- `gcloud`：`^gcloud\b`
- `gradle`：`^(gradle|gradlew|\./)gradlew?\b`
- `hadolint`：`^hadolint\b`
- `helm`：`^helm\b`
- `iptables`：`^iptables\b`
- `jira`：`^jira\b`
- `jj`：`^jj\b`
- `jq`：`^jq\b`
- `just`：`^just\b`
- `liquibase`：`^liquibase(?:\s|$)`
- `make`：`^make\b`
- `markdownlint`：`^markdownlint\b`
- `mise`：`^mise\s+(run|exec|install|upgrade)\b`
- `mix-compile`：`^mix\s+compile(\s|$)`
- `mix-format`：`^mix\s+format(\s|$)`
- `nx`：`^(pnpm\s+)?nx\b`
- `ollama`：`^ollama\s+run\b`
- `oxlint`：`^oxlint\b`
- `ping`：`^ping\b`
- `pio-run`：`^pio\s+run`
- `poetry-install`：`^poetry\s+(install|lock|update)\b`
- `pre-commit`：`^pre-commit\b`
- `ps`：`^ps(\s|$)`
- `pulumi-destroy`：`^pulumi\s+destroy(\s|$)`
- `pulumi-preview`：`^pulumi\s+preview(\s|$)`
- `pulumi-refresh`：`^pulumi\s+refresh(\s|$)`
- `pulumi-stack`：`^pulumi\s+stack(\s+(ls|output|history|select|init|rm|rename|tag|unselect|change-secrets-provider)\b|\s*$)`
- `pulumi-up`：`^pulumi\s+up(\s|$)`
- `quarto-render`：`^quarto\s+render`
- `rsync`：`^rsync\b`
- `shellcheck`：`^shellcheck\b`
- `shopify-theme`：`^shopify\s+theme\s+(push|pull)`
- `skopeo`：`^skopeo\b`
- `sops`：`^sops\b`
- `spring-boot`：`^(mvn\s+spring-boot:run|java\s+-jar\s+(?:\S*[/\\])?[^/\\\s]*(?i:spring)[^/\\\s]*\.jar|gradle\s+.*bootRun)`
- `ssh`：`^ssh(?:\s|$)`
- `stat`：`^stat\b`
- `swift-build`：`^swift\s+build\b`
- `systemctl-status`：`^systemctl\s+status\b`
- `task`：`^task\b`
- `terraform-plan`：`^terraform\s+plan`
- `tofu-fmt`：`^tofu\s+fmt(\s|$)`
- `tofu-init`：`^tofu\s+init(\s|$)`
- `tofu-plan`：`^tofu\s+plan`
- `tofu-validate`：`^tofu\s+validate(\s|$)`
- `trunk-build`：`^trunk\s+build`
- `turbo`：`^turbo\b`
- `ty`：`^ty\b`
- `uv-sync`：`^uv\s+(sync|pip\s+install)\b`
- `xcodebuild`：`^xcodebuild\b`
- `yadm`：`^yadm\b`
- `yamllint`：`^yamllint\b`

## 6. 有映射仍需原生或谨慎的情况

- `cargo fmt`、`docker run|exec|build`、`kubectl describe|apply`、`oc describe|apply|status|adm`、
  `pnpm exec|run|run-script` 可被路由但属于透传，不要声称有专用压缩；原生命令通常更清楚。
- `swift test` 虽有静态 rewrite，却没有正式 `swift` 子命令或 `swift test` TOML 过滤器；原生执行。
- `yadm status` 映射到 `rtk git status` 会改变二进制语义；需保持 yadm 时原生执行。
- `prisma migrate reset` 未被正式 Prisma parser 覆盖；用原生命令或 `rtk npx prisma reset` 的透传路径，
  不把 rewrite 字符串当语法保证。
- 参数含 `--json`、`--jq`、`--template` 的 `gh` 明确不改写，保留原生结构化输出。
- `dotnet test` 正式支持但静态 rewrite 只列 `dotnet build`；“能直接写正式命令”与“会自动改写”是两件事。
- `rtk git rebase`、`rtk go mod` 等正式外部子命令可透传，但不承诺压缩或自动映射。

## 7. Windows、补丁与维护

- `Get-ChildItem`、`Get-Content`、`Select-String`、`Test-Path`、`Set-Location`、`New-Item`、
  `Remove-Item`、`Copy-Item`、`Move-Item`、`Start-Process` 等 PowerShell cmdlet 直接执行。
- `rtk ls/tree/grep/rg/find/wc` 依赖真实二进制；PowerShell 别名不提供等价语义。Windows 与
  Linux/macOS/Git Bash/WSL 的参数语义不同，不从 POSIX 正则反推 Windows 可用参数。
- 手工修改优先 `apply_patch`；补丁必须以精确 `*** End Patch` 结束。构建前后端使用项目指定的
  `npm.cmd`，不运行用户明确禁止的长测试。
- RTK 升级、源码审计范围、静态 rewrite、TOML 过滤器、Windows fallback 或项目构建约束改变时，先更新
  `RTK-Codex-commands.md`，再按本表逐项同步。运行时自检至少包括：

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
