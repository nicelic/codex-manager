code-Manager
============

code-Manager 是独立于 C:\EXEXX\newexe 的本地 Go + Vue 网关程序。双击
code-Manager.exe 后，它会启动本地 HTTP 服务，并自动使用 Windows 默认浏览器打开：

http://127.0.0.1:7780

Vue 页面用于查看本地服务、修改网关配置以及控制 llmtrim daemon；它已嵌入 EXE，不需要
WebView2，也不需要单独启动前端开发服务器。

程序使用 Windows 单实例互斥锁：若 7780 尚未监听，程序启动网关、显示托盘图标并打开浏览器；
若同一个 code-Manager 已运行，新的 EXE 不会创建第二个进程或托盘图标，只会等待现有服务就绪，
然后打开现有网页并退出。如果该端口被其他程序占用，程序会明确报错，避免误连到未知服务。

托盘图标交互：右键单击显示“打开网页”和“退出 code-Manager”菜单；左键单击不执行任何操作；
左键双击打开管理网页。


工作链路
--------

调用 API 时，链路如下：

Codex 或其他 OpenAI 兼容客户端
  -> http://<listen_address>/v1（以 config.yaml 的 listen_address 为准）
  -> code-Manager
  -> （llmtrim 运行时）显式 HTTP(S) 代理 http://127.0.0.1:43117
  -> llmtrim 识别真实上游主机、MITM、压缩并转发
  -> upstream_base_url 指定的真实上游 API

点击“启动代理”后，网关先开始 HTTP `/v1/` 监听并进入“上游连接中”状态，同时预热上游 HTTPS 连接。
连接中再次点击“停止代理”会立刻取消握手和等待中的 `/v1` 请求。llmtrim daemon 运行时，
code-Manager 仍构造真实上游 URL、路径、请求头、Authorization 和正文，但用标准 HTTP Transport
把请求显式交给 `http://127.0.0.1:43117` 作为正向代理；HTTPS 请求通过 CONNECT 建立隧道，由
llmtrim 完成 MITM、压缩和转发。此时不使用 `outbound_proxy`，也不假定请求继续走 H3。llmtrim
停止时才恢复 code-Manager 自己的基线客户端。未配置出站代理时，
H3/QUIC+TLS 与 H2/TCP+TLS 使用同一 IP 和端口并发握手：H3 先完成时立即选择 H3；H2 先完成时
最多再给 H3 600 ms，窗口内 H3 完成则选择 H3。单轮握手最长 10 秒；两者都失败后等待 1 秒重试，
最多 5 轮，仍失败则关闭本次专用监听并在页面显示“上游不可用”。握手只验证 TLS 和 HTTP/2/HTTP/3
会话可承载数据，不发送 API 请求，也不探测 API Key。

已建立的 H2/H3 物理连接按上游地址复用，每条最多承载 500 个同时活动的 HTTP 请求流；响应完成、
客户端取消或响应体关闭后立即释放该流名额。达到上限时下一条连接在后台建立；扩容期间会临时超额复用，
不会把不同请求或响应的数据合并，也不会等待扩容结果。每 20 秒执行 H2 PING 或 H3 QUIC keep-alive。连接达到 10 分钟后
进入待轮换状态，不再接受新流；已有 HTTP 流和 WS 会继续运行，直到活动流归零后才关闭旧连接，因此实际寿命可能略长于 10 分钟。

H2/H3 预热独立于 llmtrim 路由状态：点击“启动代理”始终执行并发 TLS 预热，不发送 API 请求，也不承诺任一上游
一定接受 WebSocket。每次直连 WS 承载实际是否可用，仍由上游 SETTINGS 与该次扩展 CONNECT/Upgrade 握手决定。

`listen_address` 只提供 HTTP `/v1/` 网关，不再监听或处理本地 SOCKS4/SOCKS5。

code-Manager 接收 `/v1/` 下的 GET、HEAD、POST；每条请求都会检查 llmtrim 状态。运行中时，三种方法
都使用专用的 llmtrim 正向代理客户端；停止时都使用既有的基线客户端。不存在“POST 到 43117 根路径
取得转换 JSON”的本地协议。

在 code-Manager 的 `/v1` 网关链路中，43117 daemon 是运行中的显式 HTTP(S) 代理。code-Manager 始终控制
真实上游 URL 和 Authorization 请求头；llmtrim TOML 不包含上游 API Key。需要注意：

- 上游 URL、Authorization 和请求正文由 code-Manager 原样交给真实上游请求；本程序不会在日志中输出请求正文或 API Key。
- `llmtrim.exe setup` 和 `llmtrim.exe stop` 只用于启停 daemon；后台执行时不会显示控制台窗口。
- “显示日志”才会创建可见的 `cmd.exe /k .\\llmtrim.exe status` 实体窗口，供用户查看 llmtrim 自己持续刷新的
  状态界面。
- 网关复用专用的 Go `http.Transport` 连接池访问 43117，显式设置 `Proxy=http://127.0.0.1:43117`，不读取 Windows
  `HTTP_PROXY` / `HTTPS_PROXY`，不会把到 llmtrim 的连接再次代理回自身。其空闲连接保留最多 5 分钟；不会对
  流式响应施加总请求时限。
- llmtrim 运行时，基线 `outbound_proxy` 与 H2/H3 客户端不参与实际请求；停止后立即恢复它们的原有行为。
- 页面启动按钮会调用 `llmtrim.exe setup`。运行状态确认后，新 GET/HEAD/POST 全部经 43117 正向代理；代理、CA、
  CONNECT 或 TLS 失败时不会绕过 llmtrim 直连上游。
- 页面停止按钮会调用 `llmtrim.exe stop`，停止确认成功后关闭并丢弃专用代理客户端的空闲连接；后续请求恢复基线路由。
- llmtrim 的“运行中”只在“当前配置路径对应的 llmtrim.exe 进程存在且 43117 已监听”时成立。成功启动或停止后，
  网关主动关闭已经进入 `/v1` 链路的 HTTP keep-alive 和 WS 连接，管理页面与基线 H2/H3 预热会话保持运行；客户端重连后
  才按新的完整运行状态重新选路。

本程序不会在日志中输出请求正文或 API Key。


配置
----

程序只使用 EXE 同级 `config\config.yaml`，不支持命令行指定其它配置路径。`config` 目录或配置文件
缺失时会自动创建 UTF-8 默认模板；此时管理页面可以打开，但必须先在页面中填写有效上游 API Key，代理才允许启动。

listen_address: "127.0.0.1:7780"
upstream_base_url: "https://your-api.example.com"
upstream_api_key: "你的上游 API Key"
outbound_proxy: ""
llmtrim_path: "C:\\EXEXX\\llmtrim\\llmtrim.exe"
local_api_key: ""
upstream_websocket_enabled: true
startup_enabled: false
background_start: false
retry_enabled: false
retry_count: "5"
retry_interval_seconds: "1"
retry_status_codes: "100-199,300-399,401-407,409-499,500-503,505-523,525-599"

各字段说明：

- listen_address：HTTP/WS `/v1` 网关监听地址。只允许一个字面 IP 和端口，格式为
  `IPv4:端口` 或 `[IPv6]:端口`，端口范围为 1-65535；不接受域名、协议前缀、多个地址或多个端口。
  `127.0.0.1` 是本机 IPv4 回环，`0.0.0.0` 监听所有 IPv4 网卡，`[::]` 监听所有 IPv6 地址，`[::1]` 仅本机
  IPv6 回环。管理页面仍固定使用 127.0.0.1:7780。
- upstream_base_url：实际 OpenAI 兼容上游的 HTTPS Base URL，必须是 HTTPS；可填写域名、IPv4 或方括号
  包围的 IPv6 根地址，例如 `https://example.com:32400`、`https://203.0.113.9`、
  `https://[2001:db8::9]:32400/v1`。省略端口时使用 443。字面 IPv4/IPv6 直接连接且不做 DNS；域名才
  DNS 解析。IP 连接不发送 SNI，证书必须含匹配的 IP SAN；域名使用标准 SNI 与 DNS 名称校验。网关不会
  强制补充或删除 `/v1`，会保留用户填写的路径格式并在其后追加接口路径。每次保存该字段时，code-Manager
  会从 URL 只提取 `Hostname()`（不含 scheme、路径、端口或 IPv6 方括号），完全覆盖
  `%USERPROFILE%\\.config\\llmtrim\\config.toml` 为受管的 `extra_hosts = ["<主机>"]`；不会写入 API Key。
  保存不会自动重启 daemon，必须在页面先点击“停止”、再点击“启动”，llmtrim 才会读取新主机并重建该主机的 CA。
- upstream_api_key：发送到上游 API 的密钥。默认模板使用占位值；未填写有效值时代理不能启动。不要将此文件提交到公开仓库或发送给他人。
- outbound_proxy：可选的上游 API 出站代理。留空表示上游 API 直连；支持
  socks5://127.0.0.1:1080 和 http://127.0.0.1:7890 形式。GitHub Release 版本查询、安装包和
  校验文件下载始终使用独立直连，不读取该配置或系统 HTTP_PROXY/HTTPS_PROXY。留空时启用
  上游 H3/H2 竞速；填写 HTTP/SOCKS5 代理时保持原有 TCP 出站路径，不让 H3 绕过已配置的代理。
  llmtrim daemon 运行时该字段暂不参与请求；daemon 停止后会恢复作为基线出站路径。
- llmtrim_path：llmtrim.exe 的绝对路径。首次启动时如果为空，程序只枚举一次当前进程快照；
发现正在运行的 llmtrim.exe 后回源读取完整路径并写回配置，未发现则保持为空。路径非空时不再做进程发现。
已填写路径但文件不存在时，程序会拒绝启动。管理页面只读显示该路径，不提供编辑；页面启动、停止和日志
操作始终使用服务端已保存的配置路径。需要更换或修复路径时，可通过受管安装流程重新安装 llmtrim；若失效
配置已经阻止 code-Manager 启动，则先手工修复或清空该字段。
- local_api_key：可选的本地网关访问密钥。留空表示不校验；设置后客户端必须发送
  Authorization: Bearer <local_api_key>。
- upstream_websocket_enabled：默认 true。只控制 llmtrim 未运行时的直连上游 WS 承载协商：开启时允许 H2/H3
  扩展 CONNECT 或已配置 HTTP/SOCKS5 出站代理上的 HTTP/1.1 Upgrade；关闭时不添加这些上游 WS 协商头，直接使用
  普通上游流。它不改变本地 HTTP/WS 监听能力，也不启动或停止 llmtrim daemon。页面成功保存该开关后，若代理正处于
  “运行中”或“上游连接中”，会先停止再启动代理，关闭本程序既有的基线上游连接池并按新值重新并发预热；代理已停止或
  上游不可用时只保存配置，等待用户下次手动启动。
- startup_enabled：是否注册当前用户的 Windows 开机启动项。开启后，用户登录 Windows 后程序会等待
  8 秒再初始化服务；只有 llmtrim 的状态文件要求恢复运行时，才尝试恢复 llmtrim 并启动代理。
- background_start：仅当 startup_enabled 为 true 时有效。开启后，开机启动不会自动打开浏览器管理页面；
  手动双击 EXE 仍会正常打开页面。
- retry_enabled：自动重试总开关，默认关闭。页面成功保存该开关后，若代理正处于“运行中”或“上游连接中”，同样会先停止
  再启动代理，使本程序的基线上游连接按新配置重建；代理已停止或上游不可用时只保存配置。
- retry_count：同一条上游请求流中连续相同状态码的额外重试上限，范围为 0-999；`0` 或留空表示不重试。
- retry_interval_seconds：命中状态码后的固定等待秒数，只接受非负整数；`0` 或留空时使用 500ms 最低等待。
- retry_status_codes：可重试状态码的单值或范围列表，使用英文 `,` 分隔、英文 `-` 表示范围；保存时删除所有空白、
  排序、去重并合并重叠或包含范围。每个单值及范围两端必须在 100-599；超出范围、起点大于终点或格式非法的片段会
  被整体删除。空字符串不匹配任何状态码。
- 重试请求体缓存：仅对实际可重试的 POST 建立。内存缓存以 32 KiB 分块保存，所有并发请求共用 16 GB 十进制额度；
  额度不足时会将已缓存分块和后续内容写入 EXE 同级临时文件。临时文件创建时使用仅当前 Windows 用户可访问的受保护
  DACL，请求完成、取消或异常时立即删除，程序启动时会清理遗留文件。

若需要让 Codex 通过此网关访问 API，请将其 Base URL 配置为：

http://127.0.0.1:7780/v1（或 config.yaml 中的 listen_address）


网页配置
--------

首页的“网关配置”提供监听地址、上游 Base URL、上游 API Key 和自动重试设置。
每次保存都会先删除换行和首尾空格，再校验并立即写入 `config\config.yaml`；按 Enter 不会提交，也不会把
回车保存到界面状态或配置。

- 上游 Base URL 和 API Key：保存后，后续转发请求立即使用新值；保存 Base URL 还会同步受管的
  `extra_hosts`，llmtrim 需先停止再启动才读取新主机。
- 监听地址：保存后写入 `config\config.yaml`；点击顶部“停止代理”彻底关闭当前 HTTP 监听和全部上游连接，再点击“启动代理”即可读取新地址并重新完成握手。管理页面始终保持在 127.0.0.1:7780。
- API Key 会在本机配置页面中明文显示，输入新的 Key 并保存即可替换。由于密钥可被本机访问
  7780 管理端口的程序读取，请勿将该页面或端口暴露到局域网/公网。
- “上游 WS 承载”和自动重试开关切换后立即保存。保存成功后，页面先读取代理状态；仅在代理运行中或连接中时，才复用
  现有“停止代理”“启动代理”流程串行重建本程序的基线上游连接池。两个开关在该流程期间会同时禁用；停止或启动失败时，
  已保存的新值会保留，页面显示失败状态，仍可使用顶部“启动代理”重试。代理停止或上游不可用时只保存配置，不会主动启动。
- 重试次数、间隔时间和状态码在输入框失焦时自动保存，没有单独保存按钮，也不会触发代理重启。无效次数、间隔或状态码
  片段会在失焦时清除并保存为空；状态码输入会自动规范化。保存请求返回期间继续编辑时，旧响应不会覆盖未失焦的新输入。
- 页面底部的“开机启动”会创建或删除当前用户的
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\code-Manager` 启动项；“后台运行”依赖
  “开机启动”，关闭开机启动时会被自动关闭并禁用。“停止并退出”会调用与托盘退出相同的清理流程。


RTK、snip 与 llmtrim 管理
-------------------------

页面页签顺序为 `RTK | snip | llmtrim`。llmtrim 独立运行；RTK 和 snip 为互斥的命令输出处理方式，
RTK 处于“已激活”或 snip 处于“运行中”时，另一方的版本加载、安装、启动和删除控件都会禁用，后端接口也会返回
HTTP 409，不能绕过网页同时激活两者。

- RTK 安装只下载到 EXE 同级 `RTK-AI\rtk.exe` 并写入版本标记，状态为“已安装/已停止”。点击“启动”后，
  code-Manager 才会写入用户与系统 PATH，并按实际检测到且可安全写入的平台配置 Codex 和 Claude Code 的提示词、
  Claude Code、GitHub Copilot、Cursor 的官方 Hook。点击“停止”只删除 code-Manager 自己写入的 PATH、两份提示词
  标记段和三个官方 Hook；完整命令参考、旧 `RTK.md`、`@` 引用及用户的其它内容保持不变。
- snip 安装只下载到 EXE 同级 `Snip\snip.exe`，不会立即修改 PATH 或 Agent 配置。启动时只检查已存在的
  四个平台用户级目录：Codex 固定为 `%USERPROFILE%\.codex`，Claude Code 使用
  `%CLAUDE_CONFIG_DIR%`（未设置时为 `%USERPROFILE%\.claude`），Cursor 与 GitHub Copilot 分别使用
  `%USERPROFILE%\.cursor`、`.copilot`。缺失 Hook 会由对应官方 `init` 创建；已经存在但非本程序受管的
  Snip Hook 不会被覆盖或接管。snip 通过这些 Agent 的原生 Hook 配置拦截命令，由 snip 自己决定是否处理；
  它不写项目级 `GEMINI.md`、`.windsurfrules`，也不安装第三方插件。
  停止、删除和迁移只精确删除账本已拥有、且仍指向对应发布目录绝对 `snip.exe` 路径的处理器，不调用官方
  宽匹配 `--uninstall`。Codex 信任完全由页面的独立授权操作处理；启动、停止和删除都不会改写 Codex 的
  Hook 开关或本地 `trusted_hash` 信任记录。code-Manager 会复算当前规范化 Hook 身份并与
  `trusted_hash` 比对，Hook 内容变化后显示“待人工确认”，不把“Hook 文件已写入”误报成“Hook 已获得 Codex 授权”。
- snip 四个平台的官方接入文件固定为：Codex=`%USERPROFILE%\.codex\hooks.json`（`PreToolUse`）、
  Claude Code=`%CLAUDE_CONFIG_DIR%\settings.json`（未设置变量时为
  `%USERPROFILE%\.claude\settings.json`；`PreToolUse`）、Cursor=`%USERPROFILE%\.cursor\hooks.json`
  （`beforeShellExecution`，matcher 为 `.*`）、GitHub Copilot=`%USERPROFILE%\.copilot\hooks\snip.json`
  （`preToolUse`）。code-Manager 使用同一份四平台规范表驱动目录检测、目标文件状态、初始化和精确清理，
  避免只处理其中一个平台。
- 四个平台的官方初始化命令分别是：Claude Code 使用 `snip.exe init`；Codex、Cursor、GitHub Copilot
  分别使用 `snip.exe init --agent codex|cursor|copilot`。Hook 命令使用 snip 写入的绝对 EXE 路径；
  Codex 为 `snip.exe hook codex`，Claude Code/Cursor 为 `snip.exe hook`，Copilot 为
  `snip.exe hook copilot`。Snip 上游提供的 `--uninstall` 会宽匹配同文件里的 Snip Hook，
  因而 code-Manager 不把它用于受管与手工接入并存的清理流程。
- 每次启动或停止都会统一检查四个平台：启动前先读取全部已存在的 Agent 目录并记录其目标文件快照；
  只有目标事件中不存在任何 Snip 命令时才调用官方初始化。运行中若受管处理器缺失，也仅在同一目标事件中没有其它
  Snip 命令时允许重新初始化；已被改写、移动、替换或格式异常的 Hook 不会交给上游宽匹配逻辑覆盖。任一初始化、PATH
  或状态写入失败都会恢复 Hook 快照和本次新增 PATH，不会留下半套接入。每次成功初始化都会把目标文件、事件、组/处理器
  下标、Hook 组上下文指纹、完整处理器指纹和命令写入 `Snip\.code-manager-state.json` 的 `metadata.snip_ownership_v1`；停止、删除和迁移
  只清理这份精确账本证明拥有的处理器。旧 `owned_agents` 仅用于恢复恰好一条、仍指向对应发布目录绝对 `snip.exe` 的
  旧 Hook；同一平台因 `CLAUDE_CONFIG_DIR` 等目录切换而出现多个受管目标文件时会分别记录并分别清理。歧义、未知或手工内容
  不会被接管或删除。
- snip PATH 是为兼容性保留的主动配置：启动时把 EXE 同级 `Snip` 目录分别写入当前用户 PATH 和系统 PATH，
  停止或删除时只移除 code-Manager 自己写入的条目；系统 PATH 写入需要管理员权限时会触发 UAC。Hook
  本身使用绝对 `snip.exe` 路径，因此即使某个 Agent 不读取新 PATH，Hook 仍可执行；PATH 主要用于手工调用
  `snip`、其它外部工具发现 snip，以及兼容不同 Agent 的运行环境。
- RTK 和 snip 启动时都写入各自目录的用户 PATH 与系统 PATH；系统 PATH 需要权限时会触发 UAC。RTK 与 snip
  的原生 Hook 使用绝对 EXE 路径；RTK 的 Codex 与 Claude Code 提示词规则仍使用 `rtk` 命令并依赖 Agent 进程加载到 PATH。
- RTK 的“已激活”与 snip 的“运行中”都表示激活步骤已经完成，不表示存在后台 daemon。对 RTK 而言，它也不表示四个平台都已接入；
  是否接入必须以平台明细为准。删除前必须先停止；停止失败时页面
  保留 `attention` 状态，不会伪装成已停止。RTK 使用专用精确归属账本记录每一段提示词和每一条 Hook；启动会迁移内容明确匹配的
  已知旧 RTK 提示词标记段，而停止/卸载对没有新账本的旧环境仅在原 `owned_agents` 明确记录的平台仍存在可识别内容时恢复归属；未知或手工
  内容不会被接管或删除。尚未发布阶段更换版本前仍应先停止并删除旧的 code-Manager 部署及其 RTK-AI 目录。
- snip 和 llmtrim 的“安装”仍按各自的受管归属规则处理旧环境：其它发布目录只有存在有效
  `.code-manager-state.json` 时才会纳入迁移。snip 只精确清理账本记录的目标文件、处理器位置、内容指纹和命令都仍可
  验证的 Hook，不调用官方 `--uninstall`；llmtrim
  只有发现受管目录时才会清理用户级 CA、代理环境、自启动和 `%USERPROFILE%\.llmtrim`。未带归属记录的手工安装、
  PATH、提示词和 Hook 不会被自动删除。正在执行的 RTK 命令或未受管程序占用 43117 时，安装会报错而不会强行结束它。
- 成功安装后只为 snip 和 llmtrim 记录专属目录到 `%APPDATA%\code-Manager\managed-tools.json`，用于它们未来
  移动发布目录时发现已验证的旧安装；RTK 不使用该迁移索引。


RTK 部署手册
-------------

本章描述 code-Manager 对 RTK 的完整部署边界。RTK 对已检测到且可安全写入的 Codex、Claude Code、
GitHub Copilot、Cursor 用户级配置执行同一套可回滚激活事务。以后修改 RTK 的安装、启动、停止、卸载、
升级、PATH 或页面状态时，必须同时检查四个平台的关联实现；但平台目录缺失时会被跳过，不能把“已激活”
误解为四个平台全部存在或全部接入成功。

项目根目录的 `RTK-部署总结.md` 是与具体电脑无关的开发说明，记录安装、激活、四平台状态、
归属和验收边界；该文件不记录用户密钥、绝对路径或一次性部署结果。

### 1. RTK 的作用和目录边界

RTK 是命令输出处理工具。code-Manager 不把 RTK 当作常驻 daemon，也不负责替 RTK 实现命令过滤逻辑；
RTK 只有在某个平台的 Hook 被触发时才执行：

平台 Shell 工具
  -> 平台原生 Hook
  -> `RTK-AI\\rtk.exe hook ...`
  -> RTK 判断是否重写命令并压缩输出
  -> 平台继续执行命令并接收结果

RTK 的所有专属文件都放在 `code-Manager.exe` 同级的 `RTK-AI` 目录：

- 可执行文件：`<code-Manager.exe 同级>\\RTK-AI\\rtk.exe`。
- 版本标记：`RTK-AI\\.rtk-version`，内容为当前 Release 的 tag。
- 完整命令参考：`RTK-AI\\RTK-Codex-commands.md`，由程序从嵌入资源写入，保留逐条正式语法、rewrite、
  TOML fallback、Windows 陷阱和源码审计细节，不是用户助手的原生配置文件。
- Codex 常驻规则：`RTK-AI\\RTK-Codex-agent-instructions.md`，同样由嵌入资源写入；其内容受 32 KiB
  上限保护，但本身包含正式 CLI 索引、rewrite/fallback 精确模式、Windows 特例和高风险语法纠错。
  完整命令参考保留给源码审计、未列参数与版本升级复核，不会被假定为每次命令都自动加载。
- Claude Code 提示词：`RTK-AI\\RTK-Claude-agent-instructions.md`，由嵌入资源写入；启动时在已有
  `.claude` 目录内写入或更新 `CLAUDE.md` 的专属 RTK 标记段。
- code-Manager 状态：`RTK-AI\\.code-manager-state.json`，只记录本程序写入的 PATH、平台接入归属和期望状态。

不要把 `C:\\EXEXX\\edit\\RTK-AI` 当作固定安装路径。它只是当前工作区的示例；程序运行时始终从自身
EXE 路径动态计算 `RTK-AI`，因此移动整个 EXE 和目录后仍应按新位置工作。

### 2. Release 来源、安装包和校验

- Release API 来源为 `https://api.github.com/repos/rtk-ai/rtk/releases`。
- Windows x64 只接受资产 `rtk-x86_64-pc-windows-msvc.zip`；当前程序不是为其它架构选择安装包。
- 每个可安装版本必须同时提供 `checksums.txt`。程序从该文件中按资产名查找 SHA-256，再计算下载 ZIP 的实际
  SHA-256；摘要不一致时拒绝安装。
- ZIP 会先解压到 `RTK-AI` 同级的临时目录。解压时按文件名安全收敛，拒绝路径穿越、重复文件名、没有
  `rtk.exe` 或没有可安装文件的压缩包，校验通过后才整体替换专属目录。
- 安装超时、校验失败、解压失败或版本标记/状态写入失败时会清理本次安装结果，不把半套文件报告为成功。
- 安装阶段只写入 RTK 文件、`.rtk-version`、完整命令参考、Codex/Claude Code 提示词资源和初始停止状态；不会写 PATH，也不会
  修改四个平台的用户配置。真正的 PATH 和平台接入只在“启动”阶段执行。

### 3. 四个平台的 RTK 接入映射

code-Manager 只接入用户级配置。目标目录不存在时跳过该平台；目标目录存在时，按平台官方格式创建或更新
Hook 配置文件。Claude Code 的已有 `.claude` 目录还会创建或更新 `CLAUDE.md` 的专属 RTK 提示词标记段；
程序不会创建不存在的用户级平台目录。

| 平台 | 检测目录/环境变量 | 目标文件 | code-Manager 写入内容 | 运行入口 |
|---|---|---|---|---|
| Codex | `%CODEX_HOME%`；未设置时 `%USERPROFILE%\\.codex` | `AGENTS.md` | `---RTK命令_开始---` 与 `---RTK命令_结束---` 之间不超过 32 KiB 的 `RTK-Codex-agent-instructions.md` 高密度规则；包含正式 CLI、rewrite、fallback、Windows 与陷阱决策表。未列参数、版本升级时才查 `RTK-Codex-commands.md` | 由 Codex 按常驻决策表执行 RTK 命令 |
| Claude Code | `%CLAUDE_CONFIG_DIR%`；未设置时 `%USERPROFILE%\\.claude` | `CLAUDE.md`、`settings.json` | 在 `CLAUDE.md` 追加专属 RTK 提示词标记段，并在 `hooks.PreToolUse` 中追加 `matcher: "Bash"` 的命令 Hook；保留其它用户内容 | `"<启动时按 RTK-AI 安装目录生成的路径>\\rtk.exe hook claude"` |
| GitHub Copilot | `%COPILOT_HOME%`；未设置时 `%USERPROFILE%\\.copilot` | `hooks/rtk-rewrite.json` | 在 `hooks.PreToolUse` 中追加官方命令 Hook，包含 `cwd: "."`、`timeout: 5`；保留其它用户 Hook | `"<启动时按 RTK-AI 安装目录生成的路径>\\rtk.exe hook copilot"` |
| Cursor | `%USERPROFILE%\\.cursor` | `hooks.json` | 在 `hooks.preToolUse` 数组中追加 `matcher: "Shell"` 的官方 RTK Hook；保留其它用户 Hook | `"<启动时按 RTK-AI 安装目录生成的路径>\\rtk.exe hook cursor"` |

Codex 使用受上限保护的 `RTK-Codex-agent-instructions.md` 高密度决策表；Claude Code 同时使用
`RTK-Claude-agent-instructions.md` 提示词和官方 Hook，GitHub Copilot、Cursor 使用上表中的官方 Hook。完整 `RTK-Codex-commands.md` 仍随 EXE 打包并释放
到 `RTK-AI`，保存逐条审计依据、少见参数和升级复核资料。停止或卸载时只删除 code-Manager 自己写入的内容，
不删除用户原有的其它段落、Hook 或配置。

### 4. 安装、启动是两个不同阶段

**安装版本**只解决“RTK 文件是否存在”：

1. 页面从 Release 列表选择 tag。
2. 后端下载 Windows x64 ZIP 和 `checksums.txt`。
3. 校验 ZIP 的 SHA-256。
4. 安全解压并替换 `RTK-AI` 专属目录。
5. 写入 `.rtk-version`、`RTK-Codex-commands.md`、Codex/Claude Code 提示词资源和停止状态。

**启动 RTK**才解决“系统是否接入”：

1. 获取 RTK/snip 统一互斥锁；snip 正在运行时返回冲突，不自动抢占。
2. 确认 `RTK-AI\\rtk.exe` 和状态文件可读。
3. 快照已检测到平台的提示词与 Hook 目标文件，快照内容包含文件是否存在和原始 UTF-8 字节。
4. 读取用户 PATH 与系统 PATH，按分段、大小写不敏感方式判断 RTK-AI 目录是否已存在。
5. 补齐用户 PATH 和系统 PATH；系统 PATH 无权限时通过 UAC 请求管理员权限。
6. 写入/刷新完整命令参考、Codex 高密度规则和 Claude Code 提示词资源，再处理 Claude、Copilot、Cursor 官方 Hook。
7. 记录本次实际写入或迁移的提示词段、Hook 目标文件和完整命令，以及 PATH 归属，写入 `.code-manager-state.json`，最后才将 `running` 和
   `desired_running` 置为 `true`。

启动阶段是一个事务：任意已检测到且可写入的平台文件解析失败、文件写入失败、Cursor JSON 结构异常、PATH
写入失败或状态无法落盘，都会恢复本次快照并回滚本次新增 PATH。缺失的平台目录会被跳过；页面的“已激活”
只表示这套事务已经提交，四个平台是否接入必须查看逐项状态。RTK 本身没有后台进程。

### 5. PATH 部署和兼容性规则

RTK PATH 是项目主动保留的兼容性功能，不要在后续修改中删除：

- 用户 PATH 对应注册表 `HKCU\\Environment\\Path`。
- 系统 PATH 对应 `HKLM\\SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment\\Path`。
- 启动时分别检查并写入 `RTK-AI` 目录；已存在的同目录条目不会重复添加，比较不区分大小写并忽略首尾引号和
  斜杠差异。
- 写入注册表后广播 Windows 环境变量变化，使新启动的进程可以看到新 PATH。当前 code-Manager 进程不会凭空
  更新已经启动的其它程序环境块。
- 系统 PATH 读写需要管理员权限时触发隐藏的 UAC PowerShell 操作；用户取消 UAC、写入后复查失败或只成功写入
  一个范围，启动会失败并回滚用户 PATH。
- Claude Code、GitHub Copilot、Cursor 的 Hook 在启动时写入按当前 RTK-AI 安装目录动态生成的绝对 EXE 路径，
  因此三类 Hook 本身不依赖当前进程是否重新加载 PATH；Codex 和 Claude Code 的 Markdown 提示词按内置文档执行 `rtk` 命令，
  仍依赖对应 Agent 进程能够看到用户/系统 PATH。PATH 主要用于 Agent 提示词、手工调用 `rtk`、其它外部工具
  发现 RTK 以及兼容不同平台的运行环境。
- 停止/卸载只移除状态文件记录为 code-Manager 所拥有的 PATH 条目，不删除用户手工加入的同目录条目。

程序不会仅因为发现 `RTK-AI\\rtk.exe` 就自动激活 RTK 或修改四个平台。RTK 的 PATH、Codex/Claude Code 提示词和三类官方 Hook
只在用户点击“启动”或开机恢复明确要求 `desired_running=true` 时，通过同一套启动事务配置。

### 6. 四个平台统一启动事务的维护要求

四个平台必须作为关联整体检查；对已检测到且可写入的平台，以下任一情况都不能只修一个平台后结束：

- 修改安装目录、版本切换、ZIP 解压或版本标记时，同时确认 RTK Hook 使用的是新 EXE 的绝对路径。
- 修改 Codex 或 Claude Code 提示词段时，同时确认 Claude `CLAUDE.md`、`settings.json`、Copilot
  `hooks/rtk-rewrite.json` 和 Cursor `hooks.json` 仍可解析、仍能移除。
- 修改 PATH 或 UAC 逻辑时，同时检查三类平台 Hook 是否仍不依赖 PATH、Codex/Claude Code 提示词是否可看到 PATH，以及失败时能否恢复全部快照。
- 修改状态字段、启动/停止按钮或 `/api/rtk` 返回时，同时核对四个平台的 `available/configured/modified` 状态；Claude Code 还要分别核对 Hook 与提示词。
- 修改卸载逻辑时，同时确认四个平台的本程序内容都能清理，用户其它内容和用户未拥有的平台不被删除。

### 7. 停止流程

点击 RTK“停止”后，程序按状态文件执行以下动作：

1. 获取统一互斥锁并读取 `metadata.rtk_ownership_v1`、`user_path`、`system_path`；旧状态的 `owned_agents` 只用于安全恢复可明确识别的旧归属。
2. 删除本程序拥有的用户/系统 PATH 条目。
3. 仅按账本记录的目标文件和提示词内容 SHA-256 指纹，清理 Codex `AGENTS.md` 与 Claude Code `CLAUDE.md` 中对应的标记段。
4. 从 Claude `settings.json` 的 `hooks.PreToolUse`、Copilot `hooks/rtk-rewrite.json` 的
   `hooks.PreToolUse` 和 Cursor `hooks.json` 的 `hooks.preToolUse` 中，仅删除账本记录的目标文件内、完整绝对命令相同的 Hook。
5. 将 `running`、`desired_running`、PATH 归属清零并写回状态。

账本不存在时，停止只会从旧 `owned_agents` 恢复仍能精确确认的旧 RTK 提示词或当前发布目录绝对命令的 Hook；
不会按文件名、RTK 关键字或其他 `rtk.exe` 路径猜测清理。清理任一平台失败时保留 `attention` 和错误信息，不能伪装成
“已停止”。停止不会结束某个正在执行的外部 Shell 命令；它只撤销后续触发所需的接入。

### 8. 卸载流程和目录保护

卸载必须先处于当前状态文件记录的停止状态；只要状态文件要求运行或发现 `rtk.exe` 进程仍在运行，后端就拒绝
删除。状态文件不可用时不会猜测旧接入归属。卸载顺序为：

1. 按精确归属账本清理四个平台的 RTK 内容；旧 `owned_agents` 只恢复可准确验证的旧内容。
2. 按 `user_path/system_path` 清理本程序拥有的 PATH，并复查两处注册表。
3. 删除名称精确为 `RTK-AI` 的专属目录，并复查 `rtk.exe` 和目录是否不存在。
4. 删除 `.code-manager-state.json`；若任一步校验失败，返回错误并保留可诊断状态。

目录删除有保护：只有路径最后一级名称（不区分大小写）为 `RTK-AI` 且确实为目录时才允许 `RemoveAll`，
不会因为路径配置错误而删除其它目录。运行中的 RTK EXE 也会阻止卸载，避免正在执行的 Hook 指向失效文件。

### 9. 版本切换和升级

切换版本等同于“停止后卸载文件，再安装新版本”，不是覆盖正在使用的 EXE：

- RTK 运行中、`desired_running=true` 或 `rtk.exe` 进程存在时，拒绝切换。
- 当前版本不自动迁移其它发布目录、旧 PATH 或旧平台接入；在尚未发布阶段更换版本前，先手动停止并删除旧
  code-Manager 部署，再安装并启动新版本。
- 新版本安装失败时不会留下缺少 `rtk.exe` 的 RTK-AI 目录，也不会覆盖已有平台配置。
- 安装新版本不会自动重新启用四个平台；安装完成后仍需点击“启动”，让新 EXE 路径和四个平台接入重新成为
  同一事务。
- 若用户只手工替换了 `RTK-AI\\rtk.exe`，页面可能显示版本标记与实际 EXE 不一致，应通过页面重新安装目标
  Release，不要绕过版本标记和状态文件。

### 10. 状态文件字段

`RTK-AI\\.code-manager-state.json` 是 code-Manager 的归属账本，不是 Codex、Claude、Copilot 或 Cursor
原生配置的替代品。主要字段如下：

- `desired_running`：下次 code-Manager 启动时是否尝试恢复 RTK。
- `running`：最近一次 RTK 激活事务是否已提交；不代表有 RTK daemon 进程，也不代表四个平台全部接入。
- `updated_at`：状态最后更新时间。
- `user_path` / `system_path`：对应 PATH 范围是否由本程序写入并应由本程序清理。
- `owned_agents`：由精确账本派生的兼容摘要，值可能包含 `codex`、`claude-code`、`copilot`、`cursor`；RTK 停止和卸载不以它作为唯一删除依据。
- `owned_files`：通用状态字段；当前 RTK 不依赖它判断提示词或 Hook 归属。
- `metadata.rtk_ownership_v1`：RTK 精确归属账本。Codex/Claude 提示词记录绝对目标路径和规范化内容的 SHA-256 指纹；Claude/Copilot/Cursor Hook 记录绝对目标路径和完整命令。
- `metadata.last_error`：最近失败原因；存在时页面显示 `attention`，成功启动后清除。

若 RTK 与 snip 的状态文件同时要求恢复，程序不会擅自选择其中一个，页面显示冲突；必须先停止一方，再启动
另一方。`startup_enabled` 只控制 code-Manager 自身的 Windows 开机启动，不等于 RTK 的 `desired_running`。

### 11. 页面接口和状态含义

- `GET /api/rtk`：返回安装路径、版本、用户/系统 PATH、四个平台的 available/configured 状态、激活状态、冲突
  和修改平台列表。
- `GET /api/rtk/releases?page=N`：读取 GitHub Release 分页，标记是否存在 Windows x64 安装包。
- `POST /api/rtk/install`：下载、校验并安装选定 tag；不写 PATH、不接入平台。
- `POST /api/rtk/start`：执行 PATH 配置、内置文档写入、Codex/Claude Code 提示词与 Claude/Copilot/Cursor 官方 Hook 的统一接入事务。
- `POST /api/rtk/stop`：按状态归属清理 PATH、Codex/Claude Code 提示词段和 Claude/Copilot/Cursor 官方 Hook。
- `POST /api/rtk/uninstall`：确认停止后执行清理并删除 RTK-AI 专属目录。

状态中的 `available` 表示目标目录/文件存在，不表示平台软件一定正在运行；`configured` 表示检测到匹配的
RTK 内容；Claude Code 只有 Hook 与 `CLAUDE.md` 提示词都匹配时才为完整配置；`modified_agents` 表示精确账本记录且
当前仍存在的受管项目所属平台。Codex 状态只检查 `AGENTS.md` 标记段，不读取 Codex 个性化界面文本。`running=true`
只表示已激活，四个平台任意一个未配置都应在页面和排错中明确显示。

### 12. 常见排错

**Release 列表为空或版本不可安装**

确认当前系统为 Windows x64，目标 Release 同时存在 `rtk-x86_64-pc-windows-msvc.zip` 和 `checksums.txt`；
缺少任一资产、网络读取失败或 SHA-256 不匹配都会被拒绝。

**RTK 已安装但显示 PATH 未完整配置**

点击“启动”并接受系统 PATH 的 UAC。检查用户 PATH 和系统 PATH 是否都包含当前 EXE 同级的 `RTK-AI`；
不要只检查当前 PowerShell 窗口，因为已启动进程可能仍使用旧环境块。

**只有一个平台显示已配置**

先确认其它平台的用户目录是否存在，再重新点击“启动”。程序不会创建缺失的用户级平台目录；Claude、Copilot、
Cursor 的 Hook 配置文件会在目录存在时按官方格式创建，Claude 的 `CLAUDE.md` 提示词也会在 `.claude` 目录存在时创建，Codex 仍要求已有 `AGENTS.md`。
Codex 页面中的个性化文本不等同于 `AGENTS.md` 的 RTK 标记段，因此个性化界面显示内容时，RTK 状态仍可能显示“规则未集成”。

**Cursor 启动失败并提示 JSON 结构异常**

检查 `%USERPROFILE%\\.cursor\\hooks.json` 中 `hooks` 是对象，`preToolUse` 是数组，RTK 条目的
`matcher` 为 `Shell`。程序发现结构不符合预期时拒绝覆盖，先备份并修复 JSON 后再启动。

**提示 RTK 与 snip 冲突**

以页面状态和 `/api/rtk`、`/api/snip` 为准。停止 snip 后再启动 RTK；只关闭某个窗口或进程，不一定清理了另一方
的状态文件、PATH 或平台 Hook。

**卸载后仍显示残留**

确认 Codex 与 Claude Code 的 RTK 提示词标记段、Claude/Copilot/Cursor 官方 Hook、用户 PATH、系统 PATH 和 `RTK-AI` 目录都已清理。若状态文件损坏或
清理步骤失败，页面会保留 `attention`，不要手工删除状态来掩盖错误；先备份四个平台文件，再按停止流程重试。

### 13. RTK 修改时的四平台关联检查清单

以后每次修改 RTK 相关代码、配置或 UI，都按下面顺序核对：

1. **目录和路径**：检查 `RTK-AI` 动态定位、`.rtk-version`、`rtk.exe`、完整命令参考、Codex/Claude Code 提示词资源和状态文件路径。
2. **Release 安装**：检查 Windows x64 资产、`checksums.txt`、SHA-256、ZIP 安全解压、临时目录替换和失败回滚。
3. **Codex**：检查 `CODEX_HOME` 覆盖、`AGENTS.md` 的专属规则标记段、完整参考只写入 `RTK-AI`、旧 `RTK.md`/`@`
   引用不被程序迁移或清理，以及 UTF-8 写入。
4. **Claude Code**：检查 `CLAUDE_CONFIG_DIR` 覆盖、`CLAUDE.md` 的专属提示词标记段、`settings.json` 的 `hooks.PreToolUse`、`Bash` 匹配、动态绝对 EXE 命令、追加幂等和移除。
5. **GitHub Copilot**：检查 `COPILOT_HOME` 覆盖、`.copilot\\hooks\\rtk-rewrite.json` 的 `hooks.PreToolUse`、动态绝对 EXE 命令、追加幂等和移除。
6. **Cursor**：检查 `.cursor\\hooks.json` 的 JSON 对象/数组校验、`preToolUse` 的 `Shell` 匹配、动态绝对 EXE 命令、追加幂等和移除。
7. **统一事务**：检查已检测平台的提示词/Hook 快照、PATH 快照、逐步写入、失败恢复和状态落盘；所有可写目标成功后才可标记 `running=true`，缺失平台则明确显示未检测到。
8. **停止/卸载**：检查 `metadata.rtk_ownership_v1` 的精确归属、旧 `owned_agents` 的安全恢复、四平台清理、PATH 清理、进程阻止、RTK-AI 精确目录保护和残留复查。
9. **互斥与恢复**：检查 RTK/snip 锁、`desired_running`、开机启动恢复和冲突提示，不让两者同时激活。
10. **页面和移动端**：检查 `frontend/src/App.vue` 的“已激活”说明、四平台逐项状态、Claude Hook/提示词拆分状态、按钮禁用条件、错误提示和窄屏换行。
11. **文档同步**：只要部署路径、命令、状态字段、API 或清理边界发生变化，必须同步更新本章和总览摘要。

对应代码关系：

- `rtk_install.go`：RTK 安装、启动、停止、卸载、互斥、状态和事务回滚。
- `rtk_release.go`：Release 查询、Windows x64 资产、`checksums.txt` 校验、ZIP 安全替换。
- `rtk_windows.go`：用户/系统 PATH、注册表操作、UAC、环境变量广播和 PATH 清理。
- `assistant_integration.go`：Codex/Claude Code 目录环境变量、专属提示词标记段和 UTF-8 原子写入。
- `rtk_native_integrations.go`：Claude/Copilot 官方 Hook 的 JSON 写入、检测和清理。
- `rtk_extra_integrations.go`：Cursor Hook、四平台快照和恢复。
- `rtk_codex_commands.go`：嵌入并写入完整 `RTK-Codex-commands.md`、不超过 32 KiB 的 Codex 提示词与 Claude Code 提示词。
- `tool_state.go`：`RTK-AI\\.code-manager-state.json` 的读取、写入和 attention 状态。
- `frontend/src/App.vue`：RTK 页签、四个平台状态、安装/启动/停止/卸载按钮和错误展示。
- `build.bat`：在 CMD 中使用 `npm.cmd` 构建前端，再嵌入 `web/dist` 生成唯一发布 EXE。

### 14. 实际部署操作顺序

首次部署或更换 RTK 版本时，按下面顺序操作，不要直接手工改某一个平台：

1. 退出正在使用 RTK 或 snip 的 Agent 会话；确认页面中 RTK 不是“已激活”、snip 不是“运行中”，并处理可能存在的
   `attention` 或“冲突”状态。
2. 确认 `code-Manager.exe` 所在目录可写，且当前系统为 Windows x64；不要把 RTK 解压到用户配置目录或项目
   目录之外再让 Hook 指向另一份 EXE。
3. 在 RTK 页签加载 Release，选择带有 Windows x64 ZIP 和 `checksums.txt` 的版本，点击“安装”。安装完成后
   页面应显示“已安装/已停止”，此时 PATH 和四个平台仍可能未接入，这是正常的阶段边界。
4. 确认需要接入的平台目录存在：Codex 需要已有 `AGENTS.md`；Claude、Copilot、Cursor 的 Hook 配置文件由
   RTK 启动流程按官方格式创建或更新，Claude 的 `CLAUDE.md` 提示词会在 `.claude` 目录存在时创建；程序不创建缺失的平台目录。
5. 点击“启动”，接受系统 PATH 的 UAC。程序会对已检测到且可写入的平台执行一次事务；所有这些目标、PATH 写入、
   状态落盘成功后，页面显示“已激活”，但未检测到的平台仍会明确显示未接入。
6. 在页面确认四个平台的 `available/configured/modified` 状态，并确认 Claude Code 的 Hook 与提示词状态；重启已经打开的 Codex、Claude Code、Copilot
   和 Cursor 会话。Claude/Copilot/Cursor Hook 使用绝对路径，不受旧 PATH 影响；Codex 若在启动时显示 `Hooks need review`，应在同一个 PowerShell 窗口的
   CLI 审核界面完成信任；不要把 `/hook` 或 `/hooks` 当作首次审核命令。
7. 修改 RTK 版本或移动 EXE 前，先手动停止并删除旧 code-Manager 部署及其 `RTK-AI` 目录；当前版本不会自动
   迁移旧 PATH 或平台接入。若当前 `rtk.exe` 正在执行，安装会明确报错，避免中断命令。删除仍要求先点击“停止”。

停止、升级或卸载后，验收标准不是“rtk.exe 进程已结束”，而是已检测到平台的本程序提示词/Hook、用户/系统 PATH、状态
和 RTK-AI 专属目录都与页面状态一致。用户手工写入的其它 Hook、提示词、Markdown 内容必须仍然保留。

### 15. 手工只读验收

以下命令只读取 RTK 文件、状态和 PATH，不会调用 `init`、不会写入 Agent 配置，也不会修改注册表。将第一行的
占位路径替换为实际 `code-Manager.exe` 路径：

```powershell
$codeManager = 'C:\\path\\to\\code-Manager.exe'
$rtkDir = Join-Path (Split-Path -Parent $codeManager) 'RTK-AI'
$rtkExe = Join-Path $rtkDir 'rtk.exe'
Get-Item $rtkExe, (Join-Path $rtkDir '.rtk-version'), (Join-Path $rtkDir '.code-manager-state.json')
& $rtkExe --version
& $rtkExe --help
[Environment]::GetEnvironmentVariable('Path', 'User')
[Environment]::GetEnvironmentVariable('Path', 'Machine')
Get-Content (Join-Path $rtkDir '.code-manager-state.json')
$codexDir = if ($env:CODEX_HOME) { $env:CODEX_HOME } else { Join-Path $env:USERPROFILE '.codex' }
$claudeDir = if ($env:CLAUDE_CONFIG_DIR) { $env:CLAUDE_CONFIG_DIR } else { Join-Path $env:USERPROFILE '.claude' }
Get-Content (Join-Path $codexDir 'AGENTS.md')
Get-Content (Join-Path $claudeDir 'CLAUDE.md')
Get-Content (Join-Path $claudeDir 'settings.json')
$copilotDir = if ($env:COPILOT_HOME) { $env:COPILOT_HOME } else { Join-Path $env:USERPROFILE '.copilot' }
Get-Content (Join-Path $copilotDir 'hooks\\rtk-rewrite.json')
Get-Content (Join-Path $env:USERPROFILE '.cursor\\hooks.json')
```

文件不存在时，对应平台表示尚未创建用户级配置，不代表 RTK 安装失败。手工排查不要执行 `rtk init`、直接编辑四个平台文件或
手工删除 PATH；这些操作会绕过 `owned_agents` 和状态账本，导致后续停止/卸载无法准确判断归属。


snip 部署手册
-------------

**维护时的权威流程（2026-09-03）**：Snip 的“Hook 初始化”和 Codex 的“Hook 授权”是两个完全独立的
步骤。点击 snip“启动”只检测并接入存在的四个平台用户级 Hook，不检测、不打开 PowerShell，也不等待 Codex
信任；页面中的“Codex Hook 信任”区域才负责被动显示信任状态，用户点击“添加信任”后才打开实体 PowerShell。
PowerShell 会自动启动动态定位的 Codex CLI，但最后仍由用户在同一个窗口中等待 `Hooks need review`、选择第 2 项
`Trust all and continue`、按键盘 Enter（回车）完成授权。这里是选择菜单项，不是输入数字字符 `2`。首次审核**不要输入 `/hook` 或 `/hooks`，也不要输入 `1` 后再走旧的 `Review hooks`
和 `t` 流程**。以后排查、修改或回答本项目问题时，应以本段和下面第 5 节为准，不得恢复旧说明。

首次成功部署后的实际运行状态、Hook 归属、信任边界和目录差异见项目根目录
`SNIP-部署总结.md`；该文件不记录用户密钥或信任哈希，可用于日常验收和迁移前核对。

### 1. 组件边界

snip 是命令输出过滤器，不是常驻后台服务。`Snip\snip.exe` 只有在 Agent 触发原生 Hook 时才会被调用：

Agent 执行 Bash/Shell 工具
  -> Agent 原生 Hook
  -> `Snip\snip.exe hook ...`
  -> snip 判断是否重写命令并过滤输出
  -> Agent 继续执行命令并接收结果

`snip init` 只负责把上述 Hook 命令登记到 Agent 的用户级配置中；真正的命令判断、重写和输出压缩由
`snip.exe hook` 完成。code-Manager 不实现第二套过滤逻辑，也不把 snip 伪装成 daemon。

### 2. 安装包部署

- 页面“安装”只下载 Windows x64 的 snip Release 压缩包，要求 Release 同时提供 `checksums.txt`；
  code-Manager 按资产文件名匹配 SHA-256，校验通过后才替换 EXE 同级的专属 `Snip` 目录。
- 当前项目中的安装目录示例为 `C:\EXEXX\edit\Snip`；运行时实际使用 `code-Manager.exe` 同级的 `Snip` 目录，
  因此整体移动 EXE 和目录后仍可工作。可执行文件为 `<code-Manager.exe 同级>\Snip\snip.exe`，版本记录在
  `Snip\.snip-version`。
- 安装阶段不会写用户 PATH，不会创建 Agent Hook，不会修改 `%USERPROFILE%` 下的配置；这些动作只在点击“启动”时执行。
- 安装或版本切换要求 snip 已停止；若状态账本仍为 `running` 或 `desired_running`，后端返回 HTTP 409，
  不会在运行中替换 Hook 指向的 EXE。停止后，安装才会迁移本程序账本记录拥有的 PATH 和 Agent Hook，再替换
  `Snip` 目录；没有账本归属的用户 Hook 不会被卸载。

### 3. 四个平台官方映射

code-Manager 只接入用户级目录；目录不存在就跳过，不会为了“检测软件”而创建空目录。目录存在时，
首次启动可以让 snip 创建缺失的目标文件。

| Agent | 检测目录 | snip 初始化命令 | snip Hook 命令 | 目标文件/事件 |
|---|---|---|---|---|
| Codex | `%USERPROFILE%\\.codex` | `snip.exe init --agent codex` | `snip.exe hook codex` | `hooks.json` / `PreToolUse`，匹配 `Bash` |
| Claude Code | `%CLAUDE_CONFIG_DIR%`；未设置时 `%USERPROFILE%\\.claude` | `snip.exe init` | `snip.exe hook` | `settings.json` / `PreToolUse`，匹配 `Bash` |
| Cursor | `%USERPROFILE%\\.cursor` | `snip.exe init --agent cursor` | `snip.exe hook` | `hooks.json` / `beforeShellExecution`，matcher 为 `.*` |
| GitHub Copilot | `%USERPROFILE%\\.copilot` | `snip.exe init --agent copilot` | `snip.exe hook copilot` | `hooks\\snip.json` / `preToolUse`，Bash 命令字段 |

四个平台的目录、目标文件和命令由 `snip_install.go` 的同一份规范表驱动。不要在其它文件中另写平台
路径或自行拼接 Hook 命令；需要调整时必须同步检查四个平台、状态和卸载流程。

### 4. 启动接入流程

点击 snip“启动”后，后端按以下顺序执行：

1. 获取 RTK/snip 统一互斥锁；RTK 运行中或状态损坏时拒绝启动。
2. 确认 `Snip\\snip.exe` 和版本状态存在；仅当本次需要新增或修复 Codex Hook 时，才定位
   `0.131.0+` 的 Codex CLI，避免写入旧版无法处理 `PreToolUse.updatedInput` 的 Hook。已有非受管 Codex
   Hook 不会阻断其它平台的接入。
3. 按四个平台的官方目录规则扫描用户级目录，生成当前目标文件快照；已有文件的内容会原样保存。
   若本次需要初始化 Claude Code 或 Cursor，而 `<code-Manager.exe 同级>\\Snip\\snip.exe` 的绝对路径含空格，
   Snip 0.25.0 的官方初始化命令无法可靠引用该路径，启动会在写 PATH 或 Hook 前失败；应将 code-Manager.exe
   移到不含空格的目录后重试。
4. 检查并补齐 Snip 用户 PATH、系统 PATH；系统 PATH 缺失时通过 UAC 请求管理员权限。
5. 只对目标事件中不存在任何 Snip 命令的平台执行对应官方 `init` 命令。运行中受管 Hook 缺失时，也必须先确认
   同一事件不存在其它 Snip 命令；若处理器已被改写、移动或替换，页面显示待处理而不会把手工配置传给上游 `init`。
6. 初始化后从前后快照中唯一定位本次新增的处理器，记录目标文件、事件、组/处理器下标、Hook 组上下文指纹、完整 JSON 指纹和实际命令到
   `metadata.snip_ownership_v1`；无法唯一确认新增处理器时回滚，不把平台标记为受管。

启动流程不会检测或添加 Codex 信任，也不会因为 Codex 尚未信任而中断；Codex 信任状态只在页面中被动显示，
需要时由“Codex Hook 信任”区域的“添加信任”按钮单独触发。

四个平台作为一个事务处理：任一 `init` 失败、无法唯一确认新增 Hook、状态无法写入或 PATH 操作失败，都会回滚本次
已经完成的 Hook 和 PATH 修改，不返回“运行中”。已经运行时再次点击“启动”也会复查四个平台和双范围 PATH；
只有精确受管 Hook、用户 PATH、系统 PATH 都完整时才快速返回。已停止账本中残留的历史归属不会用于覆盖后来出现的
手工 Hook；若仍有受管残留，页面将显示“清理”而不是要求重新启动。

### 5. Codex 信任检测与人工审核

Codex 的原生 Hook 需要由 Codex 自己审核和保存信任。开源的 snip、Hook 文件已经写入，或者 code-Manager
已经完成初始化，都不能代替这一步。Snip“启动”只负责平台 Hook 接入，不负责打开审核窗口；需要添加信任时，
用户点击“Codex Hook 信任”区域的“添加信任”按钮。code-Manager 不模拟键盘、不发送 `t`、不自动点击 Codex
的确认界面，也不直接篡改 Codex 的信任账本。

#### 5.1 只读检测位置

- Codex Hook 文件固定为 `%USERPROFILE%\.codex\hooks.json`。Snip 0.25.0 的官方 Codex
  初始化不读取 `CODEX_HOME`，因此 code-Manager 也不能把该变量作为 Snip Hook 或信任记录的目录覆盖。
- Codex 配置/信任记录：同一目录下的 `config.toml`。
- 信任记录的形式是与 Hook 条目关联的 TOML 节点，例如
  `[hooks.state.'<hooks.json 路径>:pre_tool_use:0:0']` 下的 `trusted_hash`。
- code-Manager 按当前 Codex CLI 的规范化身份规则复算 `PreToolUse` 处理器哈希，再与对应节点的
  `trusted_hash` 精确比对。命令、matcher、超时、异步标志、上下文限制或条目位置变化都会使既有信任失效。
  除页面“添加信任”启动的 Codex 原生审核外，code-Manager 不会通过启动、停止、删除或回滚流程改写
  `config.toml` 的 Hook 开关或 `trusted_hash`。

#### 5.2 页面显示的信任状态

| 状态 | 判定 | 页面/信任操作 |
|---|---|---|
| `not_applicable` | 未检测到 Codex Snip Hook，或 Codex 用户目录不存在 | 不显示 Codex 信任要求；其它已存在的平台仍可接入 |
| `trusted` | 当前 `hooks.json` 对应的信任节点存在且 `trusted_hash` 与当前规范化 Hook 身份完全一致 | 页面显示“已信任”；不影响 Snip 启动按钮 |
| `untrusted` | 已找到 Snip Hook，但没有对应信任记录，或记录哈希与当前 Hook 不一致 | 页面显示“待人工确认”，显示“添加信任”按钮 |
| `unknown` | `config.toml` 无法读取或格式无法判断 | 页面显示待处理；点击“添加信任”尝试打开审核窗口 |
| `disabled` | `[features]` 中 `hooks = false` 或 `codex_hooks = false` | 页面提示先手动开启；“添加信任”不会擅自覆盖该策略 |

若存在旧哈希但当前 Hook 内容已变化，页面显示“待人工确认”并说明需要重新审核。页面的 Agent 状态还会区分
“已由本程序精确接入”“受管处理器已改写或移动”和“非本程序受管”，不会把手工 Hook 误记为 code-Manager 所有。
Claude Code、Cursor、GitHub Copilot 没有使用这份 Codex `hooks.state` 记录，不能用 Codex 的状态代表其它平台。

#### 5.3 点击“添加信任”后的流程

当页面检测到 Codex 未信任时，点击“添加信任”后，后端会：

1. 不修改 Snip Hook、PATH 或 Snip 运行状态；启动和信任是两个独立操作。
2. 在 PATH 中查找 `codex.exe`、`codex.cmd` 或 `codex`；找不到时扫描
   `%LOCALAPPDATA%\OpenAI\Codex\bin\<版本目录>\codex.exe`，选择最近修改的可执行文件。Codex 的
   Snip 原生 Hook 需要 CLI `0.131.0+`；低于该版本时必须先升级，不能以“Hook 文件已写入”冒充可用。
3. 以当前 `code-Manager.exe` 所在目录作为项目目录生成 `-C`，避免把开发机路径或版本目录写死。
4. 使用可见的新 PowerShell 控制台启动 Codex，等价于自动执行：

   ```powershell
   & '<动态定位的 codex.exe>' -C '<code-Manager.exe 同级目录>'
   ```

   启动器先用隐藏的 `cmd.exe /d /c start` 创建一个独立的顶层 PowerShell 控制台，再以 `-NoExit` 保留窗口；
   这不是把用户困在后台启动器里，而是完整的、可输入命令的 Windows PowerShell 窗口。Codex 脚本通过 UTF-16
   `-EncodedCommand` 传入，避免 `&`、空格和路径被 `cmd.exe` 二次解析。Windows 启动器还会移除父进程可能继承的
   `TERM=dumb`；该值会让 Codex 把控制台误判为不适合交互式 TUI，并显示警告或无法正常渲染审核界面。程序是把
   命令作为 PowerShell 的启动参数传入，而不是伪造用户键盘输入；这样可以自动打开审核入口，同时保留最后的
   授权动作给用户。

#### 5.4 用户在 PowerShell/Codex 中的操作顺序

页面会显示动态生成的完整命令和以下步骤，用户按顺序操作即可：

1. 等待 PowerShell 中的 Codex CLI 界面显示 `Hooks need review`；不要输入 `/hook` 或 `/hooks`。
2. 在当前 PowerShell 窗口选择第 2 项 `Trust all and continue`，不是输入数字 `2`。
3. 按键盘 Enter（回车）确认；不需要先进入 `Review hooks`，也不需要输入 `t`。
4. 回到 code-Manager，点击“刷新状态”，确认 Codex Hook 显示“已信任”。

如果页面仍显示“待信任”，说明 Codex 尚未写入当前 Hook 定义的信任记录，或 Hook 定义已经被修改；重新点击
“添加信任”即可。若页面显示“已关闭”，先手动开启 Codex Hook，再点击“添加信任”。

#### 5.5 其它平台是否需要同样的信任

当前 code-Manager 不会为 Claude Code、Cursor、GitHub Copilot 打开 Codex 式 PowerShell 审核窗口：

- Claude Code、Cursor、GitHub Copilot 只执行各自原生的 Snip Hook 初始化和配置合并。
- 它们不读取 `%USERPROFILE%\.codex\config.toml`，也不使用 Codex 的 `trusted_hash`。
- 如果某个平台自身弹出权限、信任或安全提示，必须按该平台自己的界面完成；code-Manager 不会把这些提示
  当成 Codex 信任，也不会自动确认。

### 6. 停止和删除流程

- “停止”优先读取 `metadata.snip_ownership_v1`：只在目标 JSON 文件中删除目标文件、事件、组/处理器位置、
  Hook 组上下文、完整处理器指纹和命令都仍匹配的那一条 Hook，再按归属删除用户/系统 PATH；不会调用 `init --uninstall`，也不会改写
  Codex `config.toml` 的 Hook 开关或 `trusted_hash` 信任记录。若处理器被人工移动或重新排序，停止保留 `attention`
  并要求手工确认；若处理器已被改写或移除，则保留现状并清除无法再证明的归属。
- 旧版 `owned_agents` 只在该平台恰好存在一条仍指向对应发布目录绝对 `snip.exe` 的 Hook 时恢复为精确账本；
  多条、未知、另一套安装或手工 Hook 都不会被猜测、接管或删除。
- “删除”要求先停止，然后执行同一套 Hook/PATH 清理，最后只删除名称精确为 `Snip` 的专属目录。
- 清理使用结构化 JSON 读写并保留同文件中的其它事件、其它处理器及手工 Snip Hook；由于不调用上游
  `--uninstall`，code-Manager 也不会生成或删除 Snip 的 `.bak` 备份。
- 任一平台卸载失败都会保留 `attention` 状态和错误信息，必须重试清理，不能伪装成“已停止/已删除”。

### 7. 必要 PATH 配置

snip PATH 是本项目的必要启动配置，不应在后续修改中删除或降级为仅当前进程的临时环境变量：

- 启动时把 EXE 同级 `Snip` 目录写入当前用户 PATH 和系统 PATH，而不是只写入当前进程的临时环境变量。
- 写入前后都会按 Windows PATH 分段、大小写不敏感地检查，避免重复添加；停止/删除只移除本程序记录为自己拥有的条目。
- Hook 配置使用绝对 `snip.exe` 路径，因此 Hook 运行不依赖当前进程是否重新加载 PATH；双范围 PATH 仍是本项目
  对手工调用 `snip`、其它外部工具发现 snip 以及不同 Agent 启动环境的必要部署保证。
- 修改注册表后会广播环境变量变更；系统 PATH 写入或删除需要管理员权限时会触发 UAC。
- PATH 查询失败、写入后仍无法读取或只成功写入一个范围，启动会失败并回滚，不会留下半配置。
- 停止、迁移或删除时，受管 PATH 的注册表读取、删除和删除后复查任一步失败都会保留 `attention`；
  程序不会把无法确认的 PATH 当成已经清理，也不会继续删除 `Snip` 目录。

### 8. 状态文件和状态含义

`Snip\\.code-manager-state.json` 只记录 code-Manager 的期望状态和归属，不取代 Agent 的原生配置。主要字段：

- `desired_running`：下次 code-Manager 启动时是否尝试恢复 snip。
- `running`：上一次激活流程是否完整成功；不表示存在 snip 后台进程。
- `user_path` / `system_path`：本次是否由 code-Manager 写入对应 PATH 范围。
- `metadata.snip_ownership_v1`：主归属账本；逐条记录本程序新增 Hook 的目标文件、事件、组/处理器下标、Hook 组上下文、
  完整 JSON 指纹和命令；同一平台可有多个不同目标文件的条目。停止、删除和迁移以它为唯一清理依据。
- `owned_agents`：由精确账本派生的兼容摘要；仅用于旧版状态的安全恢复，不再作为删除依据。
- `metadata.activation_recorded`：区分“用户原本已配置 snip”与“本程序写入的接入”。
- `metadata.trust_notice`：最近一次 Codex 信任检测提示；用于页面恢复待审核说明。
- `metadata.trust_read_error`：读取 Codex 信任记录失败时保存的诊断信息。
- `metadata.last_error`：最近一次失败原因；存在时页面显示 `attention`，成功启动后清除。

若 RTK 和 snip 两个状态文件同时要求恢复，程序保持二者停止并显示冲突，不自动选择任意一方。

### 9. 页面接口

- `GET /api/snip`：返回安装路径、版本、用户/系统 PATH、运行状态、四个平台目录和 Hook 状态；停止状态仍有受管
  账本或 PATH 待处理时额外返回 `cleanup_required=true`，页面显示“清理”；
  Codex 额外返回 `trust_status`、`trust_required`、`trust_command`、`trust_steps` 和
  `trust_shell_open`，供页面展示审核引导。
- `GET /api/snip/releases`：分页读取 snip Release，仅标记有 Windows x64 安装包的版本。
- `POST /api/snip/install`：仅在 snip 已停止时下载、校验并安装选定版本；不会执行 Hook 或 PATH 修改。
- `POST /api/snip/start`：统一扫描四个平台，强制补齐用户/系统 PATH，并只对目标事件中不存在 Snip 命令的 Hook 执行
  官方 `init`；受管 Hook 改写、移动或歧义时不覆盖手工配置，不检查或修改 Codex 信任。
- `POST /api/snip/trust`：只检测 Codex Snip Hook 的信任状态；未信任时打开可见 PowerShell，并返回动态命令
  和“选择第 2 项、按键盘 Enter（回车）”的步骤；已信任时不重复打开窗口。
- `POST /api/snip/stop`：按归属卸载 Hook 并删除本程序拥有的 PATH；不会修改 Codex Hook 开关或
  `trusted_hash` 信任记录。
- `POST /api/snip/uninstall`：先校验已停止，再执行停止清理并删除 `Snip` 专属目录。

### 10. 常见问题排查

**“未检测到可接入的用户级 Agent 目录”**

检查 Codex 的 `%USERPROFILE%\\.codex`、Claude Code 的 `%CLAUDE_CONFIG_DIR%`（未设置时
`%USERPROFILE%\\.claude`）、Cursor 的 `.cursor`、Copilot 的 `.copilot` 是否存在。安装 ChatGPT、Codex 或
其它软件本身，不等于已经创建这些用户级 Agent 配置目录；没有目录的平台会被跳过。

**页面显示目录存在但 Hook 文件不存在**

这是首次接入的正常状态。点击“启动”后由 snip 创建官方目标文件；不要手工写入不符合平台格式的 JSON。
若文件中已经有非本程序受管的 Snip Hook，页面会明确显示，启动不会覆盖它。

**提示 RTK 冲突**

检查 RTK 的“运行中”或 `desired_running` 状态，先停止 RTK，再启动 snip。关闭 rtk.exe 进程不一定等于清除了
RTK 状态文件和 PATH，因此以页面状态和 `/api/rtk` 返回为准。

**页面显示 Codex Hook“待人工确认”**

点击“Codex Hook 信任”区域的“添加信任”按钮，code-Manager 会自动打开一个 PowerShell，并动态执行当前
Codex CLI 的启动命令。不要输入 `/hook` 或 `/hooks`；等待同一个 PowerShell 中出现 `Hooks need review`，
选择第 2 项 `Trust all and continue`，再按键盘 Enter（回车）。不要输入数字 `2`。完成后回到页面点击“刷新状态”，确认显示“已信任”。

如果 `%USERPROFILE%\\.codex\\config.toml` 明确设置 `[features] hooks = false` 或 `codex_hooks = false`，
页面会显示“已关闭”；必须先由用户手动开启，code-Manager 不会覆盖该策略。程序不会自动发送按键、
自动点击确认或直接修改 `trusted_hash`。

**PowerShell 打开了，但命令路径不对**

检查 code-Manager 是否从预期的 EXE 目录启动。`-C` 默认使用当前 `code-Manager.exe` 所在目录，
而 `codex.exe` 优先从 PATH 查找，找不到时从 `%LOCALAPPDATA%\\OpenAI\\Codex\\bin` 的版本目录中
动态选择最近修改的可执行文件。不要把某个版本目录名（例如随机版本哈希目录）写死到脚本或配置中。

**PowerShell 显示 `TERM is set to "dumb"` 或 Codex 窗口空白/立即退出**

这是启动器把当前终端的 `TERM=dumb` 传给 Codex，导致 Codex 交互式 TUI 被降级或停在警告提示。当前版本的
“添加信任”启动器会在创建实体 PowerShell 前自动移除该环境变量；请退出旧版 `code-Manager.exe` 并重新启动
新版后再试。正常窗口应显示 Codex TUI 和 `Hooks need review`，然后选择第 2 项、按键盘 Enter（回车）；不要输入数字 `2`。

**启动后系统 PATH 未配置**

重新点击启动并接受 UAC；若 UAC 被取消，启动会失败并回滚。用户 PATH 与系统 PATH 都是本项目的必要启动配置；
Hook 虽使用绝对路径，手工调用 `snip`、其它外部工具发现以及不同 Agent 的启动环境仍不能缺少这两项部署保证。

**新增了 Cursor/Claude/Copilot 后如何接入**

确保对应用户目录已经生成后，回到页面再次点击 snip“启动”。运行中的 snip 也会复查新增或缺失的平台，不需要先删除
其它平台配置。

### 11. 手工只读验证

以下命令只读取状态，不会修改用户配置：

```powershell
Get-ChildItem -Force "$env:USERPROFILE\\.codex", "$env:USERPROFILE\\.claude", "$env:USERPROFILE\\.cursor", "$env:USERPROFILE\\.copilot"
Get-Content "C:\EXEXX\edit\Snip\\.code-manager-state.json"
$codexDir = Join-Path $env:USERPROFILE '.codex'
Get-Content -LiteralPath (Join-Path $codexDir 'hooks.json') -Encoding utf8
Select-String -LiteralPath (Join-Path $codexDir 'config.toml') -Pattern 'hooks.state|trusted_hash'
& "C:\EXEXX\edit\Snip\\snip.exe" --version
& "C:\EXEXX\edit\Snip\\snip.exe" --help
```

也可以只访问管理页的 `GET /api/snip` 查看 `trust_status`、`trust_required` 和 `agents[].trust`；该接口
不会启动 Codex、不会运行测试命令，也不会修改 Hook 或信任记录。上面示例中的 `C:\EXEXX\edit` 仅是当前
工作区示例，实际部署时应替换为 `code-Manager.exe` 所在目录。

不要在生产用户目录直接运行 `snip init` 或 `--uninstall` 绕过 code-Manager；这样会绕过状态归属记录，
后续页面可能无法判断哪些 Hook 和 PATH 是本程序写入的。需要手工恢复时，先备份四个平台目标文件和状态文件。

### 12. 修改 snip 功能时的必查清单

以后修改 snip 相关代码，不要只改一个平台或只验证启动按钮；至少按下面顺序检查：

1. **平台规范**：核对 `snipAgentSpecs` 的四项目录、目标文件和 `snipInitArgs`；确认 Codex 固定使用
   用户目录 `.codex`、Claude Code 支持 `CLAUDE_CONFIG_DIR`、Cursor matcher 为 `.*`，且 Codex、Claude Code、
   Cursor、Copilot 都有对应项。
2. **状态读取**：检查 `snipAgentStates`、`snipAgentHookPresentAt`、`snipOwnedHookPresenceAt`、`snipStatus`
   返回的 `agents`、`hook_exists`、`configured`、`modified`、`repair_needed`、`cleanup_required` 是否仍与四个平台一致。
3. **启动事务**：检查 `snapshotSnipAgentFiles`、`snipAgentsNeedingInit`、逐平台 `init`、
   `snipHookOwnershipAddedByInit`、手工 Hook 不接管、失败回滚、PATH 回滚和状态写入；不能出现只成功部分平台却标记
   `running=true` 的情况。
4. **运行中复查**：确认 snip 已运行时再次启动仍会扫描新增目录、缺失 Hook、精确受管处理器和用户/系统 PATH，
   而不是直接跳过检查；处理器改写、移动或歧义时不得覆盖手工 Hook，已停止账本也不能重新认领手工 Hook。
5. **停止/删除/迁移**：检查 `metadata.snip_ownership_v1`、`recoverLegacySnipOwnership`、
   `removeSnipOwnedArtifacts`、用户其它 Hook 保留、PATH 删除和 `Snip` 专属目录删除保护。不得重新引入官方
   `--uninstall`、按 EXE 路径批量删除或按文本猜测残留归属。
6. **Codex 特殊规则**：检查 Codex CLI `0.131.0+` 门槛，`config.toml` 的 `[features] hooks`/`codex_hooks`
   明确关闭时仍只提示、不擅自覆盖；检查 `hooks.state.*.trusted_hash` 的规范化哈希比对、
   `trusted/untrusted/unknown/disabled` 状态、动态 Codex 路径、可见 PowerShell 审核窗口和 HTTP 409 待审核响应。
   除页面独立授权操作外，启动、停止、删除和回滚均不得改写 Hook 开关或 `trusted_hash`；绝不能自动发送 `t`
   或模拟确认。
7. **Windows PATH**：检查 `snip_windows.go` 的用户 PATH、系统 PATH、UAC、环境变量广播、重复项检测和失败后的回滚；
   双范围 PATH 是必要启动配置，不能删除或只保留当前进程临时环境。
8. **安装升级**：检查 `snip_release.go` 的 Windows x64 资产筛选、`checksums.txt` SHA-256 校验、临时目录替换和版本标记；运行中不得覆盖 EXE。
9. **页面和文档**：同步检查 `frontend/src/App.vue` 的四平台状态文案、移动端换行，以及本章的路径、命令、状态字段和故障排查说明。
10. **验证和构建**：运行聚焦测试 `go test -run 'TestSnip|TestCodex' .`，再运行
    `npm.cmd run build` 和 `go build .`；不要以 `go test ./service` 等长测试替代这些聚焦验证。

snip 相关文件关系：

- `snip_install.go`：四平台规范、目录/Hook 状态、启动/停止/卸载事务、快照回滚和状态接口。
- `snip_hook_config.go`：Hook 的结构化 JSON 解析、处理器位置/指纹检测和逐条精确清理。
- `snip_ownership.go`：Snip 精确归属账本、旧 `owned_agents` 安全迁移、处理器存在判定和清理边界。
- `snip_codex_hook.go`：Codex CLI 最低版本校验、规范化 Hook 身份和 `trusted_hash` 比对。
- `snip_trust.go`：Codex Hook 信任记录只读检测、固定 `.codex` 目录、Codex CLI 动态定位、动态 `-C` 命令和
  PowerShell 审核流程。
- `snip_trust_windows.go` / `snip_trust_other.go`：Windows 可见新控制台启动实现及非 Windows 平台的明确不支持提示。
- `snip_windows.go`：snip 用户 PATH、系统 PATH、UAC 提权、环境变量广播和 PATH 清理。
- `snip_release.go`：Release 查询、Windows x64 ZIP 下载、`checksums.txt` 校验和安全替换。
- `snip_install_test.go`：Codex 特殊开关、四平台路径、初始化参数、新建 Hook 回滚、旧账本兼容、安装前停机与含空格路径边界测试。
- `snip_ownership_test.go`：四平台逐条账本清理、重复 Hook、人工移动、旧账本迁移、未受管状态和损坏配置保护测试。
- `snip_hook_config_test.go`：Snip 官方 Windows 转义命令解析测试。
- `snip_codex_hook_test.go`：Codex 版本解析、规范化哈希匹配和 Hook 改动后失信任的测试。
- `snip_trust_test.go`：路径规范化、固定 `.codex` 目录及忽略 `CODEX_HOME` 的信任检测测试。
- `frontend/src/App.vue`：snip 页签、四平台 Hook 状态、启动/停止/安装/删除按钮和错误提示。
- `build.bat`：在 CMD 中使用 `npm.cmd` 构建前端，再嵌入 `web/dist` 并生成唯一发布 EXE。
- `Snip\\.code-manager-state.json`：运行期归属和失败状态，不是 Agent 原生 Hook 配置的替代品。


llmtrim 部署手册
-----------------

本章记录 code-Manager 对 llmtrim 的完整管理边界。llmtrim 与 RTK、snip 都由同一个
code-Manager 页面控制，但三者不是同一种工具：RTK 和 snip 主要修改 Agent 的命令处理接入；
llmtrim 是本机 HTTPS 拦截器和请求压缩器，依靠 Windows 环境变量、用户 CA 和本地 daemon
接管流量。以后修改 llmtrim 的任何安装、启动、停止、卸载、恢复或页面代码时，必须同时检查
RTK、snip 的互斥、状态恢复、退出顺序和页面显示，不能把 llmtrim 当成 RTK/snip 的第四个平台
Hook 来实现。

首次成功部署后的实际运行拓扑、已核验状态和目录差异见项目根目录
`LLMTRIM-部署总结.md`；该文件不记录 API Key，可用于日常验收和迁移前核对。

### 1. 组件作用和责任边界

截至 2026-09-01，本工作区随附的 llmtrim 二进制是 `0.13.2`（以 `llmtrim.exe --version`
的实际输出为准；更换安装包后不要继续假定版本不变）。
它本身是 Rust 编译的独立程序，随包提供：

- `llmtrim.exe`：CLI、HTTPS interceptor daemon、压缩管线和诊断命令。
- `llmtrim-tray.exe`：可选的 Windows 托盘界面。它不是 code-Manager 托盘的替代品。
- `LICENSE`、`THIRD-PARTY-LICENSES.md`：llmtrim 的 MPL-2.0 和第三方依赖许可。

llmtrim 自己负责：

- 生成和使用仅限 LLM API 域名的本地 CA。
- 在 `127.0.0.1:43117`（默认端口）运行 HTTPS MITM interceptor。
- 根据客户端请求形状压缩 OpenAI、Anthropic、Google/Gemini 请求，并记录 savings ledger。
- 写入 Windows 用户代理环境、登录自启动和自己的 `.llmtrim` CA/daemon 状态目录。
- 提供 `status`、`doctor`、`ensure`、`update`、`wrap`、`sub`、`agents` 等上游功能。

code-Manager 负责：

- 保存已有配置、首次发现或受管安装确定的 `llmtrim_path`，校验它确实指向 `llmtrim.exe`；管理页面只读展示，
  不提供路径编辑入口。
- 从 GitHub Release 读取 Windows 安装包，下载并校验 SHA-256 后安装到项目目录。
- 调用 llmtrim 的 `setup`、`stop`、`autostart --off`，并确认进程、端口和 Windows 配置。
- 将 llmtrim 的健康状态展示在网页，提供独立的 `status` CMD 窗口。
- 在代理转发链路中，llmtrim 运行时将真实上游 HTTPS 请求经 `http://127.0.0.1:43117` 显式正向代理；
  它负责 MITM、压缩和转发，代理或 CA 失败时拒绝直发。
- 在安装失败、删除、退出和开机恢复时按固定顺序停止进程、清理环境并复查残留。

code-Manager 不负责替代 llmtrim 的压缩算法、账本格式、模型映射、Claude `/sub` 订阅路由、
Claude 子代理生成或上游 API 鉴权；这些仍由 llmtrim 自身命令维护。

### 2. 文件和目录边界

程序始终根据 `code-Manager.exe` 的实际位置计算路径，不把当前工作区路径硬编码为安装要求：

- 项目安装目录：`<code-Manager.exe 同级>\\llmtrim\\`。
- 主程序：`<...>\\llmtrim\\llmtrim.exe`。
- 版本标记：`<...>\\llmtrim\\.llmtrim-version`，内容为当前下载的 Release tag；旧安装首次读取状态时会从
  `llmtrim.exe --version` 兼容补齐。
- 托盘程序：`<...>\\llmtrim\\llmtrim-tray.exe`（若 Release 包含）。
- 上游许可：`<...>\\llmtrim\\LICENSE`、`THIRD-PARTY-LICENSES.md`。
- 受管统计账本：`<...>\\llmtrim\\tracking.db`，以及 SQLite 的 `tracking.db-wal`、
  `tracking.db-shm`。安装时 code-Manager 在受管 `config.toml` 写入绝对 `db_path`，因此新的统计
  不会跟随其它目录的 llmtrim 实例混合；daemon 启动时创建该数据库。
- 旧版默认共享账本：未设置 `db_path` 的 llmtrim 0.13.x 会使用
  `%USERPROFILE%\\.local\\share\\llmtrim\\tracking.db`（若设置 `XDG_DATA_HOME` 则位于
  `%XDG_DATA_HOME%\\llmtrim\\tracking.db`）。这是当前 Windows 用户的默认位置，不随 EXE 一起移动；在
  llmtrim 管理页确认“删除”时，code-Manager 会清理该路径下的 SQLite 三件套。
- 用户状态目录：`%USERPROFILE%\\.llmtrim\\`，包含 CA、daemon 配置和其它 llmtrim 状态；它是
  当前 Windows 用户下 llmtrim 的共享目录，不随 EXE 一起移动。
- 受管拦截主机及统计路径配置：`%USERPROFILE%\\.config\\llmtrim\\config.toml`。保存上游 Base URL
  后由 code-Manager 完全接管 `extra_hosts`；受管安装还会写入 `db_path`。同目录的
  `.code-manager-managed` 是删除时所需的归属标记。
- code-Manager 归属账本：`<...>\\llmtrim\\.code-manager-state.json`，只记录期望运行状态，
  不代替 llmtrim 的原生状态文件。
- 网关配置：`<code-Manager.exe 同级>\\config\\config.yaml` 中的 `llmtrim_path`。

不要把 `C:\\EXEXX\\edit\\llmtrim` 当作固定路径，也不要把用户的 `.llmtrim` 目录、默认共享账本与项目
安装目录混用。手工把另一份 llmtrim.exe 填入 `llmtrim_path` 是允许的；迁移/卸载只会整体删除带有
有效 `.code-manager-state.json` 的 `llmtrim` 专属目录，手工目录和单独存在的 `.llmtrim` 不会被递归删除。

### 3. 上游 CLI 能力和 code-Manager 使用范围

当前二进制的主要命令如下。命令的最终参数以本机 `llmtrim.exe help <command>` 为准：

| 命令 | llmtrim 原生作用 | code-Manager 是否直接调用 |
|---|---|---|
| `setup` | 生成/校验 CA，写入环境与自启动，启动 interceptor，并安装推荐集成 | 启动、安装后初始化、开机恢复 |
| `status` | 输出 savings dashboard 和 daemon -> 端口 -> 环境 -> CA -> 流量健康链 | 网页“显示日志”CMD |
| `start` | 只启动 daemon，不重复完整 setup | 不作为网页启动入口，网页使用 `setup` |
| `stop` | 停止后台 interceptor | 网页停止、安装前、退出和卸载；只针对指定或已验证受管路径 |
| `autostart` / `--off` | 开启或关闭登录自启动；可单独配置 tray | 停止/卸载前执行 `--off` |
| `doctor` / `--fix` | 诊断并修复二进制、端口、环境、CA、账本和集成 | 不自动调用，排错时手工使用 |
| `ensure` | 幂等地恢复推荐状态并处理版本偏差 | 不自动调用，升级后可手工使用 |
| `compress` | 从 stdin 读取 provider 请求并向 stdout 输出压缩 JSON | 不再由网关调用 |
| `serve` | 前台运行 HTTPS interceptor | daemon 模式由 `setup` 管理；code-Manager 将它作为显式 HTTP(S) 正向代理使用 |
| `ca` | 打印或生成本地 CA 路径/内容 | 不调用；安装后由 `setup` 生成 |
| `wrap` | 确认当前 shell 已指向 llmtrim 后再启动 Agent | 不调用；适合手工保证单次会话不漏压缩 |
| `sub`、`agents` | CLIProxyAPI 订阅改道和 Claude 路由子代理 | 不代为配置，避免越权修改用户订阅设置 |
| `tray` | 启动 `llmtrim-tray.exe` | 不自动启动 tray，仅检测并清理残留进程 |
| `update` | 按安装渠道升级并刷新集成 | 页面版本替换由 code-Manager 完成 |
| `uninstall` | 撤销 setup 的环境、自启动、CA、状态和二进制 | code-Manager 使用自己的受管清理事务，不直接依赖该命令 |

43117 daemon 是本项目运行中请求的显式正向代理；`compress` 不参与网关请求链路。不要用 `status` 的退出码替代
code-Manager 的端口+进程确认，也不要用 `start` 代替首次 `setup`，
否则环境和 CA 可能未配置。

### 4. Release 查询、下载和安全安装

网页首次点击 llmtrim 区域的“加载”时，前端请求 `/api/llmtrim/releases?page=1`；成功后按钮显示为
“刷新”，可重新读取第 1 页并替换当前首批版本。只有实际请求期间按钮才显示等待状态；RTK 与 snip
在另一方运行时仍按互斥规则禁用版本控件。后端才访问：

`https://api.github.com/repos/fkiene/llmtrim/releases`

每页最多返回 5 个非 Draft Release；选择“加载更多”才请求下一页。每个版本会标出：tag、名称、
发布时间、是否预发布、当前 Windows 架构是否有安装包以及资产名。列表的 `available` 目前只按
Windows ZIP 资产判断；真正安装时还会再次要求匹配的 SHA-256 校验文件。

Windows 资产按 Go 运行架构选择：

- x64：`llmtrim-x86_64-pc-windows-msvc.zip`。
- arm64：`llmtrim-aarch64-pc-windows-msvc.zip`。

安装时后端还要求同一 Release 提供匹配的 SHA-256 文件（接受去掉 `.zip` 的 `.sha256` 或带
`.zip.sha256` 的命名）。实际过程是：

1. 关闭 llmtrim 状态 CMD，并只停止带 code-Manager 状态文件的当前/旧安装目录中的 daemon、tray。
2. 发现受管旧目录后，才清理该实例写入的 Windows 环境、用户 CA、自启动、项目目录、用户状态目录和
   可识别的旧安装目录；没有归属证据时保留共享 `.llmtrim` 与手工配置。旧版默认共享账本不会在升级安装时
   自动删除，以免无提示地丢失历史。
3. 以不使用系统 HTTP_PROXY/HTTPS_PROXY 的独立直连客户端读取 Release JSON、下载 ZIP 和校验文件。
4. 将 ZIP 下载到临时文件，限制最大读取量为 256 MiB；校验文件限制为 16 KiB。
5. 提取 64 位十六进制 SHA-256，计算 ZIP 实际摘要；不一致立即拒绝安装。
6. 先解压到项目同级临时目录，扁平化文件名，拒绝路径穿越、隐藏文件、重复 basename、空包以及
   缺少 `llmtrim.exe` 的压缩包。
7. 校验通过后整体替换名称精确为 `llmtrim` 的项目目录，而不是逐个覆盖运行中的文件。
8. 写入 `.llmtrim-version` 和 `config.yaml` 的 `llmtrim_path`，在受管 `config.toml` 写入
   `extra_hosts` 与安装目录的 `db_path`，执行 `setup`，校验 Windows 配置，再确认 daemon 已监听。

安装的总超时约为 3 分钟；单次 `setup` 命令最多等待 45 秒。安装前已经迁移受管旧实例并清理其环境；
因此下载、校验或解压阶段失败时不会自动恢复旧版本，只能重新选择版本安装。进入新目录后的
`.llmtrim-version`、`llmtrim_path` 保存失败、setup 失败、Windows 配置校验失败或 daemon 未就绪时，程序会尝试停止进程、
删除本次目录并清空 `llmtrim_path`，不会把半安装状态返回为成功。安装会先写入初始停止状态和受管安装记录；
这一步失败会回滚安装。setup 成功后的“运行中”状态写入失败只记录警告并保留已经启动的 daemon；下次开机
可能无法自动恢复。

### 5. `llmtrim_path` 配置和首次发现

`config.yaml` 的 `llmtrim_path` 必须是绝对路径或可解析为绝对路径的 `llmtrim.exe` 路径，文件名
必须大小写不敏感地等于 `llmtrim.exe`，且不能是目录。程序启动时会加载并校验该路径；填写了失效
路径会直接阻止 code-Manager 启动，避免页面看似可用但实际无法控制 daemon。

若字段为空，程序只在首次启动时枚举当前进程快照：

- 找到正在运行的 `llmtrim.exe` 后，读取进程真实可执行路径并写回 `config.yaml`。
- 找不到运行中的进程时保持为空，不会扫描磁盘、不会猜测安装位置，也不会自动下载。
- 一旦字段已有值，后续启动不再做进程发现；需要换版本或位置时通过页面安装或手工修改后重启。

管理页面只读展示 `llmtrim_path`，启动、停止和日志操作均从服务端当前配置读取该路径，不接受浏览器提交的
替代路径。正常换版本使用受管安装流程；若 `config.yaml` 已记录失效路径而导致程序无法启动，需先手工修复或
清空该字段，再启动 code-Manager 完成安装。

### 6. `setup` 产生的 Windows 配置副作用

llmtrim 的 `setup` 不是单纯拉起一个进程，而是一组幂等配置动作。当前 code-Manager 通过
`verifyLLMTrimWindowsSetup()` 明确校验以下用户级配置：

- `HKCU\\Environment\\HTTPS_PROXY` = `http://127.0.0.1:43117`。
- `HKCU\\Environment\\HTTP_PROXY` = `http://127.0.0.1:43117`。
- `HKCU\\Environment\\NO_PROXY` = `localhost,127.0.0.1,::1,10.0.0.0/8,172.16.0.0/12,192.168.0.0/16,169.254.0.0/16,fd00::/8,*.local`。
- `%USERPROFILE%\\.llmtrim\\ca.pem` 存在。
- `HKCU\\Environment\\NODE_EXTRA_CA_CERTS` 指向该 CA（允许实际绝对路径或
  `%USERPROFILE%\\.llmtrim\\ca.pem` 形式）。
- `HKCU\\Environment\\NODE_USE_ENV_PROXY` = `1`。

setup 还可能写入：

- 当前用户 `HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run\\llmtrim`。
- 当前用户同路径下 `llmtrim-tray` 的启动项。
- Windows `StartupApproved\\Run` 对应值。
- 当前用户 Root 证书库中的 `llmtrim local CA`。
- `%USERPROFILE%\\.llmtrim\\` 内的 daemon 配置和 CA 状态。

受管安装的 savings ledger 不在上述 `.llmtrim`：它由 `db_path` 固定为安装目录的
`llmtrim\\tracking.db`，其 WAL/SHM 文件也位于同一目录。

这些是 llmtrim 的环境接管，不是 RTK/snip 的 PATH，也不是四个平台 Agent Hook。code-Manager
只在用户点击启动或开机状态要求恢复时调用 setup；安装按钮本身在 setup 成功前不会宣称已激活。
系统已有不同代理值时，setup 可能覆盖它们；停止/卸载只删除与 llmtrim 目标值匹配的项目，不会盲删
其它代理地址，但使用前应先备份重要的用户代理配置。

写入注册表后由 code-Manager 广播 `WM_SETTINGCHANGE(Environment)`，使新启动的进程读取新环境。
已经打开的 Codex、Claude Code、Copilot、Cursor、VSCode 或终端不会自动刷新进程环境块；需要重启
这些程序或新开终端。llmtrim 的 HTTPS 拦截依赖 CA 信任，证书被删除、过期或被安全软件拦截时，
daemon 可能仍在监听但 HTTPS 请求会失败，应优先运行 `llmtrim doctor`。

### 7. daemon、端口和健康判定

code-Manager 固定把 `127.0.0.1:43117` 作为 llmtrim daemon 地址。网页“运行中”不是只看进程名，
而是同时满足：

1. `llmtrim_path` 对应的可执行文件存在。
2. 进程枚举能找到同一路径的 `llmtrim.exe`。
3. TCP `127.0.0.1:43117` 能建立连接。

状态判定的典型消息：

- 没有配置路径：尚未接管 `llmtrim.exe`。
- 没有对应进程且端口关闭：daemon 未监听 43117。
- 没有对应进程但端口打开：43117 被其它程序占用，不能确认是当前 llmtrim。
- 进程存在但端口未开：llmtrim 正在启动、启动失败或被防火墙/安全软件阻止。
- 进程和端口都匹配：当前配置路径的 llmtrim daemon 已就绪。

端口被其它程序占用时，code-Manager 不会强行把它当成健康 llmtrim，也不会自动换到另一个端口，
因为网关压缩和环境变量都固定指向 43117。应先查占用者并释放端口，再重试 setup。

### 8. 网关请求链路和 llmtrim 正向代理策略

网关启动代理后，`/v1/` 下只接受 GET、HEAD、POST。每条请求都会先检查 `llmtrim_path` 对应 daemon
和 43117 端口：

1. llmtrim 未运行时，使用既有基线客户端。`outbound_proxy` 非空时保持原 HTTP/SOCKS5 出站路径；为空时
   保持 H2/H3 预热、保活和连接复用。
2. llmtrim 运行时，GET、HEAD、POST 都使用独立的标准 Go HTTP Transport，并显式设置
   `Proxy=http://127.0.0.1:43117`。请求保留真实上游 URL、路径、请求头、Authorization 和正文；HTTPS
   经 CONNECT 隧道交给 llmtrim，由它识别目标主机、MITM、压缩并转发。
3. 该 Transport 专用加载 `%USERPROFILE%\\.llmtrim\\ca.pem` 到信任池，且不读取进程的
   `HTTP_PROXY` / `HTTPS_PROXY`。CA 缺失或无效、代理断连、CONNECT/TLS 失败时，当前请求返回错误，绝不
   回退为基线路由。
4. 自动重试仍只针对配置命中的上游 HTTP 状态码。可重试 POST 缓存原始请求体；每次上游尝试重新经
   llmtrim 正向代理，不存在缓存“转换后 JSON”或再次 POST 到 43117 根路径的行为。
5. llmtrim 停止、页面停止或删除时会关闭并丢弃该专用客户端的空闲连接。进行中的代理请求保留其明确的
   成功或失败结果；后续请求才恢复基线路由。

这意味着“llmtrim 显示运行中”要求本地 CA 和显式代理同时可用，而不仅是进程与端口存在。网关日志记录
内部请求 ID、路由选择和耗时，不记录请求正文、压缩内容、API Key 或 CA 私钥。

### 9. 网页控件、接口和截图对应关系

llmtrim 页签的控件与后端接口一一对应：

| 页面控件 | 接口/命令 | 行为 |
|---|---|---|
| “加载”/“刷新” | `GET /api/llmtrim/releases?page=N` | 从 GitHub 读取 5 个版本；刷新会重新读取第 1 页并替换当前首批版本，不安装、不改本机 |
| 版本下拉框 | 前端本地状态 | 优先选择有 Windows 包的版本；安装阶段还会强制检查校验文件 |
| “安装” | `POST /api/llmtrim/install` | 停止旧实例、清理、下载校验、替换目录、setup、确认 daemon |
| “删除” | `POST /api/llmtrim/uninstall` | 停止进程、关闭自启动，删除受管安装目录中的统计库与历史默认共享 `tracking.db`/`-wal`/`-shm`，再删除受管环境/CA/状态/项目目录和带归属标记的 `%USERPROFILE%\\.config\\llmtrim`，并复查 |
| “Daemon 状态” | `GET /api/llmtrim` | 只读显示配置路径、版本、进程、端口、tray、安装/状态目录和诊断消息 |
| “启动” | `POST /api/llmtrim/start` -> `setup` | 使用服务端已保存的 `llmtrim_path` 恢复环境、CA、自启动和 daemon；等待进程+43117 |
| “停止” | `POST /api/llmtrim/stop` -> `autostart --off`、`stop` | 先关闭状态 CMD，再停止 daemon/tray，确认端口退出 |
| “显示日志” | `POST /api/llmtrim/logs/show` | 新开独立 CMD，执行 `.\\llmtrim.exe status` |
| “关闭日志” | `POST /api/llmtrim/logs/hide` | 发送 Ctrl+C，再 taskkill CMD 子树，不影响 daemon |
| “刷新状态” | `GET /api/llmtrim` | 读取路径、进程、端口、tray、安装目录、状态目录、环境残留 |

截图中“代理已停止”和“llmtrim 已停止”是两个独立状态：顶部代理开关控制 code-Manager 是否监听
并转发 `/v1/`，llmtrim 页签只控制压缩器。启动 llmtrim 不会自动启动网关代理；启动代理也不会自动
启动 llmtrim。若 llmtrim 已进入强制状态而代理仍未启动，客户端访问 `/v1/` 仍会得到 503，这是预期的
安全行为。

前端会每秒静默轮询状态，并持续回填只读状态卡。启动/停止请求最多等待 50 秒；即使 HTTP 控制请求超时，
页面仍继续轮询真实进程和端口，不能仅凭一次请求超时就重复点击安装或启动。窄屏下安装行和状态卡使用
响应式布局，修改控件时必须同时检查桌面和移动端的按钮禁用、长路径换行及错误文案。

### 10. 安装、启动、停止、退出和卸载顺序

**安装**

安装是“迁移受管旧版本并立即 setup”的事务，不是单纯解压。安装前会关闭状态窗口，只处理已验证属于
code-Manager 的旧 daemon/tray、自启动和环境；未受管实例占用 43117 时会返回错误，不会被强制结束。
安装成功后一定会执行 setup 并等待 43117。若 setup、Windows 配置校验、
daemon 启动或路径保存失败，后端会尝试回滚本次目录、环境、CA、状态和 `llmtrim_path`；下载阶段失败
时旧版本不会自动恢复。安装结束后的状态账本写入失败只记录警告，不会杀掉已健康的 daemon。

**网页启动**

网页启动只接受 `setup`，不会只调用 `start`。成功条件是命令返回成功、进程路径匹配、43117 已监听。
成功后写入 `llmtrim\\.code-manager-state.json`：`desired_running=true`、`running=true`。路径会在 setup 前先保存到 config.yaml；如果独立启动的 setup 失败，路径仍可能
保留，需要停止后重新安装；若失效路径已经阻止页面打开，则先手工修复 `config.yaml`，不能把一次失败理解为自动回滚。状态账本写入失败只记录日志，
不会把健康 daemon 杀掉，但下次开机可能无法自动恢复。

**网页停止**

停止先关闭独立 status CMD，再获取互斥锁；优先使用当前运行进程的真实路径调用 `autostart --off` 和
`stop`，命令失败时仍会按进程名结束 `llmtrim.exe`、`llmtrim-tray.exe`，等待最多 8 秒并复查 43117。
确认退出后会关闭专用正向代理客户端的空闲连接，清除 llmtrim 当前用户自启动和本程序写入的用户环境变量，并写入
`desired_running=false`。停止不会删除 CA、安装目录或用户状态目录；需要彻底清理请用“删除”。

**code-Manager 退出**

托盘退出、网页“停止并退出”和窗口关闭共用同一顺序：阻止新代理请求 -> 等待已进入的请求完成 ->
停止 llmtrim 并清理其当前用户自启动/用户环境 -> 清理 RTK PATH 与四个平台接入 -> 清理 snip PATH 与本程序拥有的
四个平台 Hook -> 停止网关代理 -> 关闭管理 HTTP 服务。总清理上下文最长约 45 秒。

网页“停止并退出”以及删除 RTK、snip、llmtrim 都通过同一个 Vue 顶层确认遮罩完成，不调用浏览器原生确认框；遮罩不会因点击背景或按 Esc 关闭，
只能选择“确定”或“取消”，取消后焦点会回到触发按钮。确认退出后立即停止状态轮询、取消可取消的启动操作，并以顶层遮罩锁住所有编辑、保存、启动、停止和页签切换。
`POST /api/application/exit` 会等待上述受管资源清理完成，再返回 `completed`、`clean`、
`message` 和可选 `warnings`。页面仅在 `clean=true` 时显示“已停止并退出”；响应刷出后才关闭管理 HTTP 服务和托盘循环，
因此当前已加载页面会保留这个居中的终态提示。手动刷新、关闭或跳转浏览器页面后无法继续显示该提示，因为本地 HTTP 服务已停止。
若清理有失败，页面显示不可操作的告警终态，并列出失败步骤；主进程仍会继续退出。

退出 code-Manager 不等于卸载 llmtrim；CA、安装目录和 `.llmtrim` 状态会保留。

code-Manager 启动后会额外启动一个脱离主进程树的退出清理助手。助手等待主进程结束；即使主程序被任务管理器
强制结束或发生崩溃，也会继续执行上述 RTK、snip、llmtrim 清理。若用户已经快速启动了新的 code-Manager 实例，
助手会让新实例接管状态，避免旧实例清理误伤新实例刚恢复的配置。清理失败会记录到 EXE 同目录的
`code-Manager-cleanup.log`，不会因此删除用户未归 code-Manager 所有的配置内容。

**卸载**

删除接口会停止 daemon/tray，删除用户和系统范围的 `llmtrim`/`llmtrim-tray` 自启动项，清理匹配的
HTTP/HTTPS/NO_PROXY、Node CA 和 Node 环境变量，移除当前用户 Root 中名为 `llmtrim local CA` 的证书。
受管安装目录删除时会同时删除其 `tracking.db`、`tracking.db-wal`、`tracking.db-shm`；管理页确认删除还会
清理旧版默认共享目录中的同名 SQLite 三件套。随后删除项目 `llmtrim` 目录、
外部配置路径所在的独立 `llmtrim` 目录、`%USERPROFILE%\\.llmtrim`，以及带 `.code-manager-managed`
归属标记的 `%USERPROFILE%\\.config\\llmtrim`，清空 `config.yaml` 的 `llmtrim_path`，删除 code-Manager
状态文件，最后复查进程和端口。配置目录缺少正确归属标记、统计数据库被占用或删除失败时，卸载直接失败，
不会返回伪成功。

卸载中的 HKLM 自启动项实际存在且需要管理员权限时才会触发隐藏 PowerShell UAC；取消 UAC、注册表清理失败、证书仍
存在、目录仍存在、进程仍残留、统计数据库被占用或 43117 仍被占用都会返回失败/残留状态。删除是不可逆操作；
上游 CLI 的 `llmtrim uninstall` 默认还会保留 savings ledger，而 code-Manager 的彻底删除会移除受管
`llmtrim\\tracking.db` 及旧版默认共享账本，因此若要保留统计，应先复制这些 SQLite 文件后再点击网页“删除”。

### 11. 状态文件、互斥和开机恢复

`llmtrim\\.code-manager-state.json` 是 code-Manager 的小型账本，字段来自 `managedToolState`：

- `desired_running`：用户最后一次启动/停止意图，决定开机恢复是否尝试 setup。
- `running`：最近一次控制操作是否确认 daemon 运行；每次网页状态查询仍会重新探测。
- `updated_at`：UTC 更新时间。
- `metadata.last_error`：恢复或清理失败时的 attention 原因（若存在）。

code-Manager 启动时先读取 RTK、snip、llmtrim 三份账本，再按各自内部函数恢复。RTK 与 snip 同时
要求恢复时保持二者停止并显示冲突；llmtrim 不属于 RTK/snip 的命令过滤互斥，可以独立恢复。开机流程
只有在 llmtrim 状态要求恢复、配置路径有效且 daemon 已确认监听 43117 后，才会启动网关代理；llmtrim
恢复失败不会启动代理，管理页面和托盘仍保持可用。

code-Manager 自身退出时会停止 llmtrim，即使 `desired_running=true`；下次开启了 `startup_enabled`
后才会按账本自动恢复。关闭浏览器页面不会触发退出，也不会改变 `desired_running`。

### 12. 日志、诊断和常见问题

**页面显示“已安装/已停止”**：只代表 `llmtrim.exe` 文件存在且状态查询没有确认 daemon；检查
`llmtrim_path`、43117 端口、`llmtrim status` 和 `llmtrim doctor`。

**显示“进程存在但端口未监听”**：daemon 可能仍在启动，也可能 setup 失败。先等待几秒，再在独立
PowerShell 中运行 `& <llmtrim.exe> status`；查看 `config\\code-Manager.log` 中的 setup 错误，不要连续点击启动。

**显示“43117 被其它进程占用”**：确认占用者后释放端口。不要自行把环境变量改到另一个端口，除非同步
修改 llmtrim 配置、code-Manager 常量、健康检查和网关正向代理链路。

**HTTPS 请求证书错误**：检查 `%USERPROFILE%\\.llmtrim\\ca.pem`、Root 证书中的 `llmtrim local CA`、
`NODE_EXTRA_CA_CERTS`，然后运行 `llmtrim doctor --fix`。已打开的 Agent/终端需重启读取环境。

**启动后请求返回 502**：通常表示 daemon 虽显示运行中，但 `%USERPROFILE%\\.llmtrim\\ca.pem` 缺失、
无效，或 43117 的 CONNECT/MITM 链路失败。先恢复 43117 与 CA，再重试；运行中不会静默绕过 llmtrim 直连上游。

**删除按钮不可用或提示残留**：页面只有在 `residual=true` 时启用删除；残留可能来自进程、tray、项目目录、
受管 `.llmtrim`、`llmtrim_path`、用户/HKLM 自启动或匹配环境变量。单独存在的共享 `.llmtrim` 不会再被
误判为 code-Manager 残留。先刷新状态；不要直接删除状态文件。

**显示日志窗口无法关闭**：该窗口运行的是独立 `cmd.exe` 的 `llmtrim.exe status`，关闭动作会先发 Ctrl+C，
再按 PID 结束子树。它与 daemon 进程分开，关闭窗口不会停止 43117；必要时可手工结束对应 CMD，但之后应刷新页面。

只读诊断示例：

```powershell
$llmtrim = 'C:\\path\\to\\llmtrim\\llmtrim.exe'
& $llmtrim --version
& $llmtrim --help
& $llmtrim status --quiet
& $llmtrim status --json
& $llmtrim doctor
& $llmtrim ca
Get-Item $llmtrim
Get-Item (Join-Path $env:USERPROFILE '.llmtrim') -Force
[Environment]::GetEnvironmentVariable('HTTP_PROXY', 'User')
[Environment]::GetEnvironmentVariable('HTTPS_PROXY', 'User')
[Environment]::GetEnvironmentVariable('NO_PROXY', 'User')
[Environment]::GetEnvironmentVariable('NODE_EXTRA_CA_CERTS', 'User')
[Environment]::GetEnvironmentVariable('NODE_USE_ENV_PROXY', 'User')
Get-Content 'C:\\path\\to\\code-Manager\\llmtrim\\.code-manager-state.json'
Test-NetConnection 127.0.0.1 -Port 43117
```

这些命令只读文件、环境和端口；不要在生产目录直接运行 `setup`、`autostart --off` 或 `uninstall`，
除非你明确要绕过 code-Manager 的状态账本和回滚机制。

### 13. llmtrim 与 RTK、snip、四个平台的关联规则

三种工具的关系必须按“同一管理器、不同接管层”理解：

- **RTK**：为 Codex 写入 Markdown 规则段，为 Claude Code、GitHub Copilot、Cursor 写入官方 Hook，并维护 RTK PATH；没有 daemon。
- **snip**：写入四个平台的原生 Hook，并维护 Snip PATH；没有 daemon。
- **llmtrim**：通过 HTTPS_PROXY、CA 和 43117 interceptor 接管网络流量；不写入本项目定义的四平台
  RTK/snip Hook，也不依赖四个平台 Hook 才能压缩网关请求。

因此修改 llmtrim 时必须联动检查：

1. RTK/snip 是否仍能独立启动、停止、恢复和卸载；llmtrim 不应误触发它们的状态文件或 PATH 清理。
2. 四个平台的 Agent 会话是否需要重启才能看到 llmtrim 的新环境；不要把“Hook 未配置”误报为 llmtrim 失败。
3. RTK/snip 与 llmtrim 同时启用时，命令输出过滤和网络请求压缩是否叠加且不互相绕过；llmtrim 运行时的
   代理或 CA 失败必须保留拒绝直连的策略。
4. code-Manager 退出顺序是否仍能取消代理请求、关闭上游会话和监听，再停止 llmtrim；不能在 daemon 转发仍被活动请求使用时关闭网关。
5. 开机恢复是否只在 llmtrim 端口健康后启动代理；RTK/snip 的 `desired_running` 恢复不能替代 llmtrim 健康检查。

### 14. 修改 llmtrim 功能时的必查清单

以后修改 llmtrim 相关代码，至少按下面顺序核对：

1. **上游 CLI**：用随项目二进制的 `--help` 重新确认 `setup`、`stop`、`autostart`、`status`、`doctor`、
   `compress` 参数，不凭旧版本记忆写死行为。
2. **Release**：检查 GitHub API、分页、Windows x64/arm64 资产名、SHA-256 文件名、直连客户端、下载大小限制和 ZIP 安全解压。
3. **路径与统计库**：检查 `llmtrim_path` 校验、进程真实路径发现、项目目录动态计算、受管
   `db_path=llmtrim\\tracking.db`、旧版默认共享账本的 SQLite 三件套，以及外部 `llmtrim` 目录删除保护。
4. **Windows 环境**：检查 HTTP/HTTPS/NO_PROXY、Node CA、CA 信任、自启动注册表、HKLM UAC、环境广播和只删除匹配值的规则。
5. **daemon 健康**：检查进程路径、43117 TCP 端口、tray 进程、启动等待、停止等待、端口冲突和残留检测。
6. **请求安全**：检查 64 MiB 请求体限制、显式 `Proxy=http://127.0.0.1:43117`、CA 信任池、CONNECT、
   原始 POST 重试、基线路由切换、出站代理和错误状态码。
7. **事务回滚**：安装、setup、路径保存、Windows 校验、状态写入任一步失败时，确认旧环境不会被误删且新环境不会残留。
8. **状态恢复**：检查 `.code-manager-state.json`、`desired_running`、attention、开机启动和退出顺序；不得把一次查询结果当永久事实。
9. **页面和移动端**：检查版本加载、安装/删除禁用条件、长路径输入、日志按钮、50 秒超时后的继续轮询，以及窄屏布局。
10. **四平台关联**：同时检查 RTK/snip 的四个平台状态、互斥、PATH 和 Agent 重启提示；llmtrim 不得修改它们的归属文件。
11. **文档同步**：同步更新本章、配置示例、工程树、接口索引和排错命令，保持“关联管理但接管层独立”的口径。
12. **最小验证**：优先运行 llmtrim CLI 的只读 `--version`、`--help`、`status --quiet`、`doctor`，再运行相关 Go 单元测试、
    `npm.cmd run build` 和 Windows EXE 构建；不要运行 `go test ./service` 等长测试。

对应代码关系：

- `llmtrim_release.go`：Release API、分页、Windows 资产选择、直连下载、SHA-256 和 ZIP 安全替换。
- `llmtrim_install.go`：安装、停止、卸载、Windows 环境/CA/自启动清理、回滚和配置校验。
- `llmtrim_config.go`：受管 `extra_hosts`、`db_path`、UTF-8 TOML 写入，以及共享 SQLite 账本路径和清理保护。
- `main.go`：llmtrim 状态 API、setup/stop 控制、daemon 探测、日志 CMD、网关正向代理路由和退出顺序。
- `tool_state.go`：`llmtrim\\.code-manager-state.json` 的期望状态和 attention 记录。
- `managed_tool_installations.go` / `managed_tool_migration.go`：受管目录安装索引和发布目录迁移；只接受
  有效状态文件作为自动删除 PATH、提示词、Hook、CA 或用户状态的归属证据。
- `frontend/src/App.vue`：llmtrim 页签、版本列表、路径、启动/停止/安装/删除/日志控件和轮询。
- `frontend/src/style.css`：llmtrim 控件、状态徽章、安装行和移动端布局。
- `llmtrim\\llmtrim.exe`、`llmtrim\\llmtrim-tray.exe`：实际上游二进制，不在 Go 源码中实现压缩算法。

状态文件：

- `RTK-AI\.code-manager-state.json`
- `Snip\.code-manager-state.json`
- `llmtrim\.code-manager-state.json`

每个文件记录上一次按钮操作的 `desired_running`、更新时间和 code-Manager 拥有的 PATH/Hook/提示词
修改信息。code-Manager 手动启动和 Windows `--startup` 启动都会读取它们，并调用与网页按钮相同的内部
启动函数恢复。若 RTK 和 snip 的状态文件同时要求恢复，程序不会自动启动任意一方，页面显示冲突；
`startup_enabled` 只控制 code-Manager 的 Windows 开机启动，不会替代这些状态文件。


启动与停止
----------

直接双击 code-Manager.exe 即可启动，程序会自动打开浏览器页面，并在 Windows 通知区域
显示由 297763_sort-by-icon.svg 生成的 code-Manager 图标。

右键单击托盘图标可以：

- 选择“打开网页”：再次用默认浏览器打开本地页面。
  - 选择“退出 code-Manager”：先停止接收新的代理请求，等待正在处理的请求结束，
  再停止 llmtrim 并清理其自启动/用户环境变量，清理 RTK 和 snip 的 PATH/平台接入，
  最后关闭代理转发、HTTP 监听并退出程序。整个清理流程最多等待 45 秒；异常退出时由独立清理助手接续处理。

网页的代理状态卡片下方提供“启动代理/停止代理”和“显示日志/关闭日志”。“显示日志”会通过 Windows 原生独立控制台打开
CMD 窗口镜像本次运行的 `code-Manager.log`，只显示 `/v1/*` 实际代理请求及其耗时；管理页面的
`/healthz`、`/api/proxy`、`/api/llmtrim` 状态查询不会写入该日志。关闭日志按钮只关闭该日志窗口，
不会停止代理、llmtrim 或 code-Manager。用户手动关闭 CMD 后，网页会在下一次状态同步时恢复“显示日志”。

llmtrim 页签的状态徽章前提供独立的“显示日志/关闭日志”按钮。它不复用 code-Manager 日志窗口：打开后
使用 Windows 原生新控制台启动独立 `cmd.exe`，将当前目录设为配置的 llmtrim.exe 所在目录，并执行
`cmd.exe /d /k "chcp 65001>nul & title llmtrim status & .\\llmtrim.exe status"`。
该 CMD 拥有独立的真实标准输入和输出，llmtrim 的状态内容直接显示在窗口内。
关闭按钮会先向该独立控制台发送 Ctrl+C 让 status 停止，再关闭 CMD 及其子进程；手动关闭 CMD 后页面会在
下一次状态同步时恢复“显示日志”。

若需要启动程序并查看日志，请直接双击 EXE 后，在网页中点击“显示日志”。日志文件位于：

<code-Manager.exe 同级>\config\code-Manager.log

正式 EXE 使用 GUI 子系统，双击后不会自动显示控制台日志；需要查看代理请求时使用网页“显示日志”。
关闭浏览器页面不会停止程序。

开启“开机启动”后，Windows 在用户登录后调用 `code-Manager.exe --startup`。程序等待 8 秒，
后台运行关闭时先打开网页面板；只有 llmtrim 状态文件要求恢复且当前配置路径的 llmtrim.exe 已存在、
43117 端口已监听时，才认定 llmtrim 恢复成功并启动代理。恢复失败时不会启动代理，程序仍保留托盘和
管理页面供手动处理。

健康检查地址：

http://127.0.0.1:7780/healthz


重新构建
--------

修改 Go 或 Vue 源码后，运行：

双击：

  C:\EXEXX\edit\build.bat

`build.bat` 全程在 CMD 中执行：使用 npm.cmd 安装前端依赖并构建 Vue 页面，再使用 Windows Edge
的无界面渲染将 297763_sort-by-icon.svg 转为嵌入 EXE 的 Windows 托盘图标。最终只生成：

  C:\EXEXX\edit\releases\code-Manager\code-Manager.exe

发布目录不复制 Go/Vue 源码、web/dist、图标源文件、assets 或前端 node_modules；运行所需的前端页面、
托盘 ICO、完整 RTK 命令参考、Codex 常驻规则和 `uninstall.bat` 模板都会编入这一个 EXE。首次正常运行时，EXE 会在
同级目录释放 `uninstall.bat`；该脚本用于停止并卸载本程序及其自身创建的运行目录，完成后会删除自己。它会
检查同级 `code-Manager.exe` 和开发目录特征，拒绝在包含源码的开发目录中执行，不需要 `.release` 标记文件。

正式 EXE 使用 Windows GUI 子系统构建，双击不会自动显示控制台；需要查看代理请求时，在网页中
点击“显示日志”。

注意：本项目在 Windows + VSCode 环境中开发，不是 Git 仓库。前端 npm 命令统一使用
npm.cmd；后端使用 Go 工具链的 go build。正式发布入口为 build.bat，不依赖 PowerShell。
Go 1.27 的默认 HTTP/2 包装层会拒绝扩展 CONNECT 的 `:protocol`；正式 `build.bat` 已固定传入
`-tags http2legacy`，以使用 x/net 的原生 HTTP/2 实现。手动执行 Go 的运行、构建或聚焦测试时也必须带上
该标签；它是内部编译选择，不是配置字段、环境变量或用户可见的协议开关。


工程导图和维护索引
====================

下面内容用于快速定位文件、函数、接口和运行链路。后续排查问题时，优先按本索引进入
相关入口，不需要每次从整个目录开始扫描。


一、项目范围和总体结构
------------------------

项目目标：在 Windows 本机提供一个 OpenAI 兼容的本地 HTTP/WS 网关。客户端访问
config.yaml 中的 listen_address；本地监听始终是 HTTP/1.1，并且 `/v1/*` 默认接受标准 WebSocket Upgrade。
llmtrim 启动后，网关把请求交给 43117 处理，不直接绕过 llmtrim 连接上游。

核心运行链路：

  本机客户端（HTTP/1.1 或 WebSocket Upgrade）
    -> http://<listen_address>/v1（以 config.yaml 为准）
    -> code-Manager 代理转发开关
    -> （llmtrim 运行）HTTP CONNECT 到 127.0.0.1:43117
       -> 使用 llmtrim CA 完成 TLS
       -> HTTP/1.1 WebSocket Upgrade 或普通 HTTP 请求
       -> llmtrim / 上游；Upgrade 拒绝原样回传，不直连绕过
    -> （llmtrim 未运行）H2/H3 并发 TLS 预握手
       -> upstream_websocket_enabled=true 且上游扩展 CONNECT 可用：WS 帧双向原样复制
       -> 上游首个 SETTINGS 未宣告 ENABLE_CONNECT_PROTOCOL=1，或扩展 CONNECT 返回 501：
          原请求语义通过普通 H2/H3 双向流承载原始 WS 帧
       -> 普通 HTTP 先尝试 WS 二进制帧承载标准 HTTP/1.1 报文，仅上述两种能力缺失情形才回落普通 H2/H3
       -> 其它握手响应（400/401/403/404/405/426/429/502/503 等）原样返回，不发送第二条业务请求
       -> 可选 HTTP/SOCKS5 出站代理上的 HTTP/1.1 WebSocket 建连
       -> upstream_base_url

HTTP-over-WS 与 WS-over-普通 H2/H3 流是本项目固定的底层承载设计：前者把标准 HTTP/1.1 线协议报文放入
WS 二进制帧，后者以普通双向流承载原始 WS 帧。它们不引入 MASQUE、额外 Base URL、额外端点、JSON 包装、
私有协商头或新的代理协议配置。

启动链路：

  code-Manager.exe
    -> Windows 单实例互斥体
    -> 创建 config\code-Manager.log
    -> 读取 config\config.yaml
    -> 读取 RTK-AI、Snip、llmtrim 各自的 .code-manager-state.json
    -> 仅对 desired_running=true 的工具调用与网页“启动”相同的恢复函数
    -> 固定绑定管理页面 127.0.0.1:7780
    -> 加载完整配置和 HTTP 客户端
    -> 注册 HTTP 路由
    -> 创建托盘菜单
    -> 启动管理 HTTP Server
    -> 打开 http://127.0.0.1:7780

网页点击“启动代理”后，代理 HTTP Server 会先监听配置地址，再等待上游 H2/H3 连接预热并接收 SETTINGS；
只有握手成功才显示“代理运行中”。点击“停止代理”会同时关闭本地 HTTP 监听、已 Hijack 的 WS 连接和全部上游连接，
不会停止 RTK、snip 或 llmtrim。

构建链路：

  frontend/src/*
    --npm.cmd run build-->
  web/dist/*
    --go:embed frontendFS-->
  code-Manager.exe

  297763_sort-by-icon.svg
    --Edge headless 截图-->
  assets/tray.png
    --go run tools/icon-to-ico.go-->
  assets/tray.ico
    --go:embed trayIcon-->
  code-Manager.exe

  assets/RTK-Codex-commands.md
    --go:embed rtkCodexCommands-->
  assets/RTK-Codex-agent-instructions.md
    --go:embed rtkCodexAgentInstructions-->
  assets/RTK-Claude-agent-instructions.md
    --go:embed rtkClaudeAgentInstructions-->
  code-Manager.exe


二、完整工程树状图
--------------------

C:\EXEXX\edit\
|
|-- README.txt                         使用说明、架构图、函数索引和排查顺序
|-- LLMTRIM-部署总结.md                llmtrim 已部署实例的全链路、配置、验收与目录差异
|-- RTK-部署总结.md                    RTK 通用开发说明：四平台接入、PATH、状态账本与验收边界
|-- SNIP-部署总结.md                   snip 原生 Hook、Codex 信任、状态账本、验收与目录差异
|-- main.go                            Go 主程序、HTTP 服务、配置和请求转发
|-- application_exit_test.go           退出期间的管理接口闸门聚焦测试
|-- websocket.go                       WS Upgrade、H2/H3 扩展 CONNECT、固定双向承载、协商/压缩校验和连接清理
|-- websocket_test.go                  WS Upgrade、帧直通、真实 H2/H3 SETTINGS、501 回退、错误透传、协商/压缩、开关和连接清理聚焦测试
|-- retry.go                           自动重试配置规范化、按流重试、请求体重放和临时文件缓存
|-- retry_test.go                      自动重试状态码、计数、重放、取消和缓存清理聚焦测试
|-- startup_windows.go                 code-Manager 当前用户开机启动项读写
|-- llmtrim_release.go                 GitHub Release 分页、下载、SHA-256 校验和 ZIP 扁平安装
|-- llmtrim_install.go                 llmtrim setup、Windows 配置校验和安装后启动
|-- managed_tool_installations.go      受管工具安装目录索引；发布目录移动后的旧目录发现
|-- managed_tool_migration.go          snip、llmtrim 的受管残留迁移和精准清理边界
|-- managed_tool_migration_test.go     受管目录识别与 llmtrim 共享状态误判的聚焦测试
|-- llmtrim/                           随项目分发的 llmtrim 上游运行包
|   |-- llmtrim.exe                    llmtrim CLI、compress 和 HTTPS interceptor daemon
|   |-- llmtrim-tray.exe               llmtrim 独立托盘程序（若版本包包含）
|   |-- .llmtrim-version               当前下载的 llmtrim Release tag
|   |-- LICENSE                         llmtrim MPL-2.0 许可证
|   |-- THIRD-PARTY-LICENSES.md         上游第三方依赖许可证汇总
|-- rtk_release.go                     RTK GitHub Release 分页、Windows ZIP 下载、checksums.txt 校验和安全解压
|-- rtk_install.go                     RTK 安装/启动/停止/卸载、RTK-AI 目录、版本标记和统一事务
|-- rtk_codex_commands.go              嵌入完整参考和不超过 32 KiB 的 Codex 常驻规则，并在安装 RTK 时写入 RTK-AI
|-- rtk_codex_commands_test.go         嵌入参考与常驻规则双文件落盘聚焦测试
|-- assistant_integration.go           Codex Markdown 标记段的动态定位、UTF-8 原子读写和专属规则清理
|-- assistant_integration_test.go      标记段替换、幂等和异常标记测试
|-- upstream_transport_test.go         上游 H3/H2 并发握手、H3 宽限优先与 H2 回落测试
|-- rtk_windows.go                     RTK 用户/系统 PATH 读写、按缺失范围补齐、UAC 提权和助手状态检查
|-- rtk_native_integrations.go          RTK 的 Claude/Copilot 官方 Hook 管理
|-- rtk_extra_integrations.go           RTK 的 Cursor 用户级 Hook、四平台快照和恢复管理
|-- snip_release.go                     snip GitHub Release、checksums.txt 校验和 ZIP 安装
|-- snip_install.go                     snip 安装、原生 Hook 启停、状态接口和互斥控制
|-- snip_hook_config.go                 四平台 Hook 的结构化 JSON、处理器位置/指纹检测和逐条清理
|-- snip_ownership.go                   snip 精确 Hook 归属账本、旧账本安全迁移和残留判定
|-- snip_codex_hook.go                  Codex CLI 版本门槛、Hook 规范化身份和信任哈希
|-- snip_trust.go                       Codex 信任状态只读检测、CLI 动态定位和审核命令
|-- snip_trust_windows.go               Windows 可见 PowerShell 审核窗口启动
|-- snip_windows.go                     snip 独立的用户/系统 PATH 读写与 UAC 提权
|-- snip_install_test.go                四平台目录、初始化、归属、安装前停机与路径边界聚焦测试
|-- snip_ownership_test.go              四平台精确账本、重复/移动 Hook、迁移与安全保护测试
|-- snip_hook_config_test.go            Windows 转义 Hook 命令解析测试
|-- snip_codex_hook_test.go             Codex 版本和信任哈希聚焦测试
|-- snip_trust_test.go                  固定 Codex 目录与信任状态聚焦测试
|-- tool_state.go                       RTK、snip、llmtrim 的状态文件和统一互斥锁
|-- single_instance_windows.go         Windows 单实例、进程枚举和进程终止
|-- cleanup_helper_windows.go           主进程退出监测、强制退出后的工具清理助手
|-- cleanup_helper_other.go             非 Windows 构建的清理助手空实现
|-- go.mod                             Go 模块、Go 版本和直接依赖
|-- go.sum                             Go 依赖校验和
|-- third_party\systray\              固定版本的 systray Windows 实现；支持托盘双击回调
|   |-- systray.go                     托盘菜单基础 API 和图标双击回调
|   |-- systray_windows.go             Windows 托盘消息处理（右键菜单、左键双击）
|-- build.bat                          双击执行的 CMD 构建入口，含 http2legacy H2 WS 支持并生成 releases\code-Manager\code-Manager.exe
|-- build.ps1                          旧版 PowerShell 构建编排脚本；发布时不使用
|-- uninstall.bat.template             内嵌到 EXE 的卸载脚本模板，首次运行后释放到 EXE 同级
|-- uninstall_windows.go               EXE 运行期卸载脚本释放、--uninstall 清理和开发目录保护
|-- uninstall_windows_test.go          发布目录保护与运行期卸载脚本聚焦测试
|-- 297763_sort-by-icon.svg            托盘图标源文件
|
|-- config/                            EXE 首次启动时创建的运行期配置目录
|   |-- config.yaml                    网关配置，程序从 config 目录读取
|   |-- code-Manager.log               当前运行会话日志，页面“显示日志”会镜像此文件
|
|-- releases/
|   |-- code-Manager/
|       |-- code-Manager.exe           唯一发布产物；内嵌前端、托盘图标和 RTK 命令文档
|
|-- frontend/                          Vue + Vite 前端源码工程
|   |-- package.json                    npm 脚本和依赖声明
|   |-- package-lock.json               npm 依赖锁定文件
|   |-- index.html                      Vite 开发入口
|   |-- vite.config.js                  Vite 配置，产物输出到 ../web/dist
|   |-- src/
|       |-- main.js                     创建 Vue 应用并挂载 App.vue
|       |-- App.vue                     页面状态、接口调用和控制台模板
|       |-- style.css                   页面样式和移动端媒体查询
|   |-- node_modules/                   npm.cmd install 生成的依赖，不手工修改
|
|-- web/
|   |-- dist/                           npm.cmd run build 生成的静态资源
|       |-- index.html
|       |-- assets/index-*.js           编译后的 Vue JavaScript
|       |-- assets/index-*.css          编译后的页面 CSS
|
|-- assets/
|   |-- RTK-Codex-commands.md          RTK 完整命令参考；保留逐条语法、rewrite、fallback 和平台陷阱
|   |-- RTK-Codex-agent-instructions.md 不超过 32 KiB 的 Codex 高密度命令决策表；完整参考仅供审计和罕见参数复核
|   |-- RTK-Claude-agent-instructions.md Claude Code 的 RTK 提示词；与 Claude Hook 共同构成完整接入
|   |-- tray.png                        Edge headless 生成的 PNG
|   |-- tray.ico                        icon-to-ico.go 生成的 ICO
|
|-- tools/
|   |-- render-tray-icon.html           供 Edge 截图的 256x256 页面
|   |-- icon-to-ico.go                  PNG 到 ICO 的最小转换工具
|
|-- code-Manager.exe                    根目录历史/本地构建产物，非发布目录内容
|-- code-Manager-test.exe               临时/测试构建产物，不是源码
|-- code-Manager.exe~                   旧版或备份构建产物，不是源码

源码和产物的边界：

- 修改页面时只改 frontend/src，不直接改 web/dist。
- 修改托盘图标时优先改 297763_sort-by-icon.svg，不直接改 tray.png/tray.ico。
- web/dist、assets/tray.png、assets/tray.ico 会在构建时覆盖生成。
- assets 中的两份 RTK Markdown 是源码和嵌入资源，不是构建产物；完整参考不得删减或替换为专属常驻规则。
- code-Manager.exe 是构建输出，不在 EXE 内直接修改代码。


三、启动入口和程序生命周期
----------------------------

1. main.go 的 main()

main() 是唯一的程序入口，按以下顺序组装服务：

- defaultConfigPath()：计算 EXE 同级 config\config.yaml 的固定路径。
- 用户入口只接受 `--startup` 标记；内部退出清理助手使用 `--cleanup-helper <PID>`，不支持自定义配置文件路径。
- `--startup` 标记下先等待 8 秒，再继续初始化 HTTP 服务和托盘。
- acquireSingleInstance(instanceMutexName)：创建 Windows 单实例互斥体。
- 如果已有实例，手动启动会调用 openExistingInstance() 打开已有页面；`--startup` 实例静默退出，避免后台运行打开网页。
- openSessionLog()：在 EXE 同级 config 目录创建并清空本次运行的 UTF-8 `code-Manager.log`。
- 不再因为检测到 `RTK-AI\\rtk.exe` 就无条件修复 PATH。RTK、snip、llmtrim 只会在各自状态文件
  的 `desired_running=true` 时恢复运行；若 RTK 和 snip 同时要求恢复，程序不会启动其中任何一个，
  页面会显示冲突状态。
- ensureDefaultConfig(configPath)：缺失时创建同级默认 UTF-8 配置模板。
- loadConfig(configPath)：读取完整配置并校验静态格式；启动代理前额外校验上游 API Key。
- makeHTTPClient(config.OutboundProxy)：建立直连 H3/H2 竞速客户端，或配置 HTTP/SOCKS5 TCP
  出站客户端。
- 固定绑定 127.0.0.1:7780，创建管理 HTTP Server；代理地址使用独立的 HTTP Server，listen_address
  与管理端口相同时复用管理 HTTP Server。
- 创建 gateway，注册管理页面、/healthz、/api 路由，并准备独立的 /v1/ 反代处理器。
- 创建 application，保存管理 HTTP Server、Listener 和固定 localURL。
- systray.Run(app.onTrayReady, app.onTrayExit)：进入托盘生命周期。

启动失败会直接记录错误并退出，不会伪装成启动成功。可能的失败包括：默认配置无法创建、
YAML 无法解析、监听地址非法、端口被占用、上游 URL 非 HTTPS、llmtrim.exe 不存在或代理格式非法。

2. 托盘和 HTTP Server

- application.onTrayReady()
  - 设置 tray.ico 图标和“code-Manager 本地网关”提示。
  - 创建“打开网页”和“退出 code-Manager”菜单。
  - 托盘右键单击显示上述菜单；左键单击不执行任何操作；左键双击调用 openBrowser(app.localURL)
    打开管理网页。
- 在 goroutine 中调用管理 HTTP Server 的 Serve，管理页面始终监听 127.0.0.1:7780。
  - HTTP Server 异常退出时记录日志并触发关闭。
  - 普通手动启动时自动调用 openBrowser(app.localURL)。
  - 开机启动时由 runStartupSequence() 根据 background_start 决定是否打开网页，随后依次启动
    llmtrim 与代理；各最多尝试 3 次。

- application.onTrayExit()
  - 托盘库退出回调，调用 app.shutdown()。

- application.shutdown()
  - 通过 sync.Once 保证只执行一次。
  - 退出顺序固定为：停止 llmtrim 并清理其当前用户环境 -> 清理 RTK -> 清理 snip ->
    停止代理转发 -> 优雅关闭 HTTP 服务。
  - 工具清理最多共享 45 秒 context；HTTP 服务关闭最多使用 5 秒 context。

3. 重复启动处理

- acquireSingleInstance() 使用名称 Local\\code-Manager-single-instance 的 Windows 互斥体。
- 如果同一个程序已经运行，新的 EXE 不绑定第二个端口，也不创建第二个托盘图标。
- openExistingInstance() 始终打开固定的管理页面 127.0.0.1:7780，不读取反代 listen_address；仅由手动重复启动调用。
- isCodeManagerRunning() 请求 /healthz，同时检查 HTTP 200 和精确响应体。
- waitForService() 每 100 毫秒轮询一次，最长等待 5 秒。
- 即使等待结束时服务仍处于启动阶段，也会打开目标浏览器地址，保证用户有页面入口。

4. 强制退出清理助手

- main() 在取得单实例互斥体后启动脱离主进程树的 `--cleanup-helper <PID>` 子进程。
- helper 等待主进程句柄变为 signaled，再取得单实例互斥体，按与正常退出相同的顺序清理 llmtrim、RTK 和 snip。
- llmtrim 由进程名检测并强制结束 `llmtrim.exe`、`llmtrim-tray.exe`，随后清理当前用户自启动和用户环境变量。
- RTK/snip 复用现有状态归属和官方 Hook 卸载逻辑；helper 不删除安装目录，也不覆盖用户未归属的配置。
- 清理错误写入 EXE 同目录 `code-Manager-cleanup.log`；新实例已取得单实例锁时，helper 跳过旧实例清理。


四、HTTP 路由和接口契约
------------------------

所有路由都在 main() 中注册，并经过 requestLogger() 包装。

管理页面加载后会优先连接 `ws://127.0.0.1:7780/api/events`。该端点只接受 HTTP/1.1 WebSocket Upgrade，
校验 Origin 必须为固定管理页面地址，每秒推送 proxy、日志窗口、llmtrim、RTK、snip 的只读状态快照，不包含
upstream API Key，也不接受控制命令。浏览器不支持、握手超时、旧版 EXE 缺少端点或连接断开时，前端自动恢复既有的
HTTP/1.1 状态轮询；当前网页会据此显示管理通道为 `ws` 或 `http1.1`。proxy 快照还携带本地活动 HTTP/WS、
直连 H2/H3 物理连接、WS 承载和活动流计数；配置保存、启动、停止、安装和卸载仍使用下列 REST 接口。

1. GET /

- 处理：frontendHandler()
- 返回 embed.FS 中的 web/dist 静态页面。
- 静态文件不存在时回退到 index.html，兼容 Vue history 路由。
- 仅允许 GET 和 HEAD，其他方法返回 405。

2. GET /healthz

- 处理：gateway.health()
- 返回 HTTP 200 和 {"status":"ok"}。
- 只代表本地网关 HTTP 服务已经可用，不代表上游 API 或 llmtrim daemon 正常。

3. GET /api/settings

- 处理：gateway.settings()
- 返回 listen_address、upstream_base_url、upstream_api_key、upstream_websocket_enabled、startup_enabled、background_start、
  retry_enabled、retry_count、retry_interval_seconds、retry_status_codes。
- API Key 会明文返回给固定的本机管理页面；管理页面不跟随 listen_address 暴露。

4. PUT /api/settings/{setting}

- 处理：gateway.updateSetting()
- 请求体：{"value":"..."}。
- 支持 listen_address、upstream_base_url、upstream_api_key、upstream_websocket_enabled、startup_enabled、background_start、
  retry_enabled、retry_count、retry_interval_seconds、retry_status_codes。
- 使用 json.Decoder 和 DisallowUnknownFields 拒绝未知字段。
- 每个字段先校验，再由 updateConfigValue() 写入 config\config.yaml。
- 使用临时文件写入后替换原文件，避免直接截断配置导致文件损坏。
- 该接口只保存配置，不会自行停止或启动代理；管理页面在 `upstream_websocket_enabled` 或 `retry_enabled` 保存成功后，
  会先读取 `/api/proxy` 状态，并仅在 `running` 或 `connecting` 时依次调用既有的 `/api/proxy/stop` 和
  `/api/proxy/start`。
- 文本值会删除 CR/LF 并去除首尾空格，避免回车或粘贴的换行进入 config.yaml。
- listen_address 只保存配置；停止代理后再次启动时重新读取并绑定新地址。上游地址和 API Key 保存后会更新当前配置，启动代理时还会再次从文件完整重载。
- startup_enabled 会同步更新当前用户 Windows Run 启动项；关闭时会一并关闭 background_start。
  background_start 只能在 startup_enabled 已开启时设为 true。
- 重试字段的响应会携带规范化后的 `value`，页面据此更新失焦后的输入内容；旧版 config.yaml 缺少这四个
  字段时，读取时使用默认关闭、5 次、1 秒和默认状态码列表，不破坏旧配置。
- upstream_websocket_enabled 是布尔即时保存项；旧版 config.yaml 缺少该字段时读取为 true。它控制直连上游是否发起
  WS 承载协商，不改变本地 WS 监听、帧转发格式或 llmtrim daemon 路由。页面保存成功后，若代理运行或连接中，会通过
  既有代理控制接口停止并重新启动，以新配置重建基线上游连接池；后端设置接口本身只保存配置。

5. GET /api/proxy

- 处理：gateway.proxyStatus()。
- 返回代理转发是否已启用、当前监听地址、`code-Manager.exe` 进程 PID 和状态说明；
  无论代理开关状态如何，PID 都是当前管理程序进程的 PID。
- 返回的 `connections` 对象包含 `local_http1`、`local_ws`、`upstream_h2`、`upstream_h2_ws`、
  `upstream_h3`、`upstream_h3_ws`、`upstream_ws`、`upstream_streams`。本地 HTTP/1.1 统计当前 `/v1/`
  转发中的请求；本地 WS 仅在本地 101 成功写出后计入。H2/H3 统计当前连接池中可用的直连物理连接，
  `upstream_ws` 只统计直连 H2/H3 已建立的扩展 CONNECT 或 HTTP-over-WS 承载，`upstream_streams` 是
  这些直连会话上的全部活动流。关闭或轮换后的资源会立即从快照中移除；llmtrim、HTTP/SOCKS5 出站代理不纳入这些上游指标。
- 代理与管理页面共用同一个 `code-Manager.exe` 进程，代理线程没有独立 PID。
- 当 listen_address 为 127.0.0.1:7780 时，HTTP/WS `/v1` 复用管理监听；当地址不同时，HTTP/WS `/v1`
  使用配置端口上的独立监听。管理页面和 /healthz 始终可用；停止代理会关闭独立反代监听、全部上游连接和
  已 Hijack 的 WS 连接，再等待已有 HTTP/WS 反代请求结束。

6. POST /api/proxy/start 和 POST /api/proxy/stop

- 处理：gateway.proxyStart()、gateway.proxyStop()。
- 由网页主状态卡片的“启动代理/停止代理”按钮手动调用；`upstream_websocket_enabled` 或 `retry_enabled`
  保存成功且代理为运行中或连接中时，页面也会复用这两个接口完成自动重启。
- start 每次重新读取并校验 config\config.yaml，随后允许 /v1/* 上游转发；无论 llmtrim 状态如何，都会独立并发预热 H2/H3。
  预热完成只表示连接已建立；扩展 CONNECT 是否可用由上游 SETTINGS 与该次握手决定。如果 llmtrim 已启动，GET、HEAD、POST 和 WS Upgrade 都必须经 43117 的显式正向代理，llmtrim 或 CA
  不可用时返回错误而不回退；明确停止 llmtrim 后才恢复基线路由。stop 会彻底停止反代、关闭已有 WS/上游连接，
  并使 /v1/* 返回 HTTP 503。
- stop 不关闭 code-Manager 管理页面；用户可在固定的 127.0.0.1:7780 页面再次启动代理。

7. GET /api/logs、POST /api/logs/show 和 POST /api/logs/hide

- 处理：gateway.logStatus()、gateway.showLogs()、gateway.hideLogs()。
- show 通过 Windows 原生新控制台启动独立 CMD 窗口，并以只读方式持续跟随显示本次运行的 code-Manager.log；窗口会一直保留，
  直到点击“关闭日志”或用户手动关闭 CMD。hide 只关闭该窗口。
- 日志窗口不会截获、暂停或改变 `/v1/*` 代理请求；用户手动关闭窗口后，后台进程会更新其状态。

8. GET /api/llmtrim/logs、POST /api/llmtrim/logs/show 和 POST /api/llmtrim/logs/hide

- 处理：gateway.llmtrimLogStatus()、gateway.showLLMTrimLogs()、gateway.hideLLMTrimLogs()。
- show 通过 Windows 原生新控制台启动独立 CMD，将当前目录设为 llmtrim.exe 所在目录，并执行相对路径
  `.\\llmtrim.exe status`。该窗口的标准输入和输出不继承 GUI EXE 的 NUL 句柄，能正确显示 status 内容。
- 同一时间只允许一个窗口；hide 会先发送 Ctrl+C，再关闭 CMD。手动关闭 CMD、llmtrim 启停/安装/卸载和
  code-Manager 退出也都会清理 CMD 及其子进程树。

9. GET /api/llmtrim

- 处理：gateway.llmtrimStatus()
- queryLLMTrimRunning() 同时检查当前配置路径的 llmtrim.exe PID 与 127.0.0.1:43117 TCP 端口；两者同时满足才报告运行中。
- findProcessIDByExecutable() 通过 Windows 进程 API 按完整路径查找 PID。
- 返回 path、running、desired_running、activation_state、installed、configured、directory_exists、state_dir_exists、tray_running、tray_process_id、residual、process_id、port、message。
- `activation_state` 分为 `not_installed`、`installed_stopped`、`running`、`attention`。其中 `running` 必须同时满足配置路径进程与 43117 监听；tray 残留、端口被其它进程占用、状态账本要求运行但 daemon 未就绪、路径失效或 Windows 接管校验失败都会显示“需要处理”。
- `installed` 只表示实际存在可用的 `llmtrim.exe`；页面显示“未安装”“已安装/已停止”“已安装/运行中”或“需要处理”。
- `configured` 用于 daemon 运行时的 Windows 接管诊断；停止后环境已按设计清理，不能仅以该字段判断异常。
- `tray_running` 专门反映 `llmtrim-tray.exe`；`residual` 表示仍有任一 llmtrim 进程、安装/状态目录、配置路径、自启动或 Windows 环境残留。

10. POST /api/llmtrim/start 和 POST /api/llmtrim/stop

- 处理：gateway.llmtrimStart()、gateway.llmtrimStop()。
- 两者最终进入 gateway.runLLMTrimCommand()。
- start 接口内部执行 `llmtrim.exe setup`；该命令会恢复 CA、用户环境变量、自启动并启动 daemon，确认 daemon 进入运行状态后才返回成功。
- 页面请求体为 `{}`；服务端始终读取当前 `config.yaml` 的 `llmtrim_path`，保留旧 `path` 字段仅为兼容且不再采用其值。
- validateLLMTrimPath() 要求服务端配置路径的文件存在，且文件名必须是 llmtrim.exe。
- persistLLMTrimPath() 同步保存路径到 config.yaml 和内存配置。
- stop 先执行 autostart --off，防止登录自启动立即拉起 daemon；随后按进程名同时强制结束 `llmtrim.exe` 和 `llmtrim-tray.exe`，并等待两个进程退出。
- executeLLMTrimCommand() 使用临时 stdout/stderr 文件，不使用会被后台子进程继承的管道。
- 命令执行最多 15 秒，随后 waitForLLMTrimState() 最多等待 8 秒确认目标状态。
- stop 使用服务端已保存路径；路径失效或 43117 被未知进程占用时不会按名称误结束未知安装，而是返回“需要处理”的诊断。成功后确认受管 daemon/tray 与 43117 已释放。
- 失败响应带有 running、PID、端口和状态信息，不会永久停在“处理中”。

11. POST /api/application/exit

- 处理：gateway.applicationExit()。
- 页面“停止并退出”调用此接口；接口会保持当前请求，直到受管资源清理完成，再返回
  `{"completed":true,"clean":true|false,"message":"...","warnings":[...]}`。`clean=true` 表示 llmtrim、RTK、
  snip、代理和两个日志窗口均已按退出流程停止；`clean=false` 时 `warnings` 记录未完成步骤，不能把它显示为完全成功。
- 接受退出后，管理路由除该退出接口外均返回 HTTP 503，避免并发配置写入或工具控制与清理流程交错。
- 响应刷出后才调用 application.shutdown() 关闭管理 HTTP 服务并退出托盘循环；不能在同一个仍等待响应的处理器中先调用
  `http.Server.Shutdown()`，否则会等待该处理器结束而无法把最终结果交给页面。

12. GET/HEAD/POST 和 WebSocket Upgrade /v1/*

- 处理：gateway.forward()
- 本地监听始终接受 HTTP/1.1；GET 且携带 `Connection: Upgrade`、`Upgrade: websocket`、版本 13 和有效
  `Sec-WebSocket-Key` 时升级为 WS。local_api_key 非空时，WS 握手也必须先通过 Bearer 鉴权。
- llmtrim 运行时，GET/HEAD/POST 都经 `http://127.0.0.1:43117` 的显式 HTTP(S) 正向代理；WS 会先经显式
  HTTP CONNECT、llmtrim CA 校验和 TLS，再执行 HTTP/1.1 Upgrade。上游或 llmtrim 返回非 101 时，状态、头和
  响应体原样回传给客户端；后续客户端重新发起的普通 HTTP 继续走现有 llmtrim 路径，不会直连绕过。
- llmtrim 未运行且 upstream_websocket_enabled=true 时，H2 使用 `CONNECT` 与 `:protocol=websocket`，H3 使用
  扩展 CONNECT 的 `:protocol=websocket`。上游成功返回 2xx 后，本地才返回 101；两端 WS 帧不解码、不重组，直接双向复制。
  只有上游首个 SETTINGS 未宣告 `SETTINGS_ENABLE_CONNECT_PROTOCOL=1`，或扩展 CONNECT 返回 `501 Not Implemented`，
  网关才在业务请求体尚未发送前改走普通 H2/H3 流承载原始 WS 帧。
- `400`、`401`、`403`、`404`、`405`、`426`、`429`、`502`、`503` 等其它 WS 握手状态，以及其响应头和正文，
  都原样回传给客户端；它们不是回退条件，也不会触发第二条不同语义的业务请求。已建立后的 WS 关闭码只属于帧流，
  不参与 HTTP 握手回退判断。
- 普通 HTTP 在直连模式且开关开启时会先建立 WS 底层承载：请求按标准 HTTP/1.1 线协议写入 WS 二进制帧，返回字节按标准
  HTTP/1.1 响应解析。仅在上述能力缺失条件下回落既有普通 H2/H3 路径；HTTP/SOCKS5 出站代理则在相同代理链路上使用
  HTTP/1.1 WS 建连。开关关闭时不添加上游 WS 协商头，直接使用这些既有普通上游路径。
- 所有 WS 承载请求固定发送 `Accept-Encoding: identity`。若成功承载响应仍带非 identity 的 `Content-Encoding`，
  网关会在回本地 101 前拒绝该承载，绝不让自动解压改写原始 WS 帧。上游返回的 `Sec-WebSocket-Protocol` 必须由本地客户端
  提出；每个 `Sec-WebSocket-Extensions` 扩展名也必须由本地客户端提出，否则拒绝异常握手。
- HTTP-over-WS 与 WS-over-普通 H2/H3 流是固定的承载格式，不是 MASQUE、额外 Base URL 或另行配置的代理协议；
  不增加 JSON 包装、私有字段、私有头或方法覆写。
- WS 长连接占用一个活动流；10 分钟轮换时旧会话停止接收新流但不会中断已有 WS。停止代理、关闭 EXE、上游会话关闭
  或请求上下文取消时，网关主动关闭本地和上游 WS 两端。
- 代理已停止时返回 HTTP 503；普通 HTTP 的不支持方法返回 HTTP 405；无效 WS Upgrade 返回 HTTP 400。
- POST 请求体最大 64 MiB；原始请求体和长度会保留到真实上游请求。
- local_api_key 非空时，必须携带 Authorization: Bearer <local_api_key>。
- `observeLLMTrimRunning()` 每次请求检查 daemon 状态，并据此选择专用正向代理或基线路由。
- gateway.forward() 在当前检查确认 llmtrim 可用时创建/复用专用代理客户端；CA、代理、CONNECT 或 TLS 失败时
  直接拒绝该请求，不会在同一请求内回退直发。
- llmtrim 成功启动或成功停止时，网关关闭已进入 `/v1` 链路的 HTTP keep-alive 和 Hijack WS；管理页面连接和顶部
  “启动代理”独立预热得到的基线 H2/H3 会话不关闭。重连后的请求按完整运行状态重新选路：运行中必经 43117，停止后优先
  复用仍可用的基线会话，没有会话时才执行既有 H2/H3 并发握手。
- joinUpstreamURL() 保留 upstream_base_url 中用户填写的路径，不自动补充或删除 `/v1`，仅在其后追加接口路径和查询参数。
- copyHeaders() 过滤 hop-by-hop、Authorization、Host，再由网关写入上游 Authorization。
- 上游响应状态、普通响应头和响应体复制回客户端。
- 自动重试开启且次数大于 0、状态码列表非空时，GET/HEAD 可直接重发，POST 会先缓存请求体再重发；
  只对用户列表命中的上游 HTTP 状态码重试，不对 DNS、TCP、TLS 或 HTTP 客户端错误重试，也不解析
  上游错误 JSON 擅自判断 429 的具体原因。
- 重试计数属于单个 `gateway.forward()` 调用，即单条上游 HTTP 流；同一 H2/H3 物理连接上的其它本地连接
  不共享次数。只有连续相同状态码累计次数；状态码改变后重新开始计数，达到上限时返回最后一次响应。
- 中间响应不会写给本地客户端，固定按 retry_interval_seconds 等待后再发；不读取或覆盖上游 Retry-After。
  3xx 响应不由 HTTP 客户端自动跟随，保证也能按配置观察和重试。
- 可重试 POST 复用原始请求体，每次上游尝试都经 llmtrim 正向代理。所有可重试 POST 共享 16 GB 十进制内存预算；额度不足时
  使用 EXE 同级、仅当前用户可读的 `.code-manager-retry-*` 临时文件，请求结束、取消或失败后立即删除，
  启动时也会清理遗留文件。
- 43117 代理失败、CA 缺失/无效、目标 URL 无效、上游连接失败时返回 502。
- 直连上游时由 `h3H2Transport` 负责地址解析、并发建立 H3/H2 连接和协议选择；H2 先完成后
  给 H3 600 ms 宽限。启动代理会先完成预热握手，随后按 500 个活动流分配或扩容物理连接；响应关闭
  时释放流名额，20 秒保活，10 分钟待排空轮换。请求只在选定连接上发送一次；配置了 `outbound_proxy`
  时不启用直连竞速，继续使用 HTTP/SOCKS5 的 TCP Transport。

12. GET /api/llmtrim/releases

- 处理：gateway.llmtrimReleases()；仅在页面点击加载版本时访问 GitHub Releases API。

13. POST /api/llmtrim/install

- 处理：gateway.llmtrimInstall()；下载并校验所选版本，安装后执行 setup 并确认 daemon 启动。

14. POST /api/llmtrim/uninstall

- 处理：gateway.llmtrimUninstall()；停止进程并清理 llmtrim 的安装目录、环境、自启动、CA、状态目录，以及带归属标记的受管配置目录。

15. GET /api/rtk

- 处理：gateway.rtkStatus()。
- 返回 RTK-AI\\rtk.exe 是否存在、记录的版本、用户 PATH、系统 PATH，以及 Codex、Claude、GitHub Copilot、
  Cursor 四个平台的 available/configured/residual 状态、冲突来源和本程序记录的 modified_agents。
- RTK 不运行后台 daemon，状态检查只读取文件、注册表 PATH，以及四个平台的用户级配置文件；Codex 的规则状态只读取 `AGENTS.md`，不读取个性化界面文本。

16. GET /api/rtk/releases

- 处理：gateway.rtkReleases()。
- 从 `rtk-ai/rtk` GitHub Releases 分页读取版本，仅标记具有
  `rtk-x86_64-pc-windows-msvc.zip` 的版本可安装。

17. POST /api/rtk/install

- 处理：gateway.rtkInstall()。
- 确认 snip 未运行、当前 RTK 状态未运行且 `rtk.exe` 没有执行后，下载 Windows x64 ZIP 和 `checksums.txt`；按
  资产名匹配 SHA-256 后先解压到临时目录，确认存在 `rtk.exe` 后整体替换当前 `RTK-AI` 专属目录，不扫描或迁移
  其它发布目录。
- 安装只写入 `.rtk-version`、完整 `RTK-Codex-commands.md`、专属 `RTK-Codex-agent-instructions.md` 和停止状态，
  不写用户/系统 PATH，也不修改 Codex、Claude、Copilot 或 Cursor 配置；这些动作属于单独的 `POST /api/rtk/start`。
- 后续启动按平台目录接入：Codex 仅在已有 `AGENTS.md` 时写入规则段；Claude、Copilot、Cursor 在用户级目录
  存在时按官方格式创建或更新 Hook 配置文件，不创建缺失的平台目录。
- 安装流程不调用 `rtk.exe init --global --codex` 生成 `RTK.md` 引用；RTK 不迁移或删除已有的 `RTK.md`、`@` 引用。
  启动会迁移 `AGENTS.md` 或 `CLAUDE.md` 中内容匹配已知旧版 RTK 提示词的标记段；停止或卸载在没有新账本时，仍要求旧状态可安全确认归属后才清理对应内容。

18. POST /api/rtk/uninstall

- 处理：gateway.rtkUninstall()。
- 必须先停止 RTK；随后按 `metadata.rtk_ownership_v1` 的精确归属清理 Codex、Claude、GitHub Copilot、Cursor 四个平台，
  旧 `owned_agents` 仅用于恢复可验证的旧内容；再按状态归属清理用户/系统 PATH，最后删除 RTK-AI 专属目录并逐项验证不存在残留。
- 只允许删除目录名精确为 `RTK-AI` 的目录；任一步骤失败都会返回错误，不再以 warnings 伪装成功。

19. requestLogger(next)

- 仅记录 `/v1/*` 实际代理请求的方法、路径和完成耗时。
- 不记录网页静态资源、健康检查或管理状态轮询。
- 不记录请求正文、API Key 或其他敏感数据。


五、main.go 函数索引
---------------------

启动和配置：

- main()：程序入口，组装配置、HTTP 服务、托盘和全部路由。
- defaultConfigPath()：计算 EXE 同级的固定 config\config.yaml 路径。
- ensureDefaultConfig(configPath)：创建缺失的 UTF-8 默认配置模板。
- loadConfig(configPath)：读取完整 Config 并执行静态配置校验。
- validateProxyConfiguration(config)：启动代理前校验上游 URL 和 API Key。
- validateListenAddress(address)：只允许单个 `IPv4:端口` 或 `[IPv6]:端口`，端口范围为 1-65535。
- validateUpstreamBaseURL(rawURL)：要求 HTTPS 且必须存在 Host；路径可为空或包含 `/v1`。
- makeHTTPClient(rawProxy)：配置直连 H3/H2 竞速 + HTTP-over-WS Transport，或保留 HTTP/SOCKS5 代理链路的 HTTP/1.1 WS Transport。
- `h3H2Transport`：直连上游时按域名或字面 IP 解析连接地址，并发建立 H3 QUIC/TLS 与 H2 TCP/TLS；
  H2 先完成时给 H3 600 ms 宽限。它按主机维护多条连接、500 活动流上限、响应关闭释放、20 秒保活和
  10 分钟排空轮换；同时可按 H2/H3 的协议形式建立 WebSocket 扩展 CONNECT，并让普通 HTTP 在握手可用时使用
  WS 二进制帧承载 HTTP/1.1 线协议。IP 连接不发送 SNI，但仍执行标准 IP SAN 证书校验。
- `dialUpstreamH2()`、`dialUpstreamH3()`：分别建立并验证 ALPN `h2` 与 `h3` 的上游连接。

托盘和重复实例：

- application.onTrayReady()：初始化托盘、启动 Serve goroutine、打开浏览器。
- application.onTrayExit()：托盘退出回调。
- application.shutdown()：退出时先排空已有代理请求，再停止并确认 llmtrim，随后关闭代理转发，
  最后以 5 秒超时优雅关闭 HTTP 服务；llmtrim 清理总超时为 45 秒。
- isCodeManagerRunning(localURL)：请求 /healthz 判断已有实例。
- openExistingInstance()：处理手动重复启动并打开已有页面。
- waitForService(localURL, timeout)：轮询本地服务是否就绪。
- openBrowser(target)：使用 rundll32.exe 调用默认浏览器。

静态页面：

- frontendHandler()：从 embed.FS 提供 web/dist，并处理 history fallback。

网页配置：

- gateway.settings()：GET /api/settings。
- gateway.updateSetting()：PUT /api/settings/{setting}。
- gateway.currentConfig()：在读锁保护下返回当前配置。
- updateConfigValue(configPath, field, value)：保留 YAML 结构并原子替换配置文件。
- writeJSON(w, status, value)：统一设置 JSON Content-Type 并编码响应。

代理控制：

- gateway.isProxyRunning()：读取 /v1 代理转发开关。
- gateway.proxyStatus()：GET /api/proxy。
- proxyStatusResponse.ProcessID：始终返回当前 `code-Manager.exe` 的进程 PID；代理启动和停止不会改变该 PID。
- gateway.proxyStart()：POST /api/proxy/start，重新读取配置并启动 /v1 转发。
- gateway.proxyStop()：POST /api/proxy/stop，彻底停止 /v1 转发并保留管理页面。
- gateway.startProxy()：按配置地址复用 7780 管理监听，或创建独立反代 HTTP Server。
- gateway.stopProxy()：关闭独立反代监听、已 Hijack 的 WS、上游连接并等待已有请求结束；退出流程中也会调用。

日志窗口：

- gateway.logStatus()：GET /api/logs，返回日志 CMD 是否仍在显示。
- gateway.showLogs() / gateway.hideLogs()：打开或关闭独立日志窗口。
- openSessionLog()：在 EXE 同级 config 目录创建并清空本次运行的 code-Manager.log。
- logViewer：管理拥有真实标准输入输出的原生 CMD 日志窗口及其 PowerShell 子进程；关闭窗口不会影响代理进程。

- gateway.llmtrimLogStatus()：GET /api/llmtrim/logs，返回 llmtrim CMD 是否仍在显示。
- gateway.showLLMTrimLogs() / gateway.hideLLMTrimLogs()：打开或关闭 llmtrim CMD 窗口。
- llmtrimStatusViewer：独立管理 llmtrim status 的原生 CMD 控制台、Ctrl+C 发送和进程树清理，不与
  code-Manager 的 logViewer 共用启动逻辑。

llmtrim 控制：

- gateway.llmtrimStatus()：GET /api/llmtrim。
- gateway.llmtrimStart()：启动入口，转入 runLLMTrimCommand("setup")，由 llmtrim setup 同时恢复环境并启动 daemon。
- gateway.llmtrimStop()：停止入口，转入 runLLMTrimCommand("stop")。
- gateway.runLLMTrimCommand()：校验路径、保存路径、执行 CLI、等待状态并返回诊断。
- gateway.stopLLMTrim()：退出流程专用清理，依次关闭 autostart、执行 stop、清理残留进程并确认
  43117 端口已停止监听。
- llmtrimMu：串行化网页 llmtrim 控制与退出清理，避免启动/停止命令并发执行。

RTK 控制：

- gateway.rtkStatus()：读取 RTK-AI\\rtk.exe、版本标记、用户/系统 PATH，以及 Codex、Claude、GitHub Copilot、
  Cursor 四个平台的可用性和集成状态。
- repairInstalledRTKPath()：保留的底层 PATH 修复辅助函数；当前启动流程不会仅因检测到 `rtk.exe` 就自动激活
  RTK，是否恢复 RTK 由 `.code-manager-state.json` 的 `desired_running` 和统一互斥流程决定。
- gateway.rtkReleases()：读取 `rtk-ai/rtk` GitHub Release，并标记 Windows x64 安装包可用性。
- gateway.rtkInstall()：确认 snip 未运行且 RTK 未激活后，下载并校验
  `rtk-x86_64-pc-windows-msvc.zip`，创建全新 RTK-AI 目录，写入版本标记、内置命令文档和停止状态；安装阶段
  不写双范围 PATH，也不修改四个平台配置。
- gateway.startRTK()/startRTKLocked()：在统一互斥锁内快照四个平台和 PATH，写入 PATH、Codex 规则段以及
  Claude/Copilot/Cursor 官方 Hook，失败时恢复快照，成功后记录每项提示词指纹或 Hook 完整命令到 `metadata.rtk_ownership_v1` 并写入运行状态。
- gateway.stopRTK()/stopRTKLocked()：按状态归属清理 Codex 规则段、Claude/Copilot/Cursor 官方 Hook 和双范围 PATH，
  再写入停止状态；旧 `owned_agents` 只恢复能够精确确认的旧部署内容，不猜测未知归属。
- gateway.rtkUninstall()：确认未运行且无 RTK 进程后，清理四个平台、双范围 PATH，最后删除名称精确为 `RTK-AI`
  的专属目录，并逐项验证；任一步骤失败直接返回错误。

11. GET /api/llmtrim/releases

- 处理：gateway.llmtrimReleases()。
- 只有网页点击“加载”或“加载更多”时才请求 GitHub Releases API。
- 每页 5 个版本，按 GitHub 返回的最新到最旧顺序展示，并标记当前 Windows 架构是否有安装包。

12. POST /api/llmtrim/install

- 处理：gateway.llmtrimInstall()。
- 按选定 tag 下载 Windows ZIP 和 SHA-256 文件，校验通过后先停止两个 llmtrim 进程并清理旧环境，
  再将 ZIP 解压到临时目录，整体替换 EXE 同目录的 `llmtrim` 文件夹，不留下旧版本新增文件。
- 安装后保存 `llmtrim_path`，执行 `llmtrim.exe setup`，校验 HKCU\\Environment 中的代理和 CA 配置，
  再确认 daemon 已在 43117 端口运行；任一步失败都会返回错误给页面。

13. POST /api/llmtrim/uninstall

- 处理：gateway.llmtrimUninstall()。
- 删除请求会先停止并确认 `llmtrim.exe`、`llmtrim-tray.exe` 均已退出，然后清理 llmtrim 自启动、匹配的 HKCU 环境变量、
  用户 Root 中的 llmtrim CA、项目创建的 `llmtrim` 文件夹、`%USERPROFILE%\\.llmtrim` 状态目录，以及带归属标记的
  `%USERPROFILE%\\.config\\llmtrim` 配置目录。
- 配置路径位于独立且目录名严格为 `llmtrim` 时，会整体删除该目录，避免遗留未知文件；项目安装目录不会重复删除。
- 删除后的目录、进程、端口、环境和证书库都会复核；任一步失败直接返回错误，不用 warning 伪装成功。
- gateway.persistLLMTrimPath()：持久化 llmtrim_path 并更新内存配置。
- validateLLMTrimPath(pathValue)：校验绝对路径、文件存在和文件名。
- executeLLMTrimCommand(ctx, executable, args...)：15 秒超时执行 CLI，临时文件承接输出。
- readCommandOutput(filePath)：读取并清理 CLI 错误输出。
- queryLLMTrimRunning(executable)：同时确认配置路径的 llmtrim.exe PID 和 43117 端口，避免其他进程占用端口或 daemon 未就绪时误报运行。
- initializeLLMTrimPath(configPath, config)：仅在启动配置中的 llmtrim_path 为空时枚举一次当前进程，
  发现 llmtrim.exe 后保存其完整路径。
- isTCPPortOpen(address)：300 毫秒 TCP 探测。
- waitForLLMTrimState(executable, wantRunning, timeout)：轮询 daemon 目标状态。

API 转发：

- gateway.health()：返回本地健康检查 JSON。
- gateway.startProxy()：启动 HTTP `/v1/` 监听并等待上游 H2/H3 预握手。单轮失败等待 1 秒，最多尝试 5 轮；成功后启动保活任务，失败后关闭本次专用监听并返回上游不可用。
- h3H2Transport：按上游地址维护多条 H2/H3 物理连接；每条限制 500 个活动流，响应体关闭时释放名额，达到限制时扩容，扩容失败时临时超额复用。
- gateway.stopProxy()：禁止新请求、停止 HTTP 监听、取消握手/维护任务并关闭全部上游 H2/H3 会话；下一次启动必定重新握手。
- gateway.forward()：鉴权、按 llmtrim 运行状态选择显式正向代理或基线路由、等待正在进行的预热、拼接上游 URL、转发和回写响应。
- gateway.forwardWebSocket()：鉴权后的本地 Upgrade 分流；llmtrim 使用 CONNECT/TLS/H1 Upgrade，直连仅在开关开启时优先扩展 CONNECT；
  仅 SETTINGS 能力缺失或 501 回落普通 H2/H3 双向流，其它握手响应原样回传。
- gateway.proxyWebSocketStream()：仅在上游握手成功后 Hijack 本地 HTTP/1.1 连接，回写 101，并对 WS 帧做无损双向复制。
- registerWebSocketSession() / closeWebSocketSessions()：跟踪 Hijack 连接；停止代理、llmtrim 成功切换和退出时主动关闭，避免 http.Server 遗留长连接。
- registerProxyConnection() / closeGatewayProxyConnections()：只跟踪已经进入 `/v1` 网关链路的本地连接；llmtrim 成功启停时关闭它们，不关闭管理页面连接或基线会话。
- websocket.go：Upgrade 校验、101 头写入、HTTP/1.1 WS/TLS/HTTP CONNECT、H2/H3 扩展 CONNECT、固定双向承载、
  `Accept-Encoding: identity`、子协议/扩展协商校验、501 精准回落和出站代理兼容。
- gateway.observeLLMTrimRunning(executable)：检查 daemon 状态并选择专用代理或基线路由。
- newLLMTrimProxyClient()：加载 `%USERPROFILE%\\.llmtrim\\ca.pem`，显式设置 43117 代理，并建立可复用的标准 HTTP Transport。
- joinUpstreamURL(base, suffix, rawQuery)：拼接上游路径和查询参数。
- copyHeaders(destination, source)：复制非敏感、非 hop-by-hop 请求头。
- isHopByHopHeader(key)：判断 HTTP hop-by-hop 头。
- requestLogger(next)：仅记录 `/v1/*` 实际代理请求的完成耗时，忽略网页状态轮询。
- redactURL(raw)：日志中去除 URL 用户信息。


六、single_instance_windows.go 函数索引
----------------------------------------

文件顶部的 //go:build windows 表示它只参与 Windows 构建。

- errAlreadyRunning：互斥体已被其他 code-Manager 实例持有。
- singleInstance：保存 Windows mutex handle。
- acquireSingleInstance(name)：调用 CreateMutex，保证单实例。
- (*singleInstance).Close()：释放 mutex 和 Windows handle。
- terminateProcessesByPath(target)：枚举进程并按完整路径精确终止目标进程。
  只在停止 llmtrim 时使用，不按进程名盲目终止。
- findProcessIDByExecutable(target)：按完整路径查找匹配的 Windows PID。
- processImagePath(handle)：调用 QueryFullProcessImageName 获取完整进程路径，并在需要时
  扩大缓冲区。

该文件依赖 golang.org/x/sys/windows。以后如果增加非 Windows 构建目标，需要另行提供
对应的单实例和进程实现，不能直接删除 build tag。


七、前端文件和状态流
----------------------

1. frontend/src/main.js

- 导入 App.vue 和 style.css。
- createApp(App).mount('#app') 挂载单页面应用。

2. frontend/src/App.vue 的响应式状态

- online：本地 /healthz 是否返回成功。
- checking：健康检查按钮是否正在请求。
- loadingSettings：/api/settings 是否正在加载。
- checkedAt：最近一次健康检查时间。
- activeAddress：当前浏览器访问的 host，用于展示 API 地址。
- activeTab：服务页签当前选中项，默认显示 RTK；RTK 显示版本管理，llmtrim 显示 daemon 控制。
- apiKey：上游 API Key 输入框内容。
- proxy.running / proxy.state：代理是否可转发，以及 `stopped`、`connecting`、`running`、`unavailable` 四种状态；页面会明确显示上游握手中或不可用。
- proxy.processId：显示当前 code-Manager.exe 的进程 PID；代理启动和停止不会改变该 PID。
- proxy.loading / working：读取或切换代理状态时的界面忙碌状态。
- proxyNotice：代理启动、停止和失败提示。
- logViewer.showing / working：日志窗口是否显示、是否正在打开或关闭。
- logNotice：日志窗口操作提示。
- llmtrimLogViewer.showing / working：llmtrim CMD 窗口是否显示、是否正在打开或关闭。
- llmtrimLogNotice：llmtrim CMD 窗口操作提示。
- llmtrim.path：llmtrim.exe 路径。
- llmtrim.running：43117 端口是否处于监听状态。
- llmtrim.version：当前下载版本；优先读取 `llmtrim\\.llmtrim-version`，旧安装首次读取状态时兼容执行 `llmtrim.exe --version`。
- llmtrim.installed：实际是否存在可用的 llmtrim.exe；未安装显示“未配置”，已安装但未运行显示“已停止”。
- llmtrim.trayRunning / trayProcessId：`llmtrim-tray.exe` 是否运行及其 PID。
- llmtrim.residual：是否仍存在可删除的 llmtrim 残留；删除按钮依据此字段启用，即使进程正在运行也允许点击，后端负责强制停止。
- llmtrim.loading：llmtrim 状态是否正在读取。
- llmtrim.working：启动或停止请求是否正在执行。
- llmtrim.commandInFlight：控制命令请求尚未结束时锁定按钮，避免重复提交。
- 控制按钮在已安装但 daemon 未运行时显示“启动”，实际调用 start 接口并由后端执行 `llmtrim.exe setup`。
- llmtrimNotice：llmtrim 成功、失败和超时提示。
- llmtrimProcess.id / port：后台 PID 和 daemon 端口诊断信息。
- rtk.path / installed / directoryExists / version：RTK-AI\\rtk.exe 路径、安装状态、残留目录状态和版本标记。
- rtk.userPath / systemPath：RTK 用户 PATH、系统 PATH 状态。
- rtk.codexAvailable / codexConfigured / codexResidual：是否检测到 Codex `AGENTS.md`、专属规则标记段是否完整，以及异常标记残留。
- rtk.claudeAvailable / claudeConfigured / claudeResidual：是否检测到 Claude 用户目录、`settings.json` 的官方 Hook，以及 Hook 结构异常。
- rtk.copilotAvailable / copilotConfigured：是否检测到 GitHub Copilot 用户目录/`hooks/rtk-rewrite.json`，以及官方 `PreToolUse` Hook 是否存在。
- rtk.cursorAvailable / cursorConfigured：是否检测到 Cursor 用户目录/`hooks.json`，以及 `preToolUse` 的 `Shell` RTK Hook 是否存在。
- rtkReleases、selectedRTKVersion、rtkInstallWorking、rtkDeleteWorking：RTK 版本列表和安装/删除操作状态。
- 前端状态监视器：优先使用每秒一条的 `/api/events` WS 快照；浏览器不支持、握手超时或连接断开时才串行刷新
  `/healthz`、`/api/proxy`、`/api/llmtrim`，日志窗口打开时额外刷新 `/api/logs`，llmtrim CMD 窗口打开时额外刷新
  `/api/llmtrim/logs`。页面不可见时暂停，重新可见时立即刷新，避免后台无效请求和并发轮询。
- managementProtocol：当前浏览器标签的管理通道，事件 WS 已连接时为 `ws`，HTTP 状态轮询时为 `http1.1`。
- proxy.connections：顶部“连接状态”行的数据源；显示本地活动 HTTP/WS、直连 H2/H3 连接、活动 WS 承载和活动流。
  H2/H3 只要存在活动 WS 承载就在协议名后显示 `(ws)`；计数全部是当前值，不是本次启动累计。
- confirmation.open / title / description / confirmationAction：删除 RTK、snip、llmtrim 和“停止并退出”共用的 Vue 顶层确认框；背景页面通过 `inert` 锁定，
  不能通过点击遮罩或按 Esc 关闭，只能点击“确定”执行原操作或点击“取消”恢复触发按钮焦点。
- exitState / pageInteractionLocked：网页退出状态机。进入 `stopping` 后立刻停止两个轮询定时器、失焦当前控件、
  取消可中止的代理启动并通过 `inert` 和全屏遮罩锁住页面；接口返回 `clean=true` 后进入 `completed` 并持续显示
  “已停止并退出”。若连接在结果返回前断开，则进入同样不可操作的 `disconnected` 状态，避免把已送达的退出请求误恢复为可编辑页面。
- 启动或停止期间，轮询一旦确认目标状态，会立即结束“处理中”显示，不等待控制命令响应。
- 控制流程使用独立操作状态机：按钮状态以 `/api/llmtrim` 返回的真实监听状态为准，命令响应延迟、超时或乱序返回不会覆盖已确认状态。
- settings.listenAddress / upstreamBaseURL / upstreamWebSocketEnabled：网关配置输入值和直连 WS 承载开关。
- saving.*：配置保存按钮和上游 WS 承载开关各自的忙碌状态。
- notices.*：配置保存区域与上游 WS 承载开关的提示信息。
- retry.enabled / count / intervalSeconds / statusCodes：自动重试配置；开关立即保存，其余字段失焦保存。
  每个字段维护独立保存序列，较早的接口响应不会覆盖用户后续输入。

3. App.vue 函数和接口对应关系

- checkHealth()
  - GET /healthz。
  - 更新 online、checkedAt、checking。

- loadProxyStatus() / controlProxy(action)
  - GET /api/proxy，或 POST /api/proxy/start、/api/proxy/stop。
  - 主状态卡片显示代理真实状态，并允许在不关闭管理页面的情况下启动或停止 /v1 转发；controlProxy 返回成功或失败，
    设置开关的自动流程只有在停止成功后才会继续启动。

- loadLogStatus() / controlLogViewer(action)
  - GET /api/logs，或 POST /api/logs/show、/api/logs/hide。
  - 控制独立 CMD 日志镜像窗口；检测到用户关闭窗口后自动恢复“显示日志”。

- loadLLMTrimLogStatus() / controlLLMTrimLogViewer(action)
  - GET /api/llmtrim/logs，或 POST /api/llmtrim/logs/show、/api/llmtrim/logs/hide。
  - 控制 llmtrim status 独立 CMD 窗口；检测到用户关闭窗口后自动恢复“显示日志”。

- loadSettings()
  - GET /api/settings。
  - 读取 upstream_websocket_enabled；旧后端缺失该字段时页面保持默认开启。
  - 检查 Content-Type，发现旧版 EXE 时给出升级提示。

- loadLLMTrimStatus()
  - GET /api/llmtrim。
  - 更新只读路径、activation_state、真实 daemon 健康、托盘进程、残留状态、PID、端口和提示信息；静默刷新也会回填路径，避免页面重建后无法启动。

- controlLLMTrim(action)
  - POST /api/llmtrim/start 或 /api/llmtrim/stop；不提交路径，服务端按已保存配置执行 lifecycle。停止时配置路径失效会保留既有的安全清理边界，不会按名称误结束未知安装。
  - 使用 AbortController，前端最多等待 25 秒，为后端命令执行和状态确认预留缓冲。
  - 解析 JSON 或文本错误，finally 中必定恢复按钮可用状态。

- loadRTKStatus() / loadRTKReleases()
  - 分别读取 `/api/rtk` 状态和 `/api/rtk/releases?page=N` 版本列表。
- installRTK() / requestUninstallRTK() / uninstallRTK()
  - `requestUninstallRTK()` 先打开统一 Vue 确认框，确认后才调用 `/api/rtk/uninstall`；安装仍调用 `/api/rtk/install`。
    页面展示 PATH、Codex 规则段、Claude/Copilot/Cursor 官方 Hook 集成和严格清理结果；检测到残留目录、PATH 或任意平台残留时，即使 `rtk.exe` 缺失也允许点击“删除”。

- requestUninstallSnip() / uninstallSnip()、requestUninstallLLMTrim() / uninstallLLMTrim()
  - 两类删除同样先经过统一 Vue 确认框，再分别调用 `/api/snip/uninstall`、`/api/llmtrim/uninstall`；不会使用浏览器原生确认框。

- saveSetting(key, endpoint, value)
  - PUT /api/settings/{endpoint}。
  - 保存 listen_address、upstream_base_url 或 upstream_api_key。

- updateUpstreamWebSocketEnabled(enabled)
  - PUT /api/settings/upstream_websocket_enabled。
  - 开关即时保存；失败时恢复页面原值。成功保存且代理为 running 或 connecting 时，与自动重试开关共用串行停止、
    启动流程，按新配置重建基线上游连接池；不启动或停止 llmtrim daemon。

- saveRetrySetting(key, endpoint)
  - PUT /api/settings/{endpoint}。
  - 自动保存重试开关和失焦后的重试输入，采用后端返回的规范化 value 更新对应字段；只有 retry_enabled
    保存成功后会触发上述代理重建，retry_count、retry_interval_seconds、retry_status_codes 失焦保存不重启代理。

- restartProxyForConnectionSetting(settingLabel)
  - 保存成功后刷新代理状态；仅在 running 或 connecting 时，等待停止成功后再启动代理。
  - 失败时保留已保存的开关值与现有代理状态，并在顶部状态提示中说明自动重启失败。

- onMounted()
  - 页面加载时调用 checkHealth()、loadSettings()，随后优先建立管理页 HTTP/1.1 WebSocket 状态流。
  - WS 成功后由服务端首包和每秒状态快照更新页面；不支持、握手超时或连接关闭时自动回退到 HTTP 状态轮询。

- runStatusPoll() / handleVisibilityChange()
  - 仅在管理 WS 不可用时使用可取消的 `setTimeout` 串行轮询，避免请求重叠。
  - 页面进入后台时停止定时器并关闭管理 WS；回到前台时优先重新建立 WS，失败后再恢复 HTTP 轮询。

4. frontend/src/style.css

- 负责深色控制台页面、状态卡片、设置卡片、输入框和按钮样式。
- 560px 以下切换移动端布局：面板缩小内边距，状态卡片允许换行，输入框和按钮改为
  单列，llmtrim 标题和状态操作区改为纵向/全宽布局。
- 自动重试区在桌面端使用“开关、次数、间隔”的三列布局，状态码输入框占满下一行；560px 以下改为单列，
  不允许页面产生横向滚动。
- RTK 版本选择、安装和删除复用 llmtrim 的响应式安装行；长路径使用断词规则，避免在移动端溢出。
- 修改 UI 时必须同时检查桌面宽度和移动宽度，尤其注意长路径、错误提示和按钮换行。


八、配置文件导图
------------------

config.yaml 固定在 code-Manager.exe 同级的 config 目录，由 main.go 的 ensureDefaultConfig()/loadConfig() 读取，由 updateConfigValue() 更新。目录或文件缺失时会自动创建 UTF-8 默认模板。

- listen_address
  - 示例使用 127.0.0.1:7780，可修改为其它回环地址。
  - 只允许本机地址；该端口是反代 API 端口，不是管理页面端口。
  - 每次点击“启动代理”都会重新读取该值并绑定反代监听。
  - 管理页面固定为 http://127.0.0.1:7780/；listen_address 为 7780 时 HTTP 反代复用管理监听，否则使用独立 HTTP 监听器。

- upstream_base_url
  - 必须是 HTTPS URL；可填写域名根地址或带 `/v1` 的地址，网关不自动补充或删除路径。
  - 由 joinUpstreamURL() 和 gateway.forward() 使用。
  - 网页保存后，后续请求立即使用新值，并完全覆盖 `%USERPROFILE%\\.config\\llmtrim\\config.toml` 中的
    `extra_hosts` 为 URL 的 hostname；不写 API Key。正在运行的 daemon 不自动重启，需先停止再启动才读取新主机。

- upstream_api_key
  - 管理页面可在默认模板创建后修改；启动代理时不能为空，且不能使用占位值。
  - 由 gateway.forward() 写入上游 Authorization。
  - 不写入日志，但 /api/settings 会返回明文给本机页面。

- upstream_websocket_enabled
  - 默认 true，由管理页“上游 WS 承载”开关即时保存；旧配置缺失该字段时同样按 true 读取。
  - 只影响 llmtrim 未运行时，直连上游是否添加 H2/H3 扩展 CONNECT 或出站代理 H1 Upgrade 的 WS 协商头。
  - 关闭后本地 HTTP/WS `/v1` 监听仍支持 Upgrade，llmtrim 运行时仍先交给 43117；只有直连上游改走普通流。页面保存
    成功后，如代理运行或连接中，会停止并重新启动代理以重建本程序基线上游连接池，不会启动或停止 llmtrim daemon。
  - 开启不表示上游一定可用：是否承载由对端 SETTINGS 和实际握手决定；仅 SETTINGS 能力缺失或 501 回落。

- outbound_proxy
  - 空字符串表示直连。
  - 支持 http://host:port 和 socks5://host:port。
  - 由 makeHTTPClient() 配置基线 HTTP Transport；只有 llmtrim 停止时才用于真实上游连接。

- llmtrim_path
  - llmtrim.exe 的绝对路径。
  - 启动时检查文件存在；控制接口还要求文件名是 llmtrim.exe。
  - 用于检测、启动和停止 daemon；运行中 GET/HEAD/POST 均通过 43117 的显式 HTTP(S) 正向代理完成处理。

- local_api_key
  - 为空表示不做本地 API 鉴权。
  - 非空时 /v1/* 必须携带 Authorization: Bearer <local_api_key>。
  - /healthz 和网页管理接口目前不使用该字段做鉴权，因此仍必须保持回环监听。

- retry_enabled / retry_count / retry_interval_seconds / retry_status_codes
  - 共同控制上游 HTTP 状态码自动重试；任一条件使重试无效时不缓存普通 POST 请求体。
  - 计数按每条请求流和连续状态码维护，不受连接复用和其它本地客户端影响。
  - 状态码列表由 normalizeRetryStatusCodes() 统一规范化；请求体由 retryBodyCache 在内存预算内缓存，超额落盘并自动清理。
  - 页面仅在 retry_enabled 成功保存且代理运行或连接中时重启代理；其余三个字段失焦保存只更新配置，不重启代理。


九、构建和运行方式（Windows + VSCode）
---------------------------------------

1. 普通运行

直接双击：

  C:\EXEXX\edit\code-Manager.exe

程序读取同级 config\config.yaml，启动本地服务，显示托盘图标并打开：

  http://127.0.0.1:7780

关闭浏览器页面不会停止进程。请通过托盘菜单“退出 code-Manager”停止程序。

2. 前端依赖和构建

Windows 下统一使用 npm.cmd，不要依赖 PowerShell 对 npm 的别名解析：

  cd C:\EXEXX\edit\frontend
  npm.cmd install --no-audit
  npm.cmd run build

npm.cmd run build 会把产物写入：

  C:\EXEXX\edit\web\dist

frontend/vite.config.js 中的 outDir 固定为 ../web/dist。
`--no-audit` 会关闭 `npm.cmd install` 默认发起的在线漏洞审计；需要检查依赖漏洞时，
请在 frontend 目录单独执行 `npm.cmd audit`。

3. 完整构建（发布 EXE）

推荐直接双击项目根目录的：

  C:\EXEXX\edit\build.bat

`build.bat` 会以 UTF-8 控制台编码在 CMD 中完成构建，构建完成或失败后保留窗口，方便查看结果。
每次双击都会先清理并重新创建 `releases\code-Manager`，然后基于当前 `frontend/src`、Go 源码和
图标源文件生成唯一发布文件：

  C:\EXEXX\edit\releases\code-Manager\code-Manager.exe

build.bat 的实际步骤：

- 检查 SVG 和 Edge 是否存在。
- 使用 Edge headless 将 297763_sort-by-icon.svg 截图为 assets/tray.png。
- 使用 go run tools/icon-to-ico.go 生成 assets/tray.ico。
- 进入 frontend 目录执行 npm.cmd install --no-audit，避免发布构建等待在线漏洞审计。
- 执行 npm.cmd run build 生成 web/dist。
- 回到根目录执行 `go build -ldflags "-H=windowsgui" -o releases\code-Manager\code-Manager.exe .`，
  生成不自动弹出控制台的正式 EXE。

说明：前端 npm 命令必须写成 npm.cmd；构建入口不依赖 PowerShell。后端使用 Go 工具链的 go build，
不是通过 npm 构建。打包流程不会自动运行 gofmt 或 go mod tidy，避免仅为发布而修改源码或依赖清单。

构建流程和发布产物关系如下：

  frontend/src/*
    --npm.cmd run build-->
  web/dist/*
    --Go //go:embed web/dist-->
  releases/code-Manager/code-Manager.exe

- `frontend/src` 是 Vue + Vite 前端源码。
- `web/dist` 是 Vite 编译产物，由构建命令生成，不手工修改。
- `main.go` 使用 `//go:embed web/dist` 将页面嵌入 EXE，同时嵌入 `assets/tray.ico`；
  `rtk_codex_commands.go` 使用 `//go:embed assets/RTK-Codex-commands.md` 和
  `//go:embed assets/RTK-Codex-agent-instructions.md` 将完整参考与专属常驻规则嵌入 EXE；
  `uninstall.bat.template` 也会嵌入 EXE，首次正常运行时才释放到同级目录。
- `go.mod` 通过 `replace` 固定使用 `third_party\systray`；该副本仅在上游 Windows 托盘消息处理处增加
  左键双击回调，避免依赖本机 Go 模块缓存中的未修改版本。
- `go.mod` 直接固定 `github.com/quic-go/quic-go`，用于上游 HTTP/3 QUIC/TLS 连接；更新该依赖后
  必须一并提交 `go.sum` 的校验和变化。
- `go.mod` 直接固定 `github.com/gobwas/ws`，用于 HTTP-over-WS 的二进制帧编解码、直接 WS 到 WS 转发和管理页
  HTTP/1.1 WebSocket 状态快照；直接 WS 到 WS 转发不解析、不重组原始帧。
- 安装 RTK 时，程序会将嵌入的完整命令参考和 Codex 高密度规则写入 RTK-AI；点击“启动”时才按实际存在情况
  写入 Codex 规则段，并更新 Claude `settings.json`、Copilot `hooks/rtk-rewrite.json` 和 Cursor `hooks.json` 官方 Hook，
  不依赖外部源文件路径。
- 首次发布只分发 `releases\code-Manager\code-Manager.exe`。首次运行会在其同级创建用户自己的
  `uninstall.bat`、`config\config.yaml` 和 `config\code-Manager.log`；不需要分发 `web/dist`、前端
  `node_modules`、源码、图标源文件、assets 或 `.release` 标记文件。
- RTK、snip 和 llmtrim 的上游运行文件不属于首发 EXE 的静态内容，仍由页面的现有安装功能按需下载到
  EXE 同级专属目录。
- 修改前端后必须先执行 `npm.cmd run build`，再执行
  `go build -ldflags "-H=windowsgui" -o releases\code-Manager\code-Manager.exe .`，新页面才会进入 EXE。
- 构建完成后，如果旧版 `code-Manager.exe` 仍在运行，Windows 单实例机制会让新 EXE
  打开旧进程提供的页面。发布验证前应先通过托盘菜单退出旧进程，再启动新的 EXE。

4. 直接分步构建

只修改前端时：

  cd C:\EXEXX\edit\frontend
  npm.cmd run build

需要把新的 web/dist 嵌入 EXE 时：

  cd C:\EXEXX\edit
  go build -ldflags "-H=windowsgui" -o releases\code-Manager\code-Manager.exe .

修改 Go 依赖后再考虑执行 go mod tidy；普通源码修改不需要每次 tidy。

5. 前端开发服务器

可以运行：

  cd C:\EXEXX\edit\frontend
  npm.cmd run dev

但当前 vite.config.js 没有配置 API proxy。开发服务器适合观察静态页面，页面中的
/healthz、/api/settings、/api/llmtrim 请求不会自动转发到反代端口。验证真实功能时应使用
构建后的 EXE 页面，或另行配置开发代理后再测试。


十、运行排查顺序
------------------

遇到“启动中”“按钮无响应”或页面显示异常时，按下面顺序检查：

1. 查看 code-Manager 和 llmtrim 进程：

  Get-Process -Name code-Manager,llmtrim -ErrorAction SilentlyContinue |
    Select-Object Name,Id,Path,StartTime

2. 查看网关端口：

  Get-NetTCPConnection -LocalPort 7780 -ErrorAction SilentlyContinue

3. 查看 llmtrim daemon 端口：

  Get-NetTCPConnection -LocalPort 43117 -ErrorAction SilentlyContinue

4. 在浏览器访问：

  http://127.0.0.1:7780/healthz

5. 查看网页中的网关“最近检查”时间、llmtrim 状态徽标、端口、PID 和错误提示。

6. 如需手工运行 llmtrim CLI：

  C:\EXEXX\llmtrim\llmtrim.exe --help
  C:\EXEXX\llmtrim\llmtrim.exe start
  C:\EXEXX\llmtrim\llmtrim.exe stop

7. 如果页面提示“旧版 code-Manager”：

- 先通过托盘菜单退出旧进程。
- 确认任务管理器中的 EXE 路径是 C:\EXEXX\edit\code-Manager.exe。
- 再重新打开页面，避免浏览器连接到旧进程。

8. 退出 code-Manager 时的自动清理：

- 托盘“退出 code-Manager”、网页“停止并退出”和内部异常退出都会触发统一清理；强制结束由独立助手接续。
- 先按配置路径或受管安装目录精确检测并结束对应的 `llmtrim.exe`、`llmtrim-tray.exe`，确认 43117 端口停止，
  再清理 llmtrim 当前用户自启动和用户环境变量；不按进程名误结束用户独立安装。
- 然后清理 RTK 用户/系统 PATH、Codex 规则段以及 Claude/Copilot/Cursor 官方 Hook。
- 再按状态账本精确删除本程序拥有、且仍指向当前发布目录绝对 `snip.exe` 路径的 Agent Hook，并移除本程序受管的 Snip 用户/系统 PATH；同一文件中的其它 Hook 内容保留，不调用官方宽匹配 `--uninstall`。
- 工具清理完成或记录失败后，再停止 /v1 代理转发。
- 最后关闭 code-Manager 的 HTTP 服务并释放单实例互斥体。
- 若任一工具清理失败，程序仍会继续退出；主日志或 `code-Manager-cleanup.log` 会记录失败原因。

9. 如果 /healthz 正常但 /v1 请求失败：

- 检查 llmtrim 端口和 llmtrim_path。
- 检查 upstream_base_url 是否为 HTTPS 且可访问。
- llmtrim 运行时检查 `%USERPROFILE%\\.llmtrim\\ca.pem`、43117 的 CONNECT/MITM 链路和保存的 `extra_hosts`；
  llmtrim 停止时再检查 outbound_proxy 是否正确。
- 检查 upstream_api_key 和可选 local_api_key。


十一、维护时的推荐阅读顺序
----------------------------

为了避免每次全盘扫描，后续分析本项目建议按以下顺序：

1. 先读本 README 的“工程树状图”“启动链路”和“接口契约”。
2. 读 config\config.yaml，确认实际端口、上游地址、llmtrim 路径和代理设置。
3. 读 main.go 的 main() 和 startup_windows.go，确认启动顺序、配置加载、开机启动和路由注册。
4. 根据问题选择入口：
   - 启动、端口、托盘：main()、onTrayReady()、shutdown()。
   - 重复启动：acquireSingleInstance()、openExistingInstance()。
   - 网页配置与开机启动：settings()、updateSetting()、updateConfigValue()、syncCodeManagerStartup()。
   - llmtrim：llmtrimStatus()、runLLMTrimCommand()、executeLLMTrimCommand()、
     queryLLMTrimRunning()。
   - API 转发：forward()、selectUpstreamClient()、newLLMTrimProxyClient()、joinUpstreamURL()、copyHeaders()。
5. 涉及 Windows 进程、PID 或强制停止时，再读 single_instance_windows.go。
6. 涉及页面状态或按钮时，读 frontend/src/App.vue 的对应函数和模板区域。
7. 涉及布局或服务页签时，读 frontend/src/style.css 和 frontend/src/App.vue 对应模板，并同时检查 560px 以下媒体查询。
8. 修改前端后确认是否需要重新生成 web/dist；否则新页面不会进入 EXE。
9. 修改完成后执行与改动匹配的最小验证：
   - Go 修改：gofmt 和 go build。
   - Vue 修改：npm.cmd run build。
   - 启动链路修改：启动 EXE，访问 /healthz，检查进程和端口。


十二、安全和数据注意事项
--------------------------

- config.yaml 含有上游 API Key，不能提交到公开仓库或发送给他人。
- upstream_api_key 会通过本机管理页面明文显示；页面只能在本机使用。
- listen_address 可以监听 0.0.0.0 或 [::]。这会把 HTTP/WS `/v1` 暴露给可达网络；必须通过 Windows 防火墙、路由规则或隔离网络限制来源，不能直接暴露到互联网。
- local_api_key 用于 HTTP/WS `/v1` 的 Bearer 鉴权；监听地址不再提供 SOCKS 协议。
- 日志不会记录请求正文和 API Key；排查时不要公开完整配置文件或敏感日志。
- terminateProcessesByPath() 按完整路径匹配进程，不要改成仅按 llmtrim.exe 文件名终止，
  以免误杀其他目录中的实例。


十三、当前实现边界
--------------------

- listen_address 支持单个 IPv4 或 IPv6 地址，并且只监听 HTTP/1.1 `/v1`；该监听默认支持标准 WebSocket Upgrade，管理页面仍只监听 127.0.0.1:7780。
- 本地 `/v1` 不实现 SOCKS、入站 CONNECT、MASQUE 或 SOCKS-over-H2/H3；WebSocket 只使用标准 HTTP/1.1 Upgrade、H2/H3 扩展 CONNECT 或普通 HTTP 双向流。
  HTTP-over-WS 与 WS-over-普通流是固定承载格式，不增加额外 Base URL、端点、JSON 包装、私有字段或新的代理协议配置。
- 直连上游启动时会完成 H2/H3 并发预握手并读取 SETTINGS，失败后按 1 秒间隔最多重试 5 轮。每条连接最多 500 个活动流，20 秒保活，10 分钟后不再接收新流；已有 HTTP 流和 WS 不会被轮换中断。
- upstream_websocket_enabled 默认开启且只影响直连上游 WS 协商；本地 HTTP/WS 监听始终支持，llmtrim 运行时该开关不改变先经 43117 的路由。
  页面保存该开关或 retry_enabled 成功后，若代理正在运行或连接中，会复用停止、启动控制流程重建本程序的基线上游连接池；
  重试次数、间隔和状态码的失焦保存不会重启代理。
  扩展 CONNECT 仅在上游 SETTINGS 宣告能力后使用；只有 SETTINGS 能力缺失或 501 才回落，429/502/503 等错误原样回传。
- /v1/* 支持 GET、HEAD、POST 和有效的 WebSocket Upgrade；llmtrim 启动后，普通 HTTP 和 WS 都经 43117 显式正向代理、llmtrim CA 和 TLS，代理或 CA 失败时拒绝请求；明确停止 llmtrim 后才恢复基线路由。
  llmtrim 成功启动或停止会主动断开已有 `/v1` 连接，管理页面和独立 H2/H3 预热不受影响。
- /healthz 只表示本地 HTTP 服务正常，不代表上游 API 或 llmtrim 一定正常。
- 前端页面是单页控制台，不负责托盘生命周期；关闭页面不会停止后台进程。
- web/dist、assets/tray.png、assets/tray.ico 都是构建产物，源文件分别是 frontend/src、
  297763_sort-by-icon.svg 和 tools/icon-to-ico.go。
- 项目测试文件位于根目录并按功能聚焦；`websocket_test.go` 覆盖 Upgrade、帧直通、真实 H2/H3 SETTINGS 下的扩展 CONNECT 能力判定、501 精准回落、429/502/503 透传、压缩/协商校验、HTTP-over-WS、HTTP/SOCKS5 链路、开关和 llmtrim CONNECT/TLS/拒绝透传。验证以聚焦 Go 测试、`go build` 和 `go test` 的 `-tags http2legacy`、npm.cmd run build 和本机运行链路为主，不要随意执行耗时很长的全量测试。


十四、变更后的交付检查清单
----------------------------

- 是否只修改了本次需求相关文件？
- 是否保持 UTF-8 编码、无乱码、无 BOM 和 LF 换行？
- 是否同步考虑了 PC 和移动端布局？
- 修改前端后是否执行 npm.cmd run build？
- 修改 Go 后是否执行 gofmt 和 go build？
- 是否确认 web/dist 已包含最新前端代码并重新嵌入 EXE？
- 是否检查 HTTP/WS Upgrade、关闭帧、停止代理、llmtrim 拒绝透传、普通 HTTP WS 回落和 API 转发相关失败路径？
- 是否需要同步更新本 README 的文件职责或函数索引？


十五、GitHub 发布流程
---------------------

GitHub 仓库：`nicelic/codex-manager`。

发布原则：

- 每次发布只以当前工作区的最新代码为准，不需要获取、比对或汇总历史版本的代码变化。
- 每次发布前都必须运行项目根目录的 `build.bat`，不能直接复用旧的 EXE。
- `build.bat` 成功后，唯一用于 GitHub Release 的附件是
  `releases\code-Manager\code-Manager.exe`。
- 先提交并推送当前源码、文档和忽略规则；不要提交本机配置、日志、`node_modules`、`web/dist`、
  `releases` 目录或其他生成的 EXE。
- GitHub Release 使用对应的语义化版本标签，例如 `v0.0.1`。发布说明可以保持简短，只说明该版本
  已发布并提供 Windows 可执行文件，无需根据历史提交自动生成变更日志。

首次发布 v0.0.1：

1. 确认工作区是准备发布的当前代码。
2. 在项目根目录运行 `build.bat`，等待它生成最新的
   `releases\code-Manager\code-Manager.exe`。
3. 检查构建成功且 EXE 存在后，提交并推送当前源码到 GitHub。
4. 创建标签和 Release：`v0.0.1`。
5. 将 `releases\code-Manager\code-Manager.exe` 上传为该 Release 的附件。

后续发布版本时重复相同流程：先运行 `build.bat` 生成最新 EXE，再创建该版本的 GitHub Release 并上传
新生成的 EXE；不需要先获取历史版本代码变化。
