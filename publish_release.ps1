# publish_release.ps1 - Local GitHub Release publisher for code-Manager-rust
$ErrorActionPreference = "Stop"

$root = $PSScriptRoot
if (-not $root) { $root = Get-Location }

Write-Host "=================================================="
Write-Host "code-Manager-rust Release Publisher"
Write-Host "=================================================="

# 1. Read version from vision.md
$visionPath = Join-Path $root "vision.md"
if (-not (Test-Path $visionPath)) {
    Write-Error "vision.md not found at $visionPath"
    exit 1
}

$visionContent = (Get-Content -Path $visionPath -Raw -Encoding UTF8).Trim()
if ($visionContent -match "^vision:\s*(v\d+\.\d+\.\d+.*)$") {
    $tag = $matches[1].Trim()
} else {
    Write-Error "Invalid version format in vision.md: $visionContent"
    exit 1
}

Write-Host "Target Release Version: $tag"

# 2. Check binary existence and hash
$exePath = Join-Path $root "releases\code-Manager-rust.exe"
if (-not (Test-Path $exePath)) {
    Write-Error "Executable not found at $exePath. Run build.bat first."
    exit 1
}

$localHash = (Get-FileHash -Path $exePath -Algorithm SHA256).Hash.ToLower()
$localSize = (Get-Item $exePath).Length
Write-Host "Local executable: $exePath"
Write-Host "Local SHA-256   : $localHash"
Write-Host "Local Size      : $localSize bytes"

# 3. Retrieve GitHub Token via git-credential-manager
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
    Write-Error "Failed to retrieve GitHub Token from Git Credential Manager"
    exit 1
}

$repo = "nicelic/codex-manager"
$headers = @{
    "Authorization" = "token $token"
    "Accept"        = "application/vnd.github.v3+json"
    "User-Agent"    = "codex-manager-rust-publisher"
}

# 4. Read custom notes if present
$notesPath = Join-Path $root "RELEASE_NOTES.md"
$customNotes = ""
if (Test-Path $notesPath) {
    $customNotes = (Get-Content -Path $notesPath -Raw -Encoding UTF8).Trim()
}

# 4.1 Prepare .git bundle and full repository zip with .git
$releasesDir = Join-Path $root "releases"
if (-not (Test-Path $releasesDir)) { New-Item -ItemType Directory -Path $releasesDir | Out-Null }

$zipName = "code-Manager-rust-$tag-with-git.zip"
$zipPath = Join-Path $releasesDir $zipName

$bundleName = "codex-manager-backup-$tag.bundle"
$bundlePath = Join-Path $releasesDir $bundleName

Write-Host "Creating Git Bundle ($bundleName)..."
& git bundle create $bundlePath --all
if ($LASTEXITCODE -ne 0) {
    Write-Error "Failed to create git bundle at $bundlePath"
    exit 1
}

Write-Host "Creating full source archive with .git ($zipName)..."
if (Test-Path $zipPath) { Remove-Item -Force $zipPath }
& tar -a -c -f $zipPath `
    --exclude="backend/target" `
    --exclude="frontend/node_modules" `
    --exclude="frontend/dist" `
    --exclude="releases/*.zip" `
    --exclude="releases/*.bundle" `
    .git backend frontend releases .gitignore .gortex.yaml build.bat publish_release.ps1 README.md RELEASE.md RELEASE_NOTES.md vision.md
if ($LASTEXITCODE -ne 0 -or -not (Test-Path $zipPath)) {
    Write-Error "Failed to create source archive with .git at $zipPath"
    exit 1
}

$zipHash = (Get-FileHash -Path $zipPath -Algorithm SHA256).Hash.ToLower()
$zipSize = (Get-Item $zipPath).Length
$bundleHash = (Get-FileHash -Path $bundlePath -Algorithm SHA256).Hash.ToLower()
$bundleSize = (Get-Item $bundlePath).Length

$bodyText = "$tag release. Windows executable included.`r`nSHA-256: $localHash`r`ncode-Manager-rust.exe`r`n`r`nSHA-256: $zipHash`r`n$zipName (Source code with .git & backup branches)`r`n`r`nSHA-256: $bundleHash`r`n$bundleName (Complete offline Git bundle)"
if ($customNotes) {
    $bodyText = "$bodyText`r`n`r`n$customNotes"
}

# 5. Check or Create Release
$release = $null
try {
    $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$repo/releases/tags/$tag" -Headers $headers -Method Get
    Write-Host "Found existing Release (ID: $($release.id), Name: $($release.name))"
} catch {
    Write-Host "Release $tag not found, creating new release..."
}

if (-not $release) {
    $bodyObj = @{
        tag_name         = $tag
        target_commitish = "main"
        name             = $tag
        body             = $bodyText
        draft            = $false
        prerelease       = $false
        make_latest      = "true"
    } | ConvertTo-Json
    $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$repo/releases" -Headers $headers -Method Post -Body ([System.Text.Encoding]::UTF8.GetBytes($bodyObj)) -ContentType "application/json; charset=utf-8"
    Write-Host "Release created successfully (ID: $($release.id))"
} else {
    $updateObj = @{
        body        = $bodyText
        make_latest = "true"
    } | ConvertTo-Json
    Invoke-RestMethod -Uri "https://api.github.com/repos/$repo/releases/$($release.id)" -Headers $headers -Method Patch -Body ([System.Text.Encoding]::UTF8.GetBytes($updateObj)) -ContentType "application/json; charset=utf-8" | Out-Null
    Write-Host "Release metadata updated"
}

# 6. Check or Upload Assets
function Upload-ReleaseAsset {
    param(
        [string]$FilePath,
        [string]$AssetName
    )
    if (-not (Test-Path $FilePath)) {
        Write-Error "Asset file not found: $FilePath"
        exit 1
    }
    $existing = $release.assets | Where-Object { $_.name -eq $AssetName }
    if ($existing) {
        if ($existing.size -eq (Get-Item $FilePath).Length -and $existing.state -eq "uploaded") {
            Write-Host "Asset $AssetName already exists and size matches ($($existing.size) bytes), skipping upload."
            return
        }
        Write-Host "Asset $AssetName already exists (ID: $($existing.id)), removing to update..."
        Invoke-RestMethod -Uri "https://api.github.com/repos/$repo/releases/assets/$($existing.id)" -Headers $headers -Method Delete -TimeoutSec 120 | Out-Null
    }

    Write-Host "Uploading asset $AssetName ($((Get-Item $FilePath).Length) bytes)..."
    $uploadUri = $release.upload_url -replace '\{\?name,label\}', "?name=$AssetName"
    $uploadHeaders = @{
        "Authorization" = "token $token"
        "Accept"        = "application/vnd.github.v3+json"
        "User-Agent"    = "codex-manager-rust-publisher"
        "Content-Type"  = "application/octet-stream"
    }

    $uploaded = Invoke-RestMethod -Uri $uploadUri -Headers $uploadHeaders -Method Post -InFile $FilePath -TimeoutSec 1800
    Write-Host "Asset $AssetName uploaded successfully (ID: $($uploaded.id), Size: $($uploaded.size) bytes)"
}

Upload-ReleaseAsset -FilePath $exePath -AssetName "code-Manager-rust.exe"
Upload-ReleaseAsset -FilePath $zipPath -AssetName $zipName
Upload-ReleaseAsset -FilePath $bundlePath -AssetName $bundleName

Write-Host "=================================================="
Write-Host "Release verification succeeded!"
Write-Host "URL: https://github.com/$repo/releases/tag/$tag"
Write-Host "=================================================="
