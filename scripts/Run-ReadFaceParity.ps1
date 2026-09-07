<#
.SYNOPSIS
  一键跑供数模式对拍探针：`plant-ui-app --read-face-parity`（2026-09-07 计划 §5.4 / M4）。

.DESCRIPTION
  同一进程里各起一份服务供数与库供数的读面，对着同一个接入点把树、属性、三维实例各读一遍，
  差异只列不判，报告落成 Markdown。要活的模型服务（gen-model）与 SurrealDB，所以不进 CI。

  接入点从哪儿来，与界面同一条：资产根下 config/e3d.project.ron 给库地址与账号
  （没有就回落到工作目录的 DbOption.toml）；模型服务地址按 settings.ron > PLANT_MODEL_API_URL
  > 出厂默认，-Service 压过全部。

.PARAMETER Service
  模型服务地址，如 http://127.0.0.1:8022。不给就按上面的优先级。
.PARAMETER Depth
  往下比几层子节点（0 = 只比 SITE 根层）。默认 2。
.PARAMETER Sample
  抽多少个两边都有的元素比属性。默认 200。
.PARAMETER Roots
  三维实例的抽样根（逗号分隔的 refno，如 24381/2,24381/3）；不给就从第 1 层共有节点里等距抽。
.PARAMETER Out
  报告路径。默认 docs/evidence/<今天>-read-face-parity.md；给 "-" 打到标准输出。
.PARAMETER Exe
  现成的 plant-ui-app.exe。不给就 cargo build -p plant-ui-app（-Release 走 release）后用 target 里那一个。
.PARAMETER AssetRoot
  资产根（PLANT_ASSET_ROOT）。默认仓库的 web/public/assets。
.PARAMETER SettingsFile
  设置文件（PLANT_UI_SETTINGS_FILE，必须是绝对路径）。不给就用 exe 旁 config/settings.ron。
.PARAMETER Release
  按 release 构建 / 取 release 下的 exe。

.EXAMPLE
  .\scripts\Run-ReadFaceParity.ps1 -Service http://127.0.0.1:8022
.EXAMPLE
  .\scripts\Run-ReadFaceParity.ps1 -Depth 1 -Sample 50 -Out -
#>
param(
  [string]$Service,
  [int]$Depth = 2,
  [int]$Sample = 200,
  [string]$Roots,
  [string]$Out,
  [string]$Exe,
  [string]$AssetRoot,
  [string]$SettingsFile,
  [switch]$Release
)

$ErrorActionPreference = "Stop"
$uiRoot = Split-Path $PSScriptRoot -Parent

if (-not $Exe) {
  Push-Location $uiRoot
  try {
    $buildArgs = @("build", "-p", "plant-ui-app")
    if ($Release) { $buildArgs += "--release" }
    cargo @buildArgs
    if ($LASTEXITCODE) { exit $LASTEXITCODE }
    $targetDir = (cargo metadata --no-deps --format-version 1 | ConvertFrom-Json).target_directory
  } finally { Pop-Location }
  $profile = if ($Release) { "release" } else { "debug" }
  $Exe = Join-Path $targetDir "$profile\plant-ui-app.exe"
}
if (-not (Test-Path -LiteralPath $Exe -PathType Leaf)) { throw "找不到 plant-ui-app：$Exe" }

if (-not $AssetRoot) { $AssetRoot = Join-Path $uiRoot "web\public\assets" }
if (-not (Test-Path -LiteralPath $AssetRoot -PathType Container)) { throw "资产根不存在：$AssetRoot" }
$env:PLANT_ASSET_ROOT = $AssetRoot
if ($SettingsFile) { $env:PLANT_UI_SETTINGS_FILE = $SettingsFile }

if (-not $Out) {
  $Out = Join-Path $uiRoot ("docs\evidence\{0}-read-face-parity.md" -f (Get-Date -Format "yyyy-MM-dd"))
}

$probeArgs = @("--read-face-parity", "--depth", $Depth, "--sample", $Sample)
if ($Roots) { $probeArgs += @("--roots", $Roots) }
if ($Service) { $probeArgs += @("--service", $Service) }
if ($Out -ne "-") { $probeArgs += @("--out", $Out) }

& $Exe @probeArgs
exit $LASTEXITCODE
