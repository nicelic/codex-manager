更新说明：
1. 版本升级至 v1.0.39，同步更新前后端与 vision 配置。
2. 更新内置 `gortex提示词.md` 至 v704（批量优先编辑路由与分级降级版），确立 `edit.batch` 多处修改最高优先级、`edit.file(match/replacement)` 单次修改优选及“3 错降批、5 错切 `match`”两级熔断降级机制，并完善 `session`、`workspace_admin` 等门面参数契约。
3. 将 Gortex 外挂引导提示词默认分片行数 `DEFAULT_PROMPT_CHUNK_LINES` 调整为 200 行，并同步更新相关单元测试；清理旧版根目录 `开发.md` 与 `迁移.md`。
