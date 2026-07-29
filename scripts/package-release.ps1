$ErrorActionPreference = "Stop"

$version = "0.1.4"
$uiRoot = Split-Path $PSScriptRoot -Parent
$backendRoot = Join-Path (Split-Path $uiRoot -Parent) "gen-model"
$releaseRoot = Join-Path $uiRoot "release\plant-suite-$version"
$backendRelease = Join-Path $releaseRoot "backend"
$pcRelease = Join-Path $releaseRoot "pc"
$env:CARGO_BUILD_JOBS = "1"

if (-not (Test-Path $backendRoot)) { throw "未找到后端工程: $backendRoot" }

Push-Location $uiRoot
try {
  cargo build -j 1 -p plant-ui-app --release
  if ($LASTEXITCODE) { exit $LASTEXITCODE }
  & (Join-Path $PSScriptRoot "build-wasm.ps1")
  if ($LASTEXITCODE) { exit $LASTEXITCODE }
} finally { Pop-Location }

Push-Location $backendRoot
try {
  cargo build -j 1 --release --features http_api
  if ($LASTEXITCODE) { exit $LASTEXITCODE }
} finally { Pop-Location }

if (Test-Path $releaseRoot) { Remove-Item -Recurse -Force $releaseRoot }
New-Item -ItemType Directory -Force $backendRelease, $pcRelease | Out-Null

Copy-Item (Join-Path $backendRoot "target\release\aios-database.exe") $backendRelease
Copy-Item (Join-Path $backendRoot "DbOption.toml") $backendRelease
Copy-Item (Join-Path $backendRoot "bin") $backendRelease -Recurse
Copy-Item (Join-Path $backendRoot "assets") $backendRelease -Recurse
Copy-Item (Join-Path $backendRoot "resource") $backendRelease -Recurse
Copy-Item (Join-Path $backendRoot "rs_surreal") $backendRelease -Recurse
Copy-Item (Join-Path $uiRoot "web\public") (Join-Path $backendRelease "web") -Recurse
Copy-Item (Join-Path $uiRoot "target\release\plant-ui-app.exe") $pcRelease
Copy-Item (Join-Path $uiRoot "DbOption.toml") $pcRelease

@'
$ErrorActionPreference = "Stop"
$root = $PSScriptRoot
$backend = Join-Path $root "backend"
$pc = Join-Path $root "pc"

Start-Process -FilePath (Join-Path $backend "bin\surreal.exe") -WorkingDirectory $backend -ArgumentList @("start", "--bind", "127.0.0.1:8009", "--user", "root", "--pass", "root", "rocksdb:./data/surreal")
for ($i = 0; $i -lt 30; $i++) {
  if (Test-NetConnection 127.0.0.1 -Port 8009 -InformationLevel Quiet) { break }
  Start-Sleep -Seconds 1
}
if (-not (Test-NetConnection 127.0.0.1 -Port 8009 -InformationLevel Quiet)) { throw "SurrealDB 未在 8009 启动" }

$env:PLANT_UI_WEB_ROOT = (Join-Path $backend "web")
Start-Process -FilePath (Join-Path $backend "aios-database.exe") -WorkingDirectory $backend
for ($i = 0; $i -lt 30; $i++) {
  try { if ((Invoke-WebRequest http://127.0.0.1:8022/api/v1/health -UseBasicParsing).StatusCode -eq 200) { break } } catch {}
  Start-Sleep -Seconds 1
}
if (-not (Test-NetConnection 127.0.0.1 -Port 8022 -InformationLevel Quiet)) { throw "模型服务未在 8022 启动" }

$env:PLANT_MODEL_API_URL = "http://127.0.0.1:8022"
Start-Process -FilePath (Join-Path $pc "plant-ui-app.exe") -WorkingDirectory $pc
Write-Host "已启动：Web http://127.0.0.1:8022，PC 客户端与本地数据库。"
'@ | Set-Content -Encoding utf8 (Join-Path $releaseRoot "Start-Plant.ps1")

@'
# Plant Suite

运行 `powershell -ExecutionPolicy Bypass -File .\Start-Plant.ps1`。
Web 端在 `http://127.0.0.1:8022`，PC 客户端会同时启动。

首次启动会在 `backend\data\surreal` 创建本地数据库。部署既有数据时，将该目录替换为对应数据目录，并按需修改 `backend\DbOption.toml` 与 `backend\web\config.json`。
'@ | Set-Content -Encoding utf8 (Join-Path $releaseRoot "README.md")

Write-Host "发布包已生成: $releaseRoot"
