# RTK

Claude Code 的 `PreToolUse` Hook 会在安全的情况下自动把可处理的 Shell 命令改写为对应的 `rtk` 命令。
不要手工重复添加 `rtk` 前缀，也不要假定所有命令都能被改写；Hook 未改写时按原命令执行。

以下 RTK 元命令需要直接使用：

```bash
rtk gain
rtk gain --history
rtk discover
rtk proxy <command>
```

遇到未列参数、RTK 版本变更或命令行为不明确时，先运行 `rtk --help` 或对应子命令的 `--help`。需要保留原始输出、交互式终端、PowerShell 表达式、重定向、管道或用户明确要求原生命令时，直接执行原命令。
