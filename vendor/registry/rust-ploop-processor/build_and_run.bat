@echo off
echo 🦀 构建并运行 Rust PLOOP Processor
echo =====================================

echo.
echo 📦 构建项目...
cargo build --release
if %ERRORLEVEL% neq 0 (
    echo ❌ 构建失败
    pause
    exit /b 1
)

echo.
echo ✅ 构建成功！

echo.
echo 🚀 运行 K717 处理器...
cargo run --bin k717_processor --release
if %ERRORLEVEL% neq 0 (
    echo ❌ 运行失败
    pause
    exit /b 1
)

echo.
echo ✅ 处理完成！
echo.
echo 📁 生成的文件:
if exist rust-svg\k717_profile_with_arcs.svg (
    echo   - rust-svg\k717_profile_with_arcs.svg
)
if exist rust-svg\k717_data.json (
    echo   - rust-svg\k717_data.json
)

echo.
echo 🎉 所有任务完成！
pause
