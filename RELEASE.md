# code-Manager (Rust) 规范化发布流程指南

本文档详细记录了 **code-Manager (Rust 重构版)** 从**版本号修改**、**全量本地构建**、**Git 提交与打标签**，到最终**一键自动化发布至 GitHub Releases** 的标准全流程。

---

## 📋 发布核心原则

1. **版本号统一**：项目采用语义化版本号（Semantic Versioning），格式为 `vX.Y.Z`。
2. **单一产物名称铁律**：GitHub Release 的 Windows 可执行文件资产名称**严格保持为 `code-Manager-rust.exe`**，不得擅自修改。
3. **保留历史资产**：原版 Go 的所有历史 Tags、Releases 以及 `legacy-go` 分支必须受到严格保护，不可删除。

---

## 🔄 完整发布步骤（五步法）

### 第一步：同步修改版本号 (3 处同步)

每次发布新版本时，需要同步修改以下三个文件中的版本号（以 `1.0.2` 为例）：

1. **统一控制源 `vision.md`**（发布脚本核心读取源）：
   ```markdown
   vision: v1.0.2
   ```

2. **后端配置 `backend/Cargo.toml`**：
   ```toml
   [package]
   name = "edit-rust"
   version = "1.0.2"
   ```

3. **前端配置 `frontend/package.json`**：
   ```json
   {
     "name": "code-manager-ui",
     "version": "1.0.2"
   }
   ```

---

### 第二步：本地一键全量编译与打包

在项目根目录下运行一键打包脚本：
```cmd
build.bat
```

该脚本会自动执行：
1. `npm run build`：编译 Vue 3 前端静态产物至 `frontend/dist/`；
2. `cargo build --release`：Rust 编译器将前端产物静态内嵌编译为单一可执行文件；
3. 输出最新产物至 `releases\code-Manager-rust.exe`。

> 💡 **自查确认**：检查 `releases\code-Manager-rust.exe` 是否成功生成且大小约为 15~16 MB。

---

### 第三步：提交变更并推送到 GitHub `main` 分支

在终端执行 Git 提交操作：

```bash
# 1. 查看暂存变动
git status

# 2. 追踪并提交所有代码与产物变动
git add -A
git commit -m "chore: release v1.0.2"

# 3. 推送至 GitHub main 分支
git push origin main
```

---

### 第四步：创建 Git 标签并推送到远端

在本地为最新提交打上对应版本的 Release Tag：

```bash
# 创建附注标签
git tag -a v1.0.2 -m "Release v1.0.2"

# 推送标签至 GitHub
git push origin v1.0.2
```

---

### 第五步：自动化发布 GitHub Release

在 PowerShell 中直接运行根目录下的自动化发布脚本：

```powershell
.\publish_release.ps1
```

**该脚本全自动执行以下操作**：
1. 自动从 `vision.md` 提取版本号（如 `v1.0.2`）；
2. 校验 `releases\code-Manager-rust.exe` 并计算其准确的 **SHA-256** 哈希值；
3. 通过 Windows 本地 Git 凭证管理器自动获取 GitHub 权限；
4. 调用 GitHub REST API 创建或更新 Release，并自动将其标记为 **Latest**；
5. 自动上传名为 **`code-Manager-rust.exe`** 的二进制资产（若同名旧资产存在会自动清理重传）；
6. 校验远端 Digest 与上传状态。

---

## 🛠️ 故障排查与常见问题

### Q1: `publish_release.ps1` 提示无法获取 GitHub Token？
- **原因**：本地尚未通过 Git Credential Manager 登录 GitHub。
- **解决办法**：在终端执行一次 `git push`，在弹出的浏览器窗口中完成 GitHub 授权登录即可。

### Q2: 上传资产时网络中断或超时？
- **解决办法**：由于 `publish_release.ps1` 具有完全的**幂等性**，如遇网络中断，只需重新运行 `.\publish_release.ps1`，脚本会自动检测已创建的 Release，清掉未完成的残缺资产并重新上传。
