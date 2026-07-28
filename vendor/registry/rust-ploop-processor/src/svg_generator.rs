use crate::{vertex::*, ploop::*, fradius::*, error::*};
use std::fs;

/// SVG生成器
pub struct SvgGenerator {
    width: u32,
    height: u32,
    margin: u32,
    fradius_processor: FradiusProcessor,
}

impl SvgGenerator {
    pub fn new() -> Self {
        Self {
            width: 1000,
            height: 800,
            margin: 60,
            fradius_processor: FradiusProcessor::new(crate::DEFAULT_TOLERANCE),
        }
    }
    
    pub fn with_size(width: u32, height: u32, margin: u32) -> Self {
        Self {
            width,
            height,
            margin,
            fradius_processor: FradiusProcessor::new(crate::DEFAULT_TOLERANCE),
        }
    }
    
    /// 生成带真实圆弧的SVG
    pub fn generate_svg_with_arcs(
        &self,
        ploop: &PLoop,
        processed_vertices: &[Vertex],
        filename: &str,
    ) -> Result<String> {
        if processed_vertices.is_empty() {
            return Err(PLoopError::SvgError("没有顶点数据".to_string()));
        }
        
        // 计算边界
        let (min_x, max_x, min_y, max_y) = self.calculate_bounds(&ploop.vertices);
        
        // 计算缩放比例
        let data_width = max_x - min_x;
        let data_height = max_y - min_y;
        
        let data_width = if data_width == 0.0 { 1.0 } else { data_width };
        let data_height = if data_height == 0.0 { 1.0 } else { data_height };
        
        let available_width = (self.width - 2 * self.margin) as f64;
        let available_height = (self.height - 2 * self.margin) as f64;
        
        let scale = (available_width / data_width).min(available_height / data_height);
        
        // 坐标变换函数
        let transform_x = |x: f64| -> f64 {
            self.margin as f64 + (x - min_x) * scale
        };
        
        let transform_y = |y: f64| -> f64 {
            self.height as f64 - self.margin as f64 - (y - min_y) * scale
        };
        
        // 开始生成SVG
        let mut svg_lines = Vec::new();
        
        // SVG头部
        svg_lines.push(format!(
            r#"<svg width="{}" height="{}" viewBox="0 0 {} {}" xmlns="http://www.w3.org/2000/svg">"#,
            self.width, self.height, self.width, self.height
        ));
        
        // 定义箭头标记
        svg_lines.push("<defs>".to_string());
        svg_lines.push("  <marker id=\"arrowhead\" markerWidth=\"6\" markerHeight=\"4\" refX=\"5\" refY=\"2\" orient=\"auto\">".to_string());
        svg_lines.push("    <polygon points=\"0 0, 6 2, 0 4\" fill=\"#FF6600\" stroke=\"#FF6600\" stroke-width=\"0.5\"/>".to_string());
        svg_lines.push("  </marker>".to_string());
        svg_lines.push("  <marker id=\"arrowhead-arc\" markerWidth=\"6\" markerHeight=\"4\" refX=\"5\" refY=\"2\" orient=\"auto\">".to_string());
        svg_lines.push("    <polygon points=\"0 0, 6 2, 0 4\" fill=\"#FF0066\" stroke=\"#FF0066\" stroke-width=\"0.5\"/>".to_string());
        svg_lines.push("  </marker>".to_string());
        svg_lines.push("</defs>".to_string());
        
        // 背景
        svg_lines.push(format!(
            "<rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" opacity=\"1\" fill=\"#FFFFFF\" stroke=\"none\"/>",
            self.width, self.height
        ));
        
        // 标题
        svg_lines.push(format!(
            "<text x=\"{}\" y=\"25\" dy=\"0.76em\" text-anchor=\"middle\" font-family=\"Arial\" font-size=\"16\" opacity=\"1\" fill=\"#000000\">",
            self.width / 2
        ));
        svg_lines.push(format!(
            "PLOOP Profile: {} (高度: {:.1}mm, 带路径箭头标识)",
            ploop.name, ploop.height
        ));
        svg_lines.push("</text>".to_string());
        
        // 生成路径
        let path_data = self.generate_path_with_arcs(processed_vertices, &transform_x, &transform_y)?;
        svg_lines.push(format!(
            "<path d=\"{}\" fill=\"none\" opacity=\"1\" stroke=\"#0000FF\" stroke-width=\"2\"/>",
            path_data
        ));
        
        // 添加顶点标记
        self.add_vertex_markers(&mut svg_lines, processed_vertices, &transform_x, &transform_y);
        
        // 添加FRADIUS标记
        self.add_fradius_markers(&mut svg_lines, &ploop.vertices, &transform_x, &transform_y);
        
        // 添加网格
        self.add_grid(&mut svg_lines);
        
        // 添加坐标轴和刻度
        self.add_axes_and_ticks(&mut svg_lines, min_x, max_x, min_y, max_y);
        
        // 添加坐标表格
        self.add_coordinate_table(&mut svg_lines, processed_vertices);
        
        svg_lines.push("</svg>".to_string());
        
        let svg_content = svg_lines.join("\n");
        
        // 保存到文件
        fs::write(filename, &svg_content)
            .map_err(|e| PLoopError::IoError(e))?;
        
        println!("SVG已保存到: {}", filename);
        
        Ok(svg_content)
    }
    
    /// 计算边界
    fn calculate_bounds(&self, vertices: &[Vertex]) -> (f64, f64, f64, f64) {
        if vertices.is_empty() {
            return (0.0, 1.0, 0.0, 1.0);
        }
        
        let mut min_x = vertices[0].x;
        let mut max_x = vertices[0].x;
        let mut min_y = vertices[0].y;
        let mut max_y = vertices[0].y;
        
        for vertex in vertices {
            min_x = min_x.min(vertex.x);
            max_x = max_x.max(vertex.x);
            min_y = min_y.min(vertex.y);
            max_y = max_y.max(vertex.y);
        }
        
        (min_x, max_x, min_y, max_y)
    }
    
    /// 生成带圆弧的路径
    fn generate_path_with_arcs(
        &self,
        vertices: &[Vertex],
        transform_x: &dyn Fn(f64) -> f64,
        transform_y: &dyn Fn(f64) -> f64,
    ) -> Result<String> {
        if vertices.is_empty() {
            return Ok(String::new());
        }
        
        let mut path_commands = Vec::new();
        let mut vertices_with_closure = vertices.to_vec();
        
        // 添加闭合顶点
        if !vertices.is_empty() {
            let first = vertices[0].clone();
            vertices_with_closure.push(first);
            println!("  添加闭合顶点: ({:.2}, {:.2})", vertices[0].x, vertices[0].y);
        }
        
        // 起始点
        let start_x = transform_x(vertices_with_closure[0].x);
        let start_y = transform_y(vertices_with_closure[0].y);
        path_commands.push(format!("M {:.2} {:.2}", start_x, start_y));
        
        let mut i = 1;
        while i < vertices_with_closure.len() {
            let current = &vertices_with_closure[i];
            let prev_vertex = &vertices_with_closure[i - 1];
            
            // 检查前一个顶点是否有FRADIUS
            if prev_vertex.has_fradius() && i >= 2 {
                // 计算圆弧信息
                let arc_prev = if i >= 2 { &vertices_with_closure[i - 2] } else { &vertices_with_closure[vertices_with_closure.len() - 2] };
                let arc_center = prev_vertex;
                let arc_next = current;
                
                println!("  绘制圆弧: 从({:.2}, {:.2}) 经过({:.2}, {:.2}) 到({:.2}, {:.2}) FRADIUS:{:.2}",
                    arc_prev.x, arc_prev.y, arc_center.x, arc_center.y, arc_next.x, arc_next.y, arc_center.get_fradius());
                
                if let Some(arc_info) = self.fradius_processor.calculate_fillet_arc_info(arc_prev, arc_center, arc_next)? {
                    // 画圆弧到下一个顶点
                    let end_x = transform_x(current.x);
                    let end_y = transform_y(current.y);
                    let radius_scaled = arc_info.radius * (transform_x(1.0) - transform_x(0.0));
                    
                    let large_arc_flag = if arc_info.large_arc { 1 } else { 0 };
                    let sweep_flag = if arc_info.sweep_flag { 1 } else { 0 };
                    
                    path_commands.push(format!(
                        "A {:.2} {:.2} 0 {} {} {:.2} {:.2}",
                        radius_scaled, radius_scaled, large_arc_flag, sweep_flag, end_x, end_y
                    ));
                    
                    println!("    SVG圆弧命令: A {:.2} {:.2} 0 {} {} {:.2} {:.2}",
                        radius_scaled, radius_scaled, large_arc_flag, sweep_flag, end_x, end_y);
                } else {
                    // 圆弧计算失败，画直线
                    let x = transform_x(current.x);
                    let y = transform_y(current.y);
                    path_commands.push(format!("L {:.2} {:.2}", x, y));
                }
            } else {
                // 普通顶点，画直线
                let x = transform_x(current.x);
                let y = transform_y(current.y);
                path_commands.push(format!("L {:.2} {:.2}", x, y));
            }
            
            i += 1;
        }
        
        // 闭合路径
        path_commands.push("Z".to_string());
        
        Ok(path_commands.join(" "))
    }
    
    /// 添加顶点标记
    fn add_vertex_markers(
        &self,
        svg_lines: &mut Vec<String>,
        vertices: &[Vertex],
        transform_x: &dyn Fn(f64) -> f64,
        transform_y: &dyn Fn(f64) -> f64,
    ) {
        for (i, vertex) in vertices.iter().enumerate() {
            let x = transform_x(vertex.x);
            let y = transform_y(vertex.y);
            
            // 顶点圆圈
            svg_lines.push(format!(
                r#"<circle cx="{:.0}" cy="{:.0}" r="3" opacity="1" fill="#FF0000" stroke="white" stroke-width="1"/>"#,
                x, y
            ));
            
            // 顶点编号
            svg_lines.push(format!(
                r#"<text x="{:.0}" y="{:.0}" font-family="Arial" font-size="10" font-weight="bold" fill="#000000" stroke="white" stroke-width="0.5">{}</text>"#,
                x + 8.0, y - 8.0, i
            ));
        }
    }
    
    /// 添加FRADIUS标记
    fn add_fradius_markers(
        &self,
        svg_lines: &mut Vec<String>,
        vertices: &[Vertex],
        transform_x: &dyn Fn(f64) -> f64,
        transform_y: &dyn Fn(f64) -> f64,
    ) {
        for vertex in vertices {
            if vertex.has_fradius() {
                let x = transform_x(vertex.x);
                let y = transform_y(vertex.y);
                
                svg_lines.push(format!(
                    r#"<circle cx="{:.0}" cy="{:.0}" r="4" opacity="1" fill="none" stroke="#FFA500" stroke-width="2"/>"#,
                    x, y
                ));
                
                svg_lines.push(format!(
                    r#"<text x="{:.0}" y="{:.0}" font-family="Arial" font-size="8" fill="#FFA500">R{:.0}</text>"#,
                    x + 10.0, y - 10.0, vertex.get_fradius()
                ));
            }
        }
    }
    
    /// 添加网格
    fn add_grid(&self, svg_lines: &mut Vec<String>) {
        let grid_step = 50;
        
        // 垂直网格线
        let mut i = self.margin;
        while i <= self.width - self.margin {
            svg_lines.push(format!(
                r#"<line opacity="0.1" stroke="#000000" stroke-width="1" x1="{}" y1="{}" x2="{}" y2="{}"/>"#,
                i, self.height - self.margin, i, self.margin
            ));
            i += grid_step;
        }
        
        // 水平网格线
        let mut i = self.margin;
        while i <= self.height - self.margin {
            svg_lines.push(format!(
                r#"<line opacity="0.1" stroke="#000000" stroke-width="1" x1="{}" y1="{}" x2="{}" y2="{}"/>"#,
                self.margin, i, self.width - self.margin, i
            ));
            i += grid_step;
        }
    }
    
    /// 添加坐标轴和刻度
    fn add_axes_and_ticks(
        &self,
        svg_lines: &mut Vec<String>,
        min_x: f64,
        max_x: f64,
        min_y: f64,
        max_y: f64,
    ) {
        // 坐标轴标签
        svg_lines.extend(vec![
            format!(
                r#"<text x="20" y="{}" dy="0.76em" text-anchor="middle" font-family="sans-serif" font-size="10" opacity="1" fill="#000000" transform="rotate(270, 20, {})">"#,
                self.height / 2, self.height / 2
            ),
            "Y (mm)".to_string(),
            "</text>".to_string(),
            format!(
                r#"<text x="{}" y="{}" dy="-0.5ex" text-anchor="middle" font-family="sans-serif" font-size="10" opacity="1" fill="#000000">"#,
                self.width / 2, self.height - 20
            ),
            "X (mm)".to_string(),
            "</text>".to_string(),
        ]);
        
        // 坐标轴
        svg_lines.extend(vec![
            format!(
                r#"<polyline fill="none" opacity="1" stroke="#000000" stroke-width="1" points="{},{} {},{}"/>"#,
                self.margin - 1, self.margin, self.margin - 1, self.height - self.margin
            ),
            format!(
                r#"<polyline fill="none" opacity="1" stroke="#000000" stroke-width="1" points="{},{} {},{}"/>"#,
                self.margin, self.height - self.margin + 1, self.width - self.margin, self.height - self.margin + 1
            ),
        ]);
        
        // 添加刻度标签（简化版本）
        let num_ticks = 5;
        for i in 0..=num_ticks {
            // Y轴刻度
            let y_pos = self.margin + i * (self.height - 2 * self.margin) / num_ticks;
            let y_value = max_y - (i as f64) * (max_y - min_y) / (num_ticks as f64);
            svg_lines.extend(vec![
                format!(
                    r#"<text x="{}" y="{}" dy="0.5ex" text-anchor="end" font-family="sans-serif" font-size="10" opacity="1" fill="#000000">"#,
                    self.margin - 10, y_pos
                ),
                format!("{:.1}", y_value),
                "</text>".to_string(),
            ]);
            
            // X轴刻度
            let x_pos = self.margin + i * (self.width - 2 * self.margin) / num_ticks;
            let x_value = min_x + (i as f64) * (max_x - min_x) / (num_ticks as f64);
            svg_lines.extend(vec![
                format!(
                    r#"<text x="{}" y="{}" dy="0.76em" text-anchor="middle" font-family="sans-serif" font-size="10" opacity="1" fill="#000000">"#,
                    x_pos, self.height - self.margin + 10
                ),
                format!("{:.1}", x_value),
                "</text>".to_string(),
            ]);
        }
    }
    
    /// 添加坐标表格
    fn add_coordinate_table(&self, svg_lines: &mut Vec<String>, vertices: &[Vertex]) {
        let table_width = 280;
        let table_height = (vertices.len().min(20) * 12 + 40).min(280);
        let table_x = self.width - table_width - 10;
        let table_y = self.height - table_height - 10;
        
        // 表格背景
        svg_lines.push(format!(
            r#"<rect x="{}" y="{}" width="{}" height="{}" fill="white" stroke="#333" stroke-width="1" opacity="0.95" rx="5"/>"#,
            table_x, table_y, table_width, table_height
        ));
        
        // 表格标题
        svg_lines.push(format!(
            r#"<text x="{}" y="{}" text-anchor="middle" font-family="Arial" font-size="12" font-weight="bold" fill="#333">顶点坐标表</text>"#,
            table_x + table_width / 2, table_y + 15
        ));
        
        // 表头
        let header_y = table_y + 30;
        svg_lines.extend(vec![
            format!(r#"<text x="{}" y="{}" font-family="Arial" font-size="10" font-weight="bold" fill="#666">序号</text>"#, table_x + 10, header_y),
            format!(r#"<text x="{}" y="{}" font-family="Arial" font-size="10" font-weight="bold" fill="#666">X (mm)</text>"#, table_x + 50, header_y),
            format!(r#"<text x="{}" y="{}" font-family="Arial" font-size="10" font-weight="bold" fill="#666">Y (mm)</text>"#, table_x + 150, header_y),
        ]);
        
        // 分隔线
        svg_lines.push(format!(
            r#"<line x1="{}" y1="{}" x2="{}" y2="{}" stroke="#ccc" stroke-width="1"/>"#,
            table_x + 5, header_y + 3, table_x + table_width - 5, header_y + 3
        ));
        
        // 坐标数据
        let max_display = vertices.len().min(20);
        for (i, vertex) in vertices.iter().take(max_display).enumerate() {
            let row_y = header_y + 15 + i * 12;
            
            svg_lines.extend(vec![
                format!(r#"<text x="{}" y="{}" text-anchor="middle" font-family="Arial" font-size="9" fill="#333">{}</text>"#, table_x + 15, row_y, i),
                format!(r#"<text x="{}" y="{}" font-family="Arial" font-size="9" fill="#333">{:.2}</text>"#, table_x + 55, row_y, vertex.x),
                format!(r#"<text x="{}" y="{}" font-family="Arial" font-size="9" fill="#333">{:.2}</text>"#, table_x + 155, row_y, vertex.y),
            ]);
        }
    }
}

impl Default for SvgGenerator {
    fn default() -> Self {
        Self::new()
    }
}
