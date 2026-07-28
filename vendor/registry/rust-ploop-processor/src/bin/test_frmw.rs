use rust_ploop_processor::*;
use anyhow::Result;

fn main() -> Result<()> {
    // 启用调试输出
    std::env::set_var("DEBUG_PRINT", "1");

    // Framework_8 Panel_1 数据
    let vertices = vec![
        Vertex::new(0.0, 0.05, 0.0),
        Vertex::with_fradius(-1604.77, -0.65, 0.0, 2778.67),
        Vertex::with_fradius(-3210.0, 2778.67, 0.0, 2778.67),
        Vertex::with_fradius(-1604.77, 5558.05, 0.0, 2778.67),
        Vertex::new(0.0, 5557.34, 0.0),
        Vertex::new(0.0, 0.05, 0.0), // 闭合
    ];

    println!("=== Framework_8 Panel_1 FRADIUS测试 ===");
    println!("\n原始顶点（{}个）：", vertices.len());
    for (i, v) in vertices.iter().enumerate() {
        if v.has_fradius() {
            println!("  [{}] ({:.2}, {:.2}) FRADIUS: {:.2}mm",
                i, v.x(), v.y(), v.get_fradius());
        } else {
            println!("  [{}] ({:.2}, {:.2})",
                i, v.x(), v.y());
        }
    }

    // 创建PLOOP
    let mut ploop = PLoop::new("Framework_8_Panel_1".to_string(), 2550.0);
    ploop.vertices = vertices.clone();

    // 处理PLOOP
    let processor = PLoopProcessor::new();
    let processed = processor.process_ploop(&ploop)?;

    println!("\n处理后顶点（{}个）：", processed.len());
    for (i, v) in processed.iter().enumerate() {
        if v.has_fradius() {
            println!("  [{}] ({:.2}, {:.2}) FRADIUS: {:.2}mm ⚠️",
                i, v.x(), v.y(), v.get_fradius());
        } else {
            println!("  [{}] ({:.2}, {:.2})",
                i, v.x(), v.y());
        }
    }

    // 分析FRADIUS变化
    println!("\n=== FRADIUS分析 ===");

    let original_fradius: Vec<_> = vertices.iter()
        .filter(|v| v.has_fradius())
        .map(|v| v.get_fradius())
        .collect();

    let processed_fradius: Vec<_> = processed.iter()
        .filter(|v| v.has_fradius())
        .map(|v| v.get_fradius())
        .collect();

    println!("原始FRADIUS数量: {}", original_fradius.len());
    println!("处理后FRADIUS数量: {}", processed_fradius.len());

    if !original_fradius.is_empty() {
        println!("\n原始FRADIUS值:");
        for (i, r) in original_fradius.iter().enumerate() {
            println!("  {}: {:.2}mm", i + 1, r);
        }
    }

    if !processed_fradius.is_empty() {
        println!("\n处理后FRADIUS值:");
        for (i, r) in processed_fradius.iter().enumerate() {
            println!("  {}: {:.2}mm", i + 1, r);
        }
    }

    // 计算实际限制
    println!("\n=== 几何约束分析 ===");

    for i in 0..vertices.len() - 1 {
        let v = &vertices[i];
        if v.has_fradius() {
            let prev = if i > 0 { &vertices[i - 1] } else { &vertices[vertices.len() - 2] };
            let next = &vertices[i + 1];

            let edge1_len = ((v.x() - prev.x()).powi(2) + (v.y() - prev.y()).powi(2)).sqrt();
            let edge2_len = ((next.x() - v.x()).powi(2) + (next.y() - v.y()).powi(2)).sqrt();

            println!("\nFRADIUS顶点{} (原始半径: {:.2}mm):", i, v.get_fradius());
            println!("  前边长: {:.2}mm", edge1_len);
            println!("  后边长: {:.2}mm", edge2_len);
            println!("  最大可能切点距离: {:.2}mm", edge1_len.min(edge2_len) * 0.9);

            // 估算实际可能的半径
            let v1 = glam::DVec2::new(v.x() - prev.x(), v.y() - prev.y()).normalize();
            let v2 = glam::DVec2::new(next.x() - v.x(), next.y() - v.y()).normalize();
            let cos_angle = v1.dot(v2).abs();
            let half_angle = cos_angle.acos() / 2.0;

            if half_angle > 0.001 {
                let max_tangent_dist = edge1_len.min(edge2_len) * 0.9;
                let max_radius = max_tangent_dist * half_angle.tan();
                println!("  最大可能半径: {:.2}mm", max_radius);

                if v.get_fradius() > max_radius {
                    println!("  ⚠️ 原始半径超出几何限制！");
                }
            }
        }
    }

    // 生成SVG
    println!("\n生成SVG...");
    let svg_generator = SimpleSvgGenerator::new();
    let svg_path = "rust-svg/Framework_8_Panel_1_profile.svg";
    svg_generator.generate_svg_with_arcs(&ploop, &processed, svg_path)?;
    println!("SVG已保存到: {}", svg_path);

    println!("\n=== 测试完成 ===");
    Ok(())
}