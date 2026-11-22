<#
  Eryu Windows 卸载脚本（每用户安装）
  用法：powershell -ExecutionPolicy Bypass -File uninstall.ps1 [-PurgeData]
  默认保留 %APPDATA%\Eryu 里的配置/模型/历史。
#>
param([switch]$PurgeData)
$ErrorActionPreference = 'Continue'

Get-Process Eryu -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 400

$dir = Join-Path $env:LOCALAPPDATA 'Programs\Eryu'
if (Test-Path $dir) { Remove-Item $dir -Recurse -Force }

$startMenu = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Eryu.lnk'
if (Test-Path $startMenu) { Remove-Item $startMenu -Force }
$desk = Join-Path ([Environment]::GetFolderPath('Desktop')) 'Eryu.lnk'
if (Test-Path $desk) { Remove-Item $desk -Force }

# 开机自启登记（如果开过）
$run = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
Remove-ItemProperty -Path $run -Name 'Eryu' -ErrorAction SilentlyContinue

if ($PurgeData) {
    $data = Join-Path $env:APPDATA 'Eryu'
    if (Test-Path $data) { Remove-Item $data -Recurse -Force }
    Write-Output 'UNINSTALL OK（数据和模型也删了）'
} else {
    Write-Output "UNINSTALL OK（保留 $env:APPDATA\Eryu；要一起删加 -PurgeData）"
}
