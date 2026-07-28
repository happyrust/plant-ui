$ErrorActionPreference = "Stop"

$clang = @(
  (Get-Command clang -ErrorAction SilentlyContinue).Source
  Get-PSDrive -PSProvider FileSystem | ForEach-Object {
    (Get-Item "$($_.Root)Program Files\LLVM\bin\clang.exe" -ErrorAction SilentlyContinue).FullName
    Get-Item "$($_.Root)Program Files\Microsoft Visual Studio\*\*\VC\Tools\Llvm\x64\bin\clang.exe" `
      -ErrorAction SilentlyContinue | Select-Object -ExpandProperty FullName
  }
) | Where-Object { $_ -and ((& $_ --print-targets 2>$null) -match "wasm32") } | Select-Object -First 1
if (-not $clang) {
  $zig = @(
    (Get-Command zig -ErrorAction SilentlyContinue).Source
    Get-Item "$env:LOCALAPPDATA\Microsoft\WinGet\Packages\zig.zig_*\*\zig.exe" `
      -ErrorAction SilentlyContinue | Select-Object -ExpandProperty FullName
  ) | Where-Object { $_ } | Select-Object -First 1
  if (-not $zig) {
    throw "ring 的 wasm 构建需要带 wasm32 target 的 LLVM clang 或 Zig"
  }
  $env:PLANT_ZIG_EXE = $zig
  $env:CC_wasm32_unknown_unknown = Join-Path $PSScriptRoot "zig-cc.cmd"
  $env:AR_wasm32_unknown_unknown = Join-Path $PSScriptRoot "zig-ar.cmd"
  $env:CFLAGS_wasm32_unknown_unknown = "--target=wasm32-freestanding-none"
} else {
  $env:CC_wasm32_unknown_unknown = $clang
  $env:AR_wasm32_unknown_unknown = Join-Path (Split-Path $clang) "llvm-ar.exe"
}

cargo build -p plant-ui-app --target wasm32-unknown-unknown --release
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$targetDir = (cargo metadata --no-deps --format-version 1 | ConvertFrom-Json).target_directory
$wasm = Join-Path $targetDir "wasm32-unknown-unknown/release/plant-ui-app.wasm"

if (-not (Get-Command wasm-bindgen -ErrorAction SilentlyContinue)) {
  cargo install wasm-bindgen-cli --version 0.2.126 --locked
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
wasm-bindgen `
  --target web `
  --out-dir web/public/pkg `
  --out-name plant_ui_app `
  $wasm
exit $LASTEXITCODE
