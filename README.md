# code-Manager (Rust 重构版)

<p align="center">
  <img src="https://img.shields.io/badge/Language-Rust%20%7C%20Vue%203-orange?style=flat-square" alt="Language">
  <img src="https://img.shields.io/badge/Platform-Windows-blue?style=flat-square&logo=windows" alt="Platform">
  <img src="https://img.shields.io/github/v/release/nicelic/codex-manager?style=flat-square&color=success" alt="Latest Release">
  <img src="https://img.shields.io/github/license/nicelic/codex-manager?style=flat-square" alt="License">
</p>

**code-Manager** 是一款专为 Windows 平台打造的本地高性能网关与 AI 辅助工具管理面板。

本项目已全面使用 **Rust (Axum + Tokio)** 替代原有的 Go 后端，并将 **Vue 3** 现代化管理前端在编译期全量静态内嵌，输出为**单一、轻量、无需配置运行时的 Windows `.exe` 可执行文件**。

> [!NOTE]
> 原版 Go 实现的源码已完整归档在 [`legacy-go`](https://github.com/nicelic/codex-manager/tree/legacy-go) 分支，历史版本标签与记录永久保留。

---

## ✨ 核心特性

- **🚀 零依赖单可执行文件**：
  前端静态产物通过 `rust-embed` 全量内嵌进 `code-Manager-rust.exe`，无需安装 Node.js、Go 或 Python 运行时环境，双击即开即用。
- **⚡ 架构现代化重构 (Go ➔ Rust)**：
  - 内存常驻与系统开销大幅降低，彻底摆脱 Go 运行时的 GC 波动与多余内存占用。
  - Windows 原生 API 深度互操作，系统托盘、注册表及进程控制更加稳定可靠。
- **🌐 本地网关与反向代理**：
  - 内置基于 Axum 的高性能 HTTP 反向代理引擎。
  - 深度支持 SSE (Server-Sent Events) 流式传输、网络波动自动重试与健康检查。
- **🧰 四大 AI 增强工具生态一键集成**：
  - **RTK (Rust Token Killer)**：智能上下文与 Token 压缩优化工具。
  - **Snip (代码快照切片器)**：语法级上下文提取与代码快照切片。
  - **LLMTrim**：大模型上下文高效修剪工具。
  - **Gortex**：全流程代码知识图谱分析、架构拓扑追踪与提示词增强集成。
- **🖥️ 贴心 Windows 原生集成**：
  - **端口防冲突自适应**：智能探测并占用空闲端口，服务就绪后自动唤起系统默认浏览器。
  - **单实例互斥机制 (Mutex)**：防止多次误双击启动多个实例。
  - **心跳守护与优雅退出**：前端面板关闭后自动心跳超时退出，亦可一键安全关闭。
  - **系统级开机自启与 PATH 配置**：支持通过注册表一键注入系统环境变量。

---

## 📥 下载与运行

1. 进入项目的 [GitHub Releases](https://github.com/nicelic/codex-manager/releases) 页面。
2. 下载最新版本的 `code-Manager-rust.exe`。
3. 双击运行 `code-Manager-rust.exe` 即可自动启动本地网关并唤起管理页面：
   - 默认访问地址：`http://127.0.0.1:7780`（如遇端口占用将自动递增寻址）。

---

## 🛠️ 从源码构建

### 环境准备
- **Rust**：1.80+ (推荐使用 `rustup` 安装 MSVC 工具链)
- **Node.js**：v18+ 及 `npm`
- **操作系统**：Windows 10 / 11

### 一键构建 (推荐)
仓库根目录下已内置自动化构建脚本：
```cmd
build.bat
```
脚本将按序执行：
1. `frontend/` 目录依赖安装与 Vue 3 前端静态产物编译（Vite 构建输出至 `dist`）；
2. `backend/` 目录 Cargo 生产环境编译（`cargo build --release`）；
3. 自动将内嵌完前端的独立二进制打包输出至 `releases\code-Manager-rust.exe`。

---

## 📂 项目结构

```text
codex-manager/
├── backend/                  # Rust 后端源码 (Axum + Tokio)
│   ├── Cargo.toml            # 后端依赖配置
│   ├── build.rs              # Windows 资源构建脚本
│   └── src/
│       ├── main.rs           # 程序入口、单实例互斥、端口自适应与生命周期控制
│       ├── state.rs          # 全局 AppState 与事件总线
│       ├── router.rs         # 核心 API 路由装配
│       ├── web.rs            # 前端静态资源内嵌路由 (rust-embed)
│       ├── api/              # 系统与配置 API 控制器
│       ├── common/           # 注册表、进程、网络下载与工具引擎
│       └── server/           # 反向代理引擎与 WebSocket 状态推送
├── frontend/                 # Vue 3 现代化管理面板源码 (Vite)
│   ├── src/                  # 页面组件与视图
│   └── package.json          # 前端依赖配置
├── releases/                 # 编译打包输出目录
│   └── code-Manager-rust.exe # 打包生成的 Windows 单可执行文件
├── build.bat                 # 本地全量一键编译脚本
├── publish_release.ps1       # GitHub Release 一键自动化发布脚本
├── RELEASE.md                # 规范化版本升级与发布全流程指南
├── vision.md                 # 当前发布版本号管理文件
└── README.md                 # 项目说明文档
```

---

## 🚀 发布与维护

关于版本号修改规范、构建校验与 GitHub Release 自动发布流程，请参阅：
👉 [版本发布指南 (RELEASE.md)](./RELEASE.md)

---

## 📜 许可证

本项目遵循开源许可证规范，详见 [LICENSE](./LICENSE) 文件。
