更新说明：
1. 版本升级至 v1.0.21，同步更新前后端与 vision 配置。
2. 优化 Gortex 进程生命周期：
   - 采用独立守护模式（Daemon Mode），主程序退出及热重载时保持 Gortex daemon 常驻后台；
   - 避免外部开发工具与宿主代理（如 Google Antigravity）因管理端退出而发生连接中断。
3. 全量重新编译打包最新单一可执行二进制产物（code-Manager-rust.exe）并同步发布至 GitHub 远端。
