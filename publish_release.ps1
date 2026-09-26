# publish_release.ps1 - GitHub Release 自动化发布脚本 (Rust 重构版)
# 用途：读取当前版本号、校验二进制产物、同步发布至 GitHub Releases 并挂载 code-Manager-rust.exe

$ErrorActionPreference = "Stop"

$root = $PSScriptRoot
if (-not $root) { $root = Get-Location }

Write-Host "==================================================" -ForegroundColor Cyan
Write-Host "  code-Manager-rust 自动化 Release 发布流程" -ForegroundColor Cyan
Write-Host "==================================================" -ForegroundColor Cyan

# 1. 从 vision.md 读取发布版本号
$visionPath = Join-Path $root "vision.md"
if (-not (Test-Path $visionPath)) {
    Write-Error "找不到版本配置文件: $visionPath"
    exit 1
}

$visionContent = (Get-Content -Path $visionPath -Raw -Encoding UTF8).Trim()
if ($visionContent -match "^vision:\s*(v\d+\.\d+\.\d+.*)$") {
    $tag = $matches[1].Trim()
} else {
    Write-Error "vision.md 中的版本号格式不正确: $visionContent (期望格式例如: vision: v1.0.1)"
    exit 1
}

Write-Host "[1/5] 目标发布版本 Tag: $tag" -ForegroundColor Green

# 2. 检查待发布的二进制可执行文件
$exePath = Join-Path $root "releases\code-Manager-rust.exe"
if (-not (Test-Path $exePath)) {
    Write-Error "未找到构建产物: $exePath。请先运行 build.bat 完成编译打包！"
    exit 1
}

$localHash = (Get-FileHash -Path $exePath -Algorithm SHA256).Hash.ToLower()
$localSize = (Get-Item $exePath).Length
Write-Host "[2/5] 本地可执行文件: $exePath"
Write-Host "      大小 (Bytes)  : $localSize"
Write-Host "      SHA-256 哈希  : $localHash" -ForegroundColor Yellow

# 3. 通过 Git Credential Manager 获取 GitHub 访问凭据
Write-Host "[3/5] 获取 GitHub 凭证..."
$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = "C:\Program Files\Git\mingw64\bin\git-credential-manager.exe"
$psi.Arguments = "get"
$psi.RedirectStandardInput = $true
$psi.RedirectStandardOutput = $true
$psi.RedirectStandardError = $true
$psi.UseShellExecute = $false
$p = [System.Diagnostics.Process]::Start($psi)
$p.StandardInput.WriteLine("protocol=https")
$p.StandardInput.WriteLine("host=github.com")
$p.StandardInput.WriteLine("")
$p.StandardInput.Close()
$credOutput = $p.StandardOutput.ReadToEnd()
$p.WaitForExit()

$token = ""
foreach ($line in ($credOutput -split "`r?`n")) {
    if ($line -match "^password=(.*)$") {
        $token = $matches[1].Trim()
    }
}

if (-not $token) {
    Write-Error "未能从 Git 凭证管理器中提取 GitHub Token，请确保已登录 GitHub。"
    exit 1
}

$repo = "nicelic/codex-manager"
$headers = @{
    "Authorization" = "token $token"
    "Accept"        = "application/vnd.github.v3+json"
    "User-Agent"    = "code-manager-rust-publisher"
}

# 4. 检查或创建 GitHub Release
Write-Host "[4/5] 检查/创建 GitHub Release ($tag)..."
$release = $null
try {
    $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$repo/releases/tags/$tag" -Headers $headers -Method Get
    Write-Host "      找到已存在的 Release (ID: $($release.id))" -ForegroundColor Cyan
} catch {
    Write-Host "      Release $tag 尚未创建，正在新建..." -ForegroundColor Cyan
}

$releaseNotes = @"
$tag release. Windows executable included.
SHA-256: $localHash
code-Manager-rust.exe

更新说明：
1.使用rust重构面板后端。
2.gortex提示词改进。
"@

if (-not $release) {
    $bodyObj = @{
        tag_name         = $tag
        target_commitish = "main"
        name             = $tag
        body             = $releaseNotes
        draft            = $false
        prerelease       = $false
        make_latest      = "true"
    }
    $bodyJson = $bodyObj | ConvertTo-Json
    $bodyBytes = [System.Text.Encoding]::UTF8.GetBytes($bodyJson)
    $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$repo/releases" -Headers $headers -Method Post -Body $bodyBytes -ContentType "application/json; charset=utf-8"
    Write-Host "      Release 创建成功！(ID: $($release.id))" -ForegroundColor Green
} else {
    # 更新已有 Release 的说明
    $updateObj = @{
        body        = $releaseNotes
        make_latest = "true"
    }
    $updateJson = $updateObj | ConvertTo-Json
    $updateBytes = [System.Text.Encoding]::UTF8.GetBytes($updateJson)
    Invoke-RestMethod -Uri "https://api.github.com/repos/$repo/releases/$($release.id)" -Headers $headers -Method Patch -Body $updateBytes -ContentType "application/json; charset=utf-8" | Out-Null
    Write-Host "      Release 信息与说明已更新！" -ForegroundColor Green
}

# 5. 上传或覆盖二进制产物 code-Manager-rust.exe
Write-Host "[5/5] 检查并挂载可执行文件资产 (code-Manager-rust.exe)..."
$existingAsset = $release.assets | Where-Object { $_.name -eq "code-Manager-rust.exe" }
if ($existingAsset) {
    Write-Host "      发现旧资产 (Asset ID: $($existingAsset.id))，正在移除..." -ForegroundColor Yellow
    Invoke-RestMethod -Uri "https://api.github.com/repos/$repo/releases/assets/$($existingAsset.id)" -Headers $headers -Method Delete | Out-Null
}

Write-Host "      正在上传最新 code-Manager-rust.exe 到 GitHub Release..." -ForegroundColor Cyan
$uploadUri = $release.upload_url -replace '\{\?name,label\}', '?name=code-Manager-rust.exe'
$uploadHeaders = @{
    "Authorization" = "token $token"
    "Accept"        = "application/vnd.github.v3+json"
    "User-Agent"    = "code-manager-rust-publisher"
    "Content-Type"  = "application/octet-stream"
}

$uploadedAsset = Invoke-RestMethod -Uri $uploadUri -Headers $uploadHeaders -Method Post -InFile $exePath
Write-Host "      资产上传成功！(Asset ID: $($uploadedAsset.id), 大小: $($uploadedAsset.size) bytes)" -ForegroundColor Green

Write-Host "==================================================" -ForegroundColor Green
Write-Host "  发布已圆满完成！" -ForegroundColor Green
Write-Host "  Release 地址: https://github.com/$repo/releases/tag/$tag" -ForegroundColor Cyan
Write-Host "==================================================" -ForegroundColor Green
