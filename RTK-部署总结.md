# RTK 部署与接入开发说明

> 维护日期：2026-09-03。本文说明 code-Manager 对 RTK 的安装、激活、四平台接入、状态归属和验收边界。本文不记录任意电脑的实际目录、当前用户状态、密钥或一次性核验结果。

## 一、职责与术语

RTK 是命令输出处理工具，不是常驻 daemon、HTTP 代理或后台服务。平台执行 Shell 工具或用户直接调用 RTK 时，RTK 才会短暂运行。

code-Manager 负责以下内容：

- 下载 Windows x64 Release、校验 SHA-256，并将文件安装到 `发布目录\RTK-AI`。
- 写入 RTK 的完整命令参考和两个 Agent 提示词资源。
- 在用户明确启动 RTK 时配置 PATH，并接入实际可用的平台配置。
- 记录本程序拥有的 PATH 与平台接入，支持停止、卸载和失败回滚。

状态账本中的 running=true 应称为“已激活”，含义是最近一次激活事务已经提交。它不表示 RTK 有后台进程，也不表示 Codex、Claude Code、GitHub Copilot、Cursor 四个平台全部存在或全部接入成功。

## 二、安装目录与内置资源

RTK 的专属目录始终根据运行中的 code-Manager.exe 动态计算：

| 内容 | 相对位置 | 作用 |
|---|---|---|
| 可执行文件 | RTK-AI\rtk.exe | RTK CLI 与平台 Hook 的执行入口 |
| 版本标记 | RTK-AI\.rtk-version | 记录选定的 Release tag |
| 状态账本 | RTK-AI\.code-manager-state.json | 记录激活意图、PATH 与每项提示词/Hook 的精确归属 |
| 完整参考 | RTK-AI\RTK-Codex-commands.md | RTK 正式命令、rewrite、fallback 与 Windows 细节的审计资料 |
| Codex 提示词资源 | RTK-AI\RTK-Codex-agent-instructions.md | 写入 AGENTS.md 的高密度 RTK 规则 |
| Claude 提示词资源 | RTK-AI\RTK-Claude-agent-instructions.md | 写入 CLAUDE.md 的 RTK 使用提示 |

安装阶段只落盘上述 RTK 文件并写入停止状态；不会修改 PATH，也不会修改任意 Agent 配置。

## 三、四个平台接入模型

平台目录不存在时，RTK 不会为了接入而创建用户级平台目录。目录存在且配置可以安全解析时，启动事务才会写入相应内容。

| 平台 | 前置条件 | 目标文件 | 本程序写入的内容 | 完整接入判定 |
|---|---|---|---|---|
| Codex | CODEX_HOME 指向的目录或用户主目录下的 .codex 存在，且 AGENTS.md 已存在 | AGENTS.md | 一段受 RTK 标记包围的高密度常驻规则 | 标记段与内置 Codex 提示词完全匹配 |
| Claude Code | CLAUDE_CONFIG_DIR 指向的目录或用户主目录下的 .claude 存在 | CLAUDE.md、settings.json | CLAUDE.md 中的 RTK 提示词标记段；settings.json 的 hooks.PreToolUse/Bash 命令 Hook | 提示词和 Hook 都匹配 |
| GitHub Copilot | COPILOT_HOME 指向的目录或用户主目录下的 .copilot 存在 | hooks\rtk-rewrite.json | hooks.PreToolUse 中的 RTK Hook | 匹配 RTK Copilot Hook |
| Cursor | 用户主目录下的 .cursor 存在 | hooks.json | hooks.preToolUse/Shell 中的 RTK Hook | 匹配 RTK Cursor Hook |

Claude Code 是两份配置共同组成的接入：

1. Hook 负责在 PreToolUse 阶段把可处理的 Shell 命令交给 RTK。
2. CLAUDE.md 提示词告知 Agent Hook 的行为边界、RTK 元命令和保留原生命令的情况。

因此 Claude 的 hook_configured 与 prompt_configured 必须同时为 true，claude_configured 才为 true。

## 四、启动事务与回滚

用户点击“启动”后，code-Manager 按以下顺序执行：

1. 获取 RTK/snip 互斥锁，确认 snip 没有激活。
2. 读取 RTK 状态、用户 PATH 和系统 PATH。
3. 快照所有已存在或可由当前平台目录创建的提示词与 Hook 文件。
4. 配置用户 PATH 与系统 PATH；系统 PATH 需要权限时请求 UAC。
5. 释放 RTK 的完整参考、Codex 提示词资源和 Claude 提示词资源。
6. 写入 Codex 的 AGENTS.md 标记段，以及 Claude 的 CLAUDE.md 标记段；内容匹配已知旧版 RTK 提示词的标记段会迁移为当前内嵌规则，未知标记段不覆盖。
7. 写入 Claude、Copilot、Cursor 的官方格式 Hook。
8. 把本次实际写入或迁移的提示词段、Hook 目标文件和完整命令写入 `metadata.rtk_ownership_v1`，并写入 running=true、desired_running=true；`owned_agents` 只保留为兼容摘要。

任意可写目标的解析、写入、PATH 配置或状态保存失败时，程序必须恢复本次的平台文件快照并撤销本次新增 PATH。没有已存在目录的平台会被跳过；跳过不是错误，也不能被描述为“该平台接入成功”。

## 五、状态接口与页面语义

GET /api/rtk 返回安装、PATH、平台状态和激活状态。前端的状态含义如下：

| 字段 | 含义 |
|---|---|
| running / desired_running | 已提交或期望恢复的激活状态，不代表四平台完整接入 |
| activation_state | 页面显示所用的安装、已激活、注意事项或冲突状态 |
| codex_available / codex_configured | Codex 目录或规则段的检测结果 |
| claude_available | Claude Code 配置目录是否存在 |
| claude_hook_configured | Claude Hook 是否匹配 |
| claude_prompt_configured | CLAUDE.md 提示词标记段是否匹配 |
| claude_configured | Claude Hook 与提示词是否同时匹配 |
| copilot_available / copilot_configured | Copilot 目录与 Hook 的检测结果 |
| cursor_available / cursor_configured | Cursor 目录与 Hook 的检测结果 |
| modified_agents | 精确账本记录且当前仍存在受管项目的平台 |

RTK 页签的绿色标签显示“已激活”。其下方必须逐项展示 Codex、Claude Code、Cursor、Copilot 的实际状态；Claude Code 必须分别展示 Hook 与提示词状态。Codex 状态仅检查 `AGENTS.md` 的 RTK 标记段，不读取个性化界面文本。不得把 running=true、PATH 已配置或某一个平台已配置解释为四平台全部完成。

RTK 没有后台 daemon。因此“停止”撤销的是后续命令处理所需的 PATH、提示词和 Hook，不会终止正在执行的外部 Shell 命令。

## 六、停止与卸载边界

停止流程优先只处理精确账本 `metadata.rtk_ownership_v1` 记录的项目：

1. 移除本程序拥有的用户 PATH 与系统 PATH 条目。
2. 按绝对目标路径和提示词内容 SHA-256 指纹删除 Codex AGENTS.md、Claude CLAUDE.md 中的受管标记段。
3. 按绝对目标文件和完整命令删除 Claude、Copilot、Cursor 的受管 Hook，不宽匹配其它 `rtk.exe` 路径。
4. 旧状态只有 `owned_agents` 时，仅恢复仍能明确识别的旧提示词或当前发布目录命令；未知标记段和手工内容不接管、不删除。
5. 将 running、desired_running、PATH 归属更新为停止状态。

卸载要求 RTK 先停止，且 rtk.exe 没有正在执行。随后按受管归属清理配置和 PATH，删除名称精确为 RTK-AI 的专属目录，并进行复查。用户原有的其它提示词段、其它 Hook、手工 PATH 条目和未受管平台内容不得被删除。

## 七、开发时的关联检查

修改 RTK 代码、资源、UI 或文档时，至少检查以下关联：

1. 嵌入资源：assets 中的完整参考、Codex 提示词和 Claude 提示词均由 rtk_codex_commands.go 写入安装目录。
2. Codex：assistant_integration.go 只在已有 AGENTS.md 中写入匹配的标记段。
3. Claude：assistant_integration.go 管理 CLAUDE.md；rtk_native_integrations.go 管理 settings.json Hook；两者共同决定完整状态。
4. 快照与回滚：rtk_extra_integrations.go 需要同时覆盖 Claude 提示词和 Claude Hook。
5. 生命周期：rtk_install.go 的启动、停止、卸载都要按 `metadata.rtk_ownership_v1` 处理提示词和 Hook，旧 `owned_agents` 只能用于安全恢复。
6. 页面：frontend/src/App.vue 要展示激活状态与每个平台的实际状态；窄屏下文字必须可换行。
7. 文档：README.txt 与本文必须使用“已激活不等于四平台完成”的同一表述。

## 八、聚焦验证

本项目的常规验证保持聚焦，不运行无关的长测试：

~~~bat
gofmt -w assistant_integration.go assistant_integration_test.go rtk_ownership.go rtk_ownership_test.go rtk_install.go rtk_native_integrations.go rtk_native_integrations_test.go rtk_extra_integrations.go rtk_extra_integrations_test.go
go test -run "Test(InstallAssistantIntegrations|RTKPromptOwnership|RTKOwnershipLedger|RecoverLegacyRTKOwnership|RTKClaude|RTKCopilot|RTKCursor)" .
pushd frontend
npm.cmd run build
popd
~~~

发布前应在 RTK 页签确认以下场景：

- 没有任何平台目录时，页面可以显示“已激活”，但四个平台均显示未检测到。
- 只有 Codex 或只有 Claude Code 存在时，页面只显示对应平台的接入结果。
- Claude Hook 存在而 CLAUDE.md 提示词缺失时，Claude 不得显示为完整接入。
- Claude 提示词存在而 Hook 缺失时，Claude 不得显示为完整接入。
- 停止后，RTK 自己写入的 Codex/Claude 提示词和 Claude/Copilot/Cursor Hook 均被清理，用户其它内容与未受管内容保留。

## 九、代码入口

- rtk_install.go：RTK 状态接口、安装、启动、停止、卸载、PATH 与互斥处理。
- assistant_integration.go：Codex/Claude 提示词标记段、UTF-8 校验和原子写入。
- rtk_ownership.go：RTK 提示词指纹、Hook 完整命令和旧状态安全恢复。
- rtk_native_integrations.go：Claude/Copilot Hook 的 JSON 读写、检测和清理。
- rtk_extra_integrations.go：Cursor Hook、平台文件快照和回滚。
- rtk_codex_commands.go：嵌入与释放 RTK 命令参考、Codex 提示词和 Claude 提示词。
- frontend/src/App.vue：RTK 状态展示、平台明细和控制操作。
- README.txt：工程总览与跨模块维护约束。
