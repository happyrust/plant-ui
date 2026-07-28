#!/bin/bash

echo "🦀 构建并运行 Rust PLOOP Processor"
echo "====================================="

echo ""
echo "📦 构建项目..."
cargo build --release
if [ $? -ne 0 ]; then
    echo "❌ 构建失败"
    exit 1
fi

echo ""
echo "✅ 构建成功！"

echo ""
echo "🚀 运行 K717 处理器..."
cargo run --bin k717_processor --release
if [ $? -ne 0 ]; then
    echo "❌ 运行失败"
    exit 1
fi

echo ""
echo "✅ 处理完成！"
echo ""
echo "📁 生成的文件:"
if [ -f "rust-svg/k717_profile_with_arcs.svg" ]; then
    echo "  - rust-svg/k717_profile_with_arcs.svg"
fi
if [ -f "rust-svg/k717_data.json" ]; then
    echo "  - rust-svg/k717_data.json"
fi

echo ""
echo "🎉 所有任务完成！"
