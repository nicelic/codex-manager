更新说明：
1. 版本升级至 v1.0.26，同步更新前后端与 vision 配置。
2. 优化 Gortex 项目跟踪与彻底清理机制：
   - 新增生命周期并发互斥锁与协调暂停守卫（`ReconcilePauseGuard`），避免 track/untrack 与后台对账冲突并保留外部接入仓库；
   - 增强彻底 untrack 清理流程（`execute_gortex_untrack_purge`），清除 `config.yaml` 与数据库残留图谱并轮询确认完成；
   - 完善 Daemon 状态与项目状态细分（支持 `已 track`、`未 track`、`仅数据库残留`、`Daemon 已停止`、`空图谱` 等状态识别与记录清理）。
3. 优化 Gortex MCP 外挂引导提示词完整性验收：
   - 明确单次/分片读取后的首末行号完整性核验与缺失区间补读机制，避免重复重读或跳过未读部分。
4. 全量重新编译打包最新单一可执行二进制产物（code-Manager-rust.exe）并同步发布至 GitHub 远端。
