use rust_ploop_processor::*;
use std::fs;
use std::path::Path;

fn main() -> Result<()> {
    debug_println!("🚀 Rust PLOOP Processor - K717 截面处理");
    debug_println!("{}", repeat_char('=', 80));

    // 读取文件
    let content = fs::read_to_string("../717.txt")
        .map_err(|e| PLoopError::IoError(e))?;

    debug_println!("文件读取成功，内容长度: {} 字符", content.len());

    // 创建处理器
    let processor = PLoopProcessor::new();

    // 解析PLOOP数据
    debug_println!("\n开始解析PLOOP数据...");
    let ploops = processor.parse_file(&content)?;
    debug_println!("\n解析完成，发现 {} 个PLOOP", ploops.len());

    if ploops.is_empty() {
        debug_println!("警告: 没有找到任何有效的PLOOP数据");
        return Ok(());
    }
    
    // 查找K717
    let k717_ploop = ploops.iter()
        .find(|ploop| ploop.name.contains("K717"))
        .ok_or_else(|| PLoopError::ParseError("没有找到K717 PLOOP".to_string()))?;
    
    debug_println!("\n找到K717: {}", k717_ploop);
    debug_println!("{}", repeat_char('=', 80));

    // 处理K717
    let processed_vertices = processor.process_ploop(k717_ploop)?;

    // 检查FRADIUS信息是否正确保留
    let fradius_count = processed_vertices.iter().filter(|v| v.has_fradius()).count();
    debug_println!("处理完成: {} 个顶点，其中 {} 个圆弧顶点", processed_vertices.len(), fradius_count);

    // 生成详细报告
    let report = processor.generate_profile_report(k717_ploop, &processed_vertices);
    debug_println!("\n{}", report);

    // 生成SVG
    debug_println!("生成SVG可视化...");
    
    // 确保rust-svg目录存在
    let svg_dir = Path::new("rust-svg");
    if !svg_dir.exists() {
        fs::create_dir_all(svg_dir)
            .map_err(|e| PLoopError::IoError(e))?;
    }
    
    let svg_filename = "rust-svg/k717_profile_with_arcs.svg";
    let svg_generator = SimpleSvgGenerator::new();
    svg_generator.generate_svg_with_arcs(k717_ploop, &processed_vertices, svg_filename)?;
    
    debug_println!("\nK717分析完成！");
    debug_println!("- 原始顶点数: {}", k717_ploop.vertices.len());
    debug_println!("- 处理后顶点数: {}", processed_vertices.len());
    debug_println!("- FRADIUS数量: {}", fradius_count);
    debug_println!("- SVG文件: {}", svg_filename);

    // 显示FRADIUS详情
    let fradius_vertices: Vec<_> = k717_ploop.vertices.iter()
        .filter(|v| v.has_fradius())
        .collect();

    if !fradius_vertices.is_empty() {
        debug_println!("\nFRADIUS详情:");
        for (i, vertex) in fradius_vertices.iter().enumerate() {
            debug_println!("  {}. 位置: ({:.2}, {:.2}) - 半径: {:.2}mm",
                i + 1, vertex.x(), vertex.y(), vertex.get_fradius());
        }
    }
    
    // 生成JSON数据供3D查看器使用
    let json_filename = "rust-svg/k717_data.json";
    JsonExporter::export_ploop(&k717_ploop, &processed_vertices, json_filename)?;
    
    Ok(())
}

/// 重复字符串的辅助函数
fn repeat_char(ch: char, count: usize) -> String {
    std::iter::repeat(ch).take(count).collect()
}
