# 🦀 Rust PLOOP Processor

基于Python算法的Rust实现，用于处理PLOOP截面数据并生成SVG可视化。

## 🎯 功能特性

- **PLOOP数据解析**: 完整解析AVEVA PDMS格式的PLOOP数据
- **FRADIUS圆弧处理**: 精确计算和处理大半径圆角
- **顶点优化**: 智能去重和共线顶点处理
- **SVG可视化**: 生成带真实圆弧的高质量SVG图形
- **高性能**: Rust实现，处理速度快，内存安全

## 🚀 快速开始

### 前置要求

- Rust 1.70+ 
- Cargo

### 构建和运行

#### Windows
```bash
# 运行构建脚本
build_and_run.bat
```

#### Linux/macOS
```bash
# 给脚本执行权限
chmod +x build_and_run.sh

# 运行构建脚本
./build_and_run.sh
```

#### 手动构建
```bash
# 构建项目
cargo build --release

# 运行K717处理器
cargo run --bin k717_processor --release
```

## 📁 项目结构

```
rust-ploop-processor/
├── src/
│   ├── lib.rs              # 库入口
│   ├── vertex.rs           # 顶点数据结构
│   ├── ploop.rs            # PLOOP数据结构
│   ├── parser.rs           # 文件解析器
│   ├── processor.rs        # 核心处理逻辑
│   ├── fradius.rs          # FRADIUS圆弧处理
│   ├── svg_generator.rs    # SVG生成器
│   ├── error.rs            # 错误处理
│   └── bin/
│       └── k717_processor.rs  # K717处理主程序
├── rust-svg/               # 输出目录
│   ├── k717_profile_with_arcs.svg  # SVG可视化
│   └── k717_data.json      # JSON数据
├── Cargo.toml              # 项目配置
└── README.md               # 说明文档
```

## 🔧 核心算法

### 1. FRADIUS处理
- 计算圆角的切点和圆心
- 生成SVG Arc命令参数
- 处理大半径圆弧的数值稳定性

### 2. 顶点优化
- **去重处理**: 1.0mm容差去除重复顶点
- **共线检测**: AVEVA风格的共线顶点裁剪
- **几何优化**: 保持几何精度的顶点简化

### 3. SVG生成
- 真实圆弧绘制（非多边线近似）
- 自适应缩放和坐标变换
- 完整的可视化元素（网格、标注、坐标表）

## 📊 输出文件

### SVG可视化 (`k717_profile_with_arcs.svg`)
- **蓝色轮廓**: 构件边界，使用真实SVG Arc命令
- **红色圆点**: 处理后的顶点，带编号标识
- **橙色圆圈**: FRADIUS位置标记，显示半径值
- **网格背景**: 便于尺寸参考
- **坐标表格**: 右下角显示所有顶点坐标

### JSON数据 (`k717_data.json`)
- 完整的PLOOP数据
- 原始和处理后的顶点坐标
- 边界框和统计信息

## 🎨 与Python版本的对比

| 特性 | Python版本 | Rust版本 |
|------|------------|----------|
| 处理速度 | 中等 | 快速 |
| 内存安全 | 运行时检查 | 编译时保证 |
| 类型安全 | 动态类型 | 静态强类型 |
| 错误处理 | 异常机制 | Result类型 |
| 并发性能 | GIL限制 | 原生并发 |
| 部署 | 需要Python环境 | 单一可执行文件 |

## 🔍 算法细节

### FRADIUS圆弧计算
```rust
// 计算圆角的几何参数
let arc_info = fradius_processor.calculate_fillet_arc_info(
    prev_vertex, current_vertex, next_vertex
)?;

// 生成SVG Arc命令
let svg_arc = format!(
    "A {:.2} {:.2} 0 {} {} {:.2} {:.2}",
    radius, radius, large_arc_flag, sweep_flag, end_x, end_y
);
```

### 顶点去重和优化
```rust
// 1.0mm容差去重
let is_duplicate = result.iter().any(|v| 
    v.is_near_2d(vertex, DEDUP_TOLERANCE)
);

// 共线检测
let cross_product = v1.cross(&v2).abs();
let is_collinear = cross_product < COLLINEAR_TOLERANCE;
```

## 🐛 故障排除

### 常见问题

1. **找不到717.txt文件**
   - 确保717.txt文件在项目根目录的上两级目录中
   - 检查文件路径: `../../717.txt`

2. **构建失败**
   - 确保Rust版本 >= 1.70
   - 运行 `cargo update` 更新依赖

3. **SVG显示异常**
   - 检查浏览器是否支持SVG
   - 验证生成的SVG文件完整性

## 📈 性能特点

- **内存效率**: 零拷贝字符串处理
- **计算精度**: f64双精度浮点运算
- **错误安全**: 完整的错误处理链
- **类型安全**: 编译时类型检查

## 🤝 贡献

欢迎提交Issue和Pull Request来改进这个项目！

## 📄 许可证

MIT License
