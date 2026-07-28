use rust_ploop_processor::*;
use std::fs;
use std::path::Path;

fn main() -> Result<()> {
    debug_println!("🚀 Rust PLOOP Processor - 批量处理test-data目录");
    debug_println!("{}", "=".repeat(80));

    // 查找test-data目录中的所有.txt文件
    let test_data_dir = Path::new("../test-data");
    if !test_data_dir.exists() {
        return Err(PLoopError::IoError(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "test-data目录不存在"
        )));
    }

    let mut txt_files = Vec::new();
    for entry in fs::read_dir(test_data_dir).map_err(|e| PLoopError::IoError(e))? {
        let entry = entry.map_err(|e| PLoopError::IoError(e))?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("txt") {
            txt_files.push(path);
        }
    }

    txt_files.sort();
    debug_println!("找到 {} 个数据文件", txt_files.len());

    // 创建处理器
    let processor = PLoopProcessor::new();

    // 创建输出目录
    let svg_dir = Path::new("rust-svg");
    if !svg_dir.exists() {
        fs::create_dir_all(svg_dir)
            .map_err(|e| PLoopError::IoError(e))?;
    }

    // 处理器和SVG生成器
    let svg_generator = SimpleSvgGenerator::new();

    // 统计信息
    let mut total_files = 0;
    let mut total_ploops = 0;
    let mut total_vertices = 0;
    let mut total_fradius = 0;
    let mut processed_count = 0;
    let mut fradius_cases = Vec::new();
    let mut all_results = Vec::new();

    // 处理每个文件
    for (file_idx, file_path) in txt_files.iter().enumerate() {
        let filename = file_path.file_name().unwrap().to_string_lossy();
        debug_println!("{}", "=".repeat(80));
        debug_println!("处理文件 [{}/{}]: {}", file_idx + 1, txt_files.len(), filename);

        // 读取文件
        let content = match fs::read_to_string(file_path) {
            Ok(content) => content,
            Err(e) => {
                debug_println!("  ❌ 读取文件失败: {}", e);
                continue;
            }
        };

        // 解析PLOOP数据
        let ploops = match processor.parse_file(&content) {
            Ok(ploops) => ploops,
            Err(e) => {
                debug_println!("  ❌ 解析失败: {}", e);
                continue;
            }
        };

        if ploops.is_empty() {
            debug_println!("  ⚠️ 没有找到PLOOP数据");
            continue;
        }

        total_files += 1;
        total_ploops += ploops.len();

        // 处理每个PLOOP
        for (i, ploop) in ploops.iter().enumerate() {
            debug_println!("  处理 PLOOP [{}/{}]: {}", i + 1, ploops.len(), ploop.name);
            debug_println!("    原始顶点数: {}", ploop.vertices.len());

            // 检查原始FRADIUS数量
            let original_fradius_count = ploop.vertices.iter().filter(|v| v.has_fradius()).count();
            if original_fradius_count > 0 {
                debug_println!("    原始FRADIUS数量: {}", original_fradius_count);
            }

            // 处理PLOOP
            match processor.process_ploop(ploop) {
                Ok(processed_vertices) => {
                    let fradius_count = processed_vertices.iter().filter(|v| v.has_fradius()).count();

                    debug_println!("    处理后顶点数: {}", processed_vertices.len());
                    debug_println!("    处理后FRADIUS数量: {}", fradius_count);

                    // 生成文件名
                    let safe_name = ploop.name.replace("/", "_").replace("-", "_");
                    let svg_filename = format!("rust-svg/{}_profile.svg", safe_name);

                    // 生成SVG
                    match svg_generator.generate_svg_with_arcs(ploop, &processed_vertices, &svg_filename) {
                        Ok(_) => {
                            debug_println!("    ✅ SVG生成成功: {}", svg_filename);
                        }
                        Err(e) => {
                            debug_println!("    ❌ SVG生成失败: {}", e);
                        }
                    }

                    // 更新统计
                    total_vertices += processed_vertices.len();
                    total_fradius += fradius_count;
                    processed_count += 1;

                    // 记录结果
                    all_results.push((ploop.name.clone(), ploop.vertices.len(), processed_vertices.len(), original_fradius_count, fradius_count));

                    // 如果有FRADIUS，记录详情
                    if original_fradius_count > 0 || fradius_count > 0 {
                        fradius_cases.push((ploop.name.clone(), original_fradius_count, fradius_count));
                    }
                }
                Err(e) => {
                    debug_println!("    ❌ 处理失败: {}", e);
                }
            }
        }
    }

    // 最终统计
    debug_println!("{}", "=".repeat(80));
    debug_println!("🎉 批量处理完成！");
    debug_println!("- 总文件数: {}", total_files);
    debug_println!("- 总PLOOP数量: {}", total_ploops);
    debug_println!("- 成功处理数量: {}", processed_count);
    debug_println!("- 总顶点数: {}", total_vertices);
    debug_println!("- 总FRADIUS数量: {}", total_fradius);
    debug_println!("- 输出目录: rust-svg/");

    // 列出包含FRADIUS的案例
    if !fradius_cases.is_empty() {
        debug_println!("\n包含FRADIUS的案例:");
        for (name, original_count, processed_count) in &fradius_cases {
            debug_println!("  - {}: {} 个原始FRADIUS → {} 个处理后FRADIUS",
                name, original_count, processed_count);
        }
    } else {
        debug_println!("\n没有发现包含FRADIUS的案例");
    }

    // 详细结果列表
    debug_println!("\n详细处理结果:");
    for (name, original_vertices, processed_vertices, original_fradius, processed_fradius) in &all_results {
        debug_println!("  - {}: {} → {} 顶点, FRADIUS: {} → {}",
            name, original_vertices, processed_vertices, original_fradius, processed_fradius);
    }

    Ok(())
}
