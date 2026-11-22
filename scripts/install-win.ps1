<#
  Eryu Windows 安装脚本（每用户安装，不需要管理员）
  用法（解压后在目录里）：
    powershell -ExecutionPolicy Bypass -File install.ps1
    powershell -ExecutionPolicy Bypass -File install.ps1 -Desktop      # 额外建桌面图标
  安装位置：%LOCALAPPDATA%\Programs\Eryu\Eryu.exe
  数据位置：%APPDATA%\Eryu（模型/配置/历史；卸载默认保留）
#>
param(
    [switch]$Desktop,
    [string]$Exe = "Eryu.exe"
)
$ErrorActionPreference = 'Stop'

# $Exe 可以是绝对路径、相对当前目录的路径，或留空（用脚本同目录的 Eryu.exe）
$src = $Exe
if (-not [System.IO.Path]::IsPathRooted($src)) {
    $cwd = Join-Path (Get-Location).Path $src
    $here = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) $src
    $src = if (Test-Path $cwd) { $cwd } else { $here }
}
if (-not (Test-Path $src)) { throw "找不到 $src（请把 Eryu.exe 和 install.ps1 放在同一目录）" }

$dir = Join-Path $env:LOCALAPPDATA 'Programs\Eryu'
New-Item -ItemType Directory -Force -Path $dir | Out-Null

# 正在跑就先关掉，避免覆盖失败
Get-Process Eryu -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 400

$dst = Join-Path $dir 'Eryu.exe'
# 覆盖正在运行的 exe 会「静默失败」（Copy-Item 不报错、装完还是旧版）。
# 拷完比哈希，不一致就等一下重试，三次还不行就明确报错，别让用户以为升级成功。
$srcHash = (Get-FileHash $src -Algorithm SHA256).Hash
$copied = $false
for ($i = 0; $i -lt 3; $i++) {
    try { Copy-Item $src $dst -Force } catch { Start-Sleep -Milliseconds 800; continue }
    if ((Get-FileHash $dst -Algorithm SHA256).Hash -eq $srcHash) { $copied = $true; break }
    Start-Sleep -Milliseconds 800
}
if (-not $copied) {
    throw "写不进 $dst（可能还有 Eryu 进程占用）。请完全退出 Eryu（托盘右键 → 退出）后重跑安装。"
}

# 绿色模式：exe 旁边没有 portable.txt / EryuData 时数据在 %APPDATA%\Eryu
$shell = New-Object -ComObject WScript.Shell
$startMenu = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs'
New-Item -ItemType Directory -Force -Path $startMenu | Out-Null
$lnk = $shell.CreateShortcut((Join-Path $startMenu 'Eryu.lnk'))
$lnk.TargetPath = $dst
$lnk.WorkingDirectory = $dir
$lnk.Description = 'Eryu 语音输入'
$lnk.Save()

if ($Desktop) {
    $desk = [Environment]::GetFolderPath('Desktop')
    $lnk2 = $shell.CreateShortcut((Join-Path $desk 'Eryu.lnk'))
    $lnk2.TargetPath = $dst
    $lnk2.WorkingDirectory = $dir
    $lnk2.Save()
}

# 随包附带的语音模型：拷到数据目录（已存在就跳过，不覆盖用户自己的）
$modelSrc = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) 'models'
if (Test-Path $modelSrc) {
    $modelDst = Join-Path $env:APPDATA 'Eryu\models'
    New-Item -ItemType Directory -Force -Path $modelDst | Out-Null
    # 判据不能是「目录存在」：之前下到一半/被清掉的目录会让安装器以为装好了，
    # 结果装完还是没模型。以主模型文件（model.int8.onnx，>100MB）为准，缺就补齐。
    Get-ChildItem $modelSrc -Directory | ForEach-Object {
        $target = Join-Path $modelDst $_.Name
        $main = Join-Path $target 'model.int8.onnx'
        $ok = (Test-Path $main) -and ((Get-Item $main).Length -gt 100MB)
        if ($ok) {
            Write-Output "语音模型已存在：$($_.Name)"
        } else {
            New-Item -ItemType Directory -Force -Path $target | Out-Null
            Copy-Item (Join-Path $_.FullName '*') $target -Recurse -Force
            Write-Output "已安装语音模型：$($_.Name)"
        }
    }
}

$ver = (Get-Item $dst).VersionInfo.FileVersion
$verText = if ($ver) { "  (version=$ver)" } else { "" }
Write-Output "INSTALL OK  $dst$verText"
Write-Output "启动：开始菜单里搜 Eryu；数据在 $env:APPDATA\Eryu"
