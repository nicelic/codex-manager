$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
$iconSource = Join-Path $root '297763_sort-by-icon.svg'
$iconRenderer = Join-Path $root 'tools\render-tray-icon.html'
$iconPng = Join-Path $root 'assets\tray.png'
$iconIco = Join-Path $root 'assets\tray.ico'

if (-not (Test-Path -LiteralPath $iconSource -PathType Leaf)) { throw "未找到托盘图标源文件: $iconSource" }
if (-not (Test-Path -LiteralPath $iconRenderer -PathType Leaf)) { throw "未找到图标渲染页: $iconRenderer" }

$edgeCandidates = @(
    'C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe',
    'C:\Program Files\Microsoft\Edge\Application\msedge.exe'
)
$edgePath = $edgeCandidates | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } | Select-Object -First 1
if (-not $edgePath) { throw '未找到 Microsoft Edge，无法将 SVG 转换为 Windows 托盘图标。' }

New-Item -ItemType Directory -Force -Path (Join-Path $root 'assets') | Out-Null
$renderUri = [System.Uri]::new($iconRenderer).AbsoluteUri
$edge = Start-Process -FilePath $edgePath -ArgumentList @(
    '--headless', '--disable-gpu', '--hide-scrollbars', '--force-device-scale-factor=1',
    '--default-background-color=00000000', "--screenshot=$iconPng", '--window-size=256,256',
    $renderUri
) -Wait -PassThru -WindowStyle Hidden
if ($edge.ExitCode -ne 0) { throw "托盘图标渲染失败，退出码: $($edge.ExitCode)" }

Push-Location $root
try {
    go run .\tools\icon-to-ico.go $iconPng $iconIco
}
finally {
    Pop-Location
}

Push-Location (Join-Path $root 'frontend')
try {
    npm.cmd install
    npm.cmd run build
}
finally {
    Pop-Location
}

Push-Location $root
try {
    gofmt -w main.go retry.go
    go mod tidy
    # GUI 子系统避免双击正式 EXE 时自动弹出控制台窗口；日志由页面按钮打开独立窗口查看。
    go build -tags http2legacy -ldflags "-H=windowsgui" -o code-Manager.exe .
}
finally {
    Pop-Location
}

Write-Host "已生成 $root\code-Manager.exe"
