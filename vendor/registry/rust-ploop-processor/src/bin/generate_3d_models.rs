use rust_ploop_processor::*;
use std::fs;

fn main() -> Result<()> {
    debug_println!("🚀 PLOOP 3D模型批量生成器");
    debug_println!("===============================================================================");

    // 读取文件
    let content = fs::read_to_string("../717.txt")
        .map_err(|e| PLoopError::IoError(e))?;

    // 解析PLOOP数据
    let parser = PLoopParser::new(1.0);
    let ploops = parser.parse_file(&content)?;

    debug_println!("解析完成，发现 {} 个PLOOP", ploops.len());

    // 创建处理器
    let processor = PLoopProcessor::new();

    // 确保输出目录存在
    fs::create_dir_all("rust-svg").map_err(|e| PLoopError::IoError(e))?;

    // 重点案例列表
    let target_cases = ["K717", "K718", "K716", "K701"];

    for case_name in &target_cases {
        // 查找指定案例
        let target_ploop = ploops.iter()
            .find(|ploop| ploop.name.contains(case_name))
            .cloned();

        if let Some(ploop) = target_ploop {
            debug_println!("\n🔧 处理案例: {}", ploop.name);
            debug_println!("- 原始顶点数: {}", ploop.vertices.len());
            debug_println!("- 拉伸高度: {:.1}mm", ploop.height);

            // 处理PLOOP
            let processed_vertices = processor.process_ploop(&ploop)?;
            let fradius_count = processed_vertices.iter()
                .filter(|v| v.has_fradius())
                .count();

            debug_println!("- 处理后顶点数: {}", processed_vertices.len());
            debug_println!("- FRADIUS数量: {}", fradius_count);

            // 生成JSON数据
            let case_id = case_name.to_lowercase();
            let json_filename = format!("rust-svg/{}_data.json", case_id);
            JsonExporter::export_ploop(&ploop, &processed_vertices, &json_filename)?;

            // 生成SVG
            let svg_filename = format!("rust-svg/{}_profile_with_arcs.svg", case_id);
            let svg_generator = SimpleSvgGenerator::new();
            svg_generator.generate_svg_with_arcs(&ploop, &processed_vertices, &svg_filename)?;
            debug_println!("- SVG文件: {}", svg_filename);

            debug_println!("✅ {} 处理完成", case_name);
        } else {
            debug_println!("⚠️  未找到案例: {}", case_name);
        }
    }

    debug_println!("\n🎉 所有案例处理完成！");
    debug_println!("现在可以运行3D模型生成器:");
    debug_println!("cd ploop-3d-viewer && cargo run --release");

    Ok(())
}
