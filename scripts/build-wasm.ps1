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

cargo build -p rs-plant --target wasm32-unknown-unknown --release
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$targetDir = (cargo metadata --no-deps --format-version 1 | ConvertFrom-Json).target_directory
$wasm = Join-Path $targetDir "wasm32-unknown-unknown/release/rs-plant.wasm"

if (-not (Get-Command wasm-bindgen -ErrorAction SilentlyContinue)) {
  cargo install wasm-bindgen-cli --version 0.2.126 --locked
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
wasm-bindgen `
  --target web `
  --out-dir web/public/pkg `
  --out-name rs-plant `
  $wasm
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

# `#[wasm_bindgen(inline_js = ...)]` is emitted as a separate ES module under
# `pkg/snippets`. Keep the published package self-contained by moving those
# snippets into the generated entry module, then remove their staging folder.
$pkgDir = Join-Path $PSScriptRoot "..\web\public\pkg"
$entryPath = Join-Path $pkgDir "rs-plant.js"
$snippetDir = Join-Path $pkgDir "snippets"
$entry = Get-Content -LiteralPath $entryPath -Raw
$snippetImportPattern = "(?m)^import \{ (?<exports>[^}]+) \} from '\./snippets/(?<path>[^']+)';\r?\n"
$imports = [regex]::Matches($entry, $snippetImportPattern)

if ($imports.Count -gt 0) {
  $inlinedSnippets = foreach ($import in $imports) {
    $snippetPath = Join-Path $snippetDir $import.Groups['path'].Value
    if (-not (Test-Path -LiteralPath $snippetPath -PathType Leaf)) {
      throw "wasm-bindgen snippet missing: $snippetPath"
    }
    (Get-Content -LiteralPath $snippetPath -Raw) -replace '(?m)^export\s+', ''
  }

  $entry = [regex]::Replace($entry, $snippetImportPattern, '')
  $firstNewline = $entry.IndexOf("`n")
  $entry = $entry.Insert($firstNewline + 1, "`n$($inlinedSnippets -join "`n")`n")
  Set-Content -LiteralPath $entryPath -Value $entry -NoNewline
}

if (Test-Path -LiteralPath $snippetDir) {
  Remove-Item -LiteralPath $snippetDir -Recurse -Force
}
