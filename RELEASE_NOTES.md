更新说明：
1. 版本升级至 v1.0.8，同步更新前后端与 vision 配置。
2. 增强 Gortex daemon 状态感知与生命周期管理：支持解析 daemon.state.json 中的 opening_store / migrating / starting / serving 等启动过渡阶段。
3. 前端界面新增 Gortex 启动中状态显示与自动轮询机制（pollGortexUntilReady），细化显示数据库迁移、数据校验与初始化状态，防止启动中重复触发。
4. 全量编译与打包最新二进制可执行产物，发布完整离线 Git bundle 及带 .git 源码归档。
