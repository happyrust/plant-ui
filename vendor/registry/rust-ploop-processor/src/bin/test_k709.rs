use rust_ploop_processor::*;
use std::fs;

fn main() -> Result<()> {
    debug_println!("🔍 专门测试K709的SVG生成");
    
    // 读取K709文件
    let content = fs::read_to_string("../test-data/1KA-ARCH-P-R-K709.txt")
        .map_err(|e| PLoopError::IoError(e))?;
    
    let processor = PLoopProcessor::new();
    let ploops = processor.parse_file(&content)?;
    
    if let Some(ploop) = ploops.first() {
        debug_println!("原始PLOOP: {}", ploop.name);
        debug_println!("原始顶点数: {}", ploop.vertices.len());

        for (i, v) in ploop.vertices.iter().enumerate() {
            debug_println!("  原始[{}]: ({:.2}, {:.2}, {:.2})", i, v.x(), v.y(), v.z());
        }

        // 处理PLOOP
        let processed_vertices = processor.process_ploop(ploop)?;
        debug_println!("\n处理后顶点数: {}", processed_vertices.len());

        for (i, v) in processed_vertices.iter().enumerate() {
            debug_println!("  处理后[{}]: ({:.2}, {:.2}, {:.2})", i, v.x(), v.y(), v.z());
        }
        
        // 生成SVG
        let svg_generator = SimpleSvgGenerator::new();
        let svg_filename = "rust-svg/test_k709_debug.svg";
        
        debug_println!("\n开始生成SVG...");
        svg_generator.generate_svg_with_arcs(ploop, &processed_vertices, svg_filename)?;
        debug_println!("SVG已保存到: {}", svg_filename);
        
        // 读取生成的SVG文件，检查路径
        let svg_content = fs::read_to_string(svg_filename)
            .map_err(|e| PLoopError::IoError(e))?;
        
        // 查找path元素
        for line in svg_content.lines() {
            if line.contains("<path") {
                debug_println!("\n生成的SVG路径:");
                debug_println!("{}", line);

                // 解析路径命令
                if let Some(start) = line.find("d=\"") {
                    if let Some(end) = line[start+3..].find("\"") {
                        let path_data = &line[start+3..start+3+end];
                        debug_println!("路径数据: {}", path_data);

                        // 分析路径命令
                        let commands: Vec<&str> = path_data.split_whitespace().collect();
                        debug_println!("路径命令数: {}", commands.len());
                        for (i, cmd) in commands.iter().enumerate() {
                            debug_println!("  [{}]: {}", i, cmd);
                        }
                    }
                }
                break;
            }
        }
    }
    
    Ok(())
}
