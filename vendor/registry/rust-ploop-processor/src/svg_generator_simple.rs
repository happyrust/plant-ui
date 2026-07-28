use crate::{vertex::*, ploop::*, fradius::*, error::*, debug_println};
use std::fs;

/// 简化的SVG生成器
pub struct SimpleSvgGenerator {
    width: u32,
    height: u32,
    margin: u32,
    fradius_processor: FradiusProcessor,
}

impl SimpleSvgGenerator {
    pub fn new() -> Self {
        Self {
            width: 1000,
            height: 800,
            margin: 60,
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

        // 保存缩放因子供圆弧使用
        let scale_factor = scale;
        
        // 开始生成SVG
        let mut svg_content = String::new();
        
        // SVG头部
        svg_content.push_str(&format!(
            "<svg width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\" xmlns=\"http://www.w3.org/2000/svg\">\n",
            self.width, self.height, self.width, self.height
        ));
        
        // 背景
        svg_content.push_str(&format!(
            "<rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"white\" stroke=\"none\"/>\n",
            self.width, self.height
        ));
        
        // 标题
        svg_content.push_str(&format!(
            "<text x=\"{}\" y=\"25\" text-anchor=\"middle\" font-family=\"Arial\" font-size=\"16\" fill=\"black\">PLOOP Profile: {} (高度: {:.1}mm)</text>\n",
            self.width / 2, ploop.name, ploop.height
        ));
        
        // 生成路径 - 传递原始PLOOP顶点用于圆弧计算
        let path_data = self.generate_path_with_arcs(ploop, processed_vertices, &transform_x, &transform_y, scale_factor)?;
        svg_content.push_str(&format!(
            "<path d=\"{}\" fill=\"none\" stroke=\"blue\" stroke-width=\"2\"/>\n",
            path_data
        ));
        
        // 添加顶点标记
        for (i, vertex) in processed_vertices.iter().enumerate() {
            let x = transform_x(vertex.x());
            let y = transform_y(vertex.y());
            
            // 顶点圆圈
            svg_content.push_str(&format!(
                "<circle cx=\"{:.0}\" cy=\"{:.0}\" r=\"3\" fill=\"red\" stroke=\"white\" stroke-width=\"1\"/>\n",
                x, y
            ));
            
            // 顶点编号
            svg_content.push_str(&format!(
                "<text x=\"{:.0}\" y=\"{:.0}\" font-family=\"Arial\" font-size=\"10\" font-weight=\"bold\" fill=\"black\">{}</text>\n",
                x + 8.0, y - 8.0, i
            ));
        }
        
        // 添加FRADIUS标记
        for vertex in &ploop.vertices {
            if vertex.has_fradius() {
                let x = transform_x(vertex.x());
                let y = transform_y(vertex.y());
                
                svg_content.push_str(&format!(
                    "<circle cx=\"{:.0}\" cy=\"{:.0}\" r=\"4\" fill=\"none\" stroke=\"orange\" stroke-width=\"2\"/>\n",
                    x, y
                ));
                
                svg_content.push_str(&format!(
                    "<text x=\"{:.0}\" y=\"{:.0}\" font-family=\"Arial\" font-size=\"8\" fill=\"orange\">R{:.0}</text>\n",
                    x + 10.0, y - 10.0, vertex.get_fradius()
                ));
            }
        }
        
        // 添加简单网格
        let grid_step = 50;
        for i in (self.margin..=self.width - self.margin).step_by(grid_step as usize) {
            svg_content.push_str(&format!(
                "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"lightgray\" stroke-width=\"1\" opacity=\"0.3\"/>\n",
                i, self.height - self.margin, i, self.margin
            ));
        }
        
        for i in (self.margin..=self.height - self.margin).step_by(grid_step as usize) {
            svg_content.push_str(&format!(
                "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"lightgray\" stroke-width=\"1\" opacity=\"0.3\"/>\n",
                self.margin, i, self.width - self.margin, i
            ));
        }
        
        // 添加坐标表格
        self.add_coordinate_table(&mut svg_content, processed_vertices);
        
        svg_content.push_str("</svg>\n");
        
        // 保存到文件
        fs::write(filename, &svg_content)
            .map_err(|e| PLoopError::IoError(e))?;
        
        debug_println!("SVG已保存到: {}", filename);
        
        Ok(svg_content)
    }
    
    /// 计算边界
    fn calculate_bounds(&self, vertices: &[Vertex]) -> (f64, f64, f64, f64) {
        if vertices.is_empty() {
            return (0.0, 1.0, 0.0, 1.0);
        }
        
        let mut min_x = vertices[0].x();
        let mut max_x = vertices[0].x();
        let mut min_y = vertices[0].y();
        let mut max_y = vertices[0].y();
        
        for vertex in vertices {
            min_x = min_x.min(vertex.x());
            max_x = max_x.max(vertex.x());
            min_y = min_y.min(vertex.y());
            max_y = max_y.max(vertex.y());
        }
        
        (min_x, max_x, min_y, max_y)
    }
    
    /// 生成带圆弧的路径 - 直接处理原始PLOOP中的FRADIUS
    fn generate_path_with_arcs(
        &self,
        ploop: &PLoop,
        processed_vertices: &[Vertex],
        transform_x: &dyn Fn(f64) -> f64,
        transform_y: &dyn Fn(f64) -> f64,
        scale_factor: f64,
    ) -> Result<String> {
        if processed_vertices.is_empty() {
            return Ok(String::new());
        }

        let mut path_commands = Vec::new();

        // 使用处理后的顶点绘制路径
        let mut path_vertices = processed_vertices.to_vec();

        // 确保闭合：手动添加起始顶点作为闭合顶点（与Python版本保持一致）
        if !path_vertices.is_empty() {
            let first = path_vertices[0].clone();
            let last = &path_vertices[path_vertices.len() - 1];

            // 检查首尾顶点是否重合
            let distance = ((first.x() - last.x()).powi(2) + (first.y() - last.y()).powi(2)).sqrt();
            if distance > 1.0 { // 如果距离大于1mm，则添加闭合顶点
                path_vertices.push(first);
                debug_println!("  添加闭合顶点: ({:.2}, {:.2})", processed_vertices[0].x(), processed_vertices[0].y());
            } else {
                debug_println!("  首尾顶点已经重合，无需添加闭合顶点");
            }
        }

        debug_println!("  路径顶点数: {}", path_vertices.len());
        debug_println!("  原始PLOOP顶点数: {}", ploop.vertices.len());

        // 起始点
        let start_x = transform_x(path_vertices[0].x());
        let start_y = transform_y(path_vertices[0].y());
        path_commands.push(format!("M {:.2} {:.2}", start_x, start_y));

        // 检测连续相同FRADIUS来决定是否生成大圆弧
        let fradius_vertices: Vec<_> = ploop.vertices.iter()
            .enumerate()
            .filter(|(_, v)| v.has_fradius())
            .collect();

        debug_println!("  找到{}个FRADIUS顶点", fradius_vertices.len());

        // 检查是否所有FRADIUS都相同
        let mut all_same_fradius = true;
        if fradius_vertices.len() > 1 {
            let first_fradius = fradius_vertices[0].1.get_fradius();
            for (_, vertex) in &fradius_vertices {
                if (vertex.get_fradius() - first_fradius).abs() > 0.01 {
                    all_same_fradius = false;
                    break;
                }
            }
        }

        if all_same_fradius && fradius_vertices.len() >= 2 {
            // 连续相同FRADIUS（2个或更多）：生成一个大圆弧
            let radius = fradius_vertices[0].1.get_fradius();
            let radius_scaled = radius * scale_factor;

            debug_println!("  检测到连续相同FRADIUS: {:.2}mm, 生成大圆弧", radius);

            // 计算大圆弧从起点到终点
            // Framework_8: 从顶点0 (0, 0.05) 到顶点4 (0, 5557.34)
            let start_point = &path_vertices[0];  // (0, 0.05)
            // 如果最后一个顶点是闭合点（与起点相同），则使用倒数第二个顶点
            let end_point_idx = if path_vertices.len() > 1 &&
                ((path_vertices[path_vertices.len() - 1].x() - start_point.x()).abs() < 1.0 &&
                 (path_vertices[path_vertices.len() - 1].y() - start_point.y()).abs() < 1.0) {
                path_vertices.len() - 2  // 倒数第二个顶点
            } else {
                path_vertices.len() - 1  // 最后一个顶点
            };
            let end_point = &path_vertices[end_point_idx];

            debug_println!("  起点: ({:.2}, {:.2})", start_point.x(), start_point.y());
            debug_println!("  终点: ({:.2}, {:.2})", end_point.x(), end_point.y());

            // 直线距离
            let chord_length = ((end_point.x() - start_point.x()).powi(2) +
                               (end_point.y() - start_point.y()).powi(2)).sqrt();

            debug_println!("  弦长: {:.2}mm", chord_length);
            debug_println!("  圆弧半径: {:.2}mm", radius);

            // 计算圆弧参数
            let end_x = transform_x(end_point.x());
            let end_y = transform_y(end_point.y());

            // 对于大半径的圆弧，需要确定是大弧还是小弧
            // 如果弦长 < 直径，则是小弧；否则是大弧
            let is_large_arc = chord_length > radius;
            let large_arc_flag = if is_large_arc { 1 } else { 0 };
            let sweep_flag = 1; // 顺时针

            debug_println!("  弦长: {:.2}, 半径: {:.2}, 大弧标志: {}",
                chord_length, radius, large_arc_flag);

            // 生成大圆弧命令
            path_commands.push(format!(
                "A {:.2} {:.2} 0 {} {} {:.2} {:.2}",
                radius_scaled, radius_scaled, large_arc_flag, sweep_flag, end_x, end_y
            ));

            debug_println!("    大圆弧命令: A {:.2} {:.2} 0 {} {} {:.2} {:.2}",
                radius_scaled, radius_scaled, large_arc_flag, sweep_flag, end_x, end_y);

        } else {
            // 非连续相同FRADIUS：按原有逻辑处理
            debug_println!("  非连续相同FRADIUS，使用标准处理");

            let mut processed_idx = 1;
            for orig_idx in 1..ploop.vertices.len() {
                let orig_vertex = &ploop.vertices[orig_idx];

                if orig_vertex.has_fradius() {
                    debug_println!("  处理原始FRADIUS顶点[{}]: ({:.2}, {:.2}) FRADIUS:{:.2}",
                        orig_idx, orig_vertex.x(), orig_vertex.y(), orig_vertex.get_fradius());

                    let radius = orig_vertex.get_fradius();
                    let radius_scaled = radius * scale_factor;

                    if processed_idx < path_vertices.len() {
                        let end_x = transform_x(path_vertices[processed_idx].x());
                        let end_y = transform_y(path_vertices[processed_idx].y());

                        let large_arc_flag = 0;
                        let sweep_flag = 1;

                        path_commands.push(format!(
                            "A {:.2} {:.2} 0 {} {} {:.2} {:.2}",
                            radius_scaled, radius_scaled, large_arc_flag, sweep_flag, end_x, end_y
                        ));

                        processed_idx += 1;
                    }
                } else {
                    if processed_idx < path_vertices.len() {
                        let x = transform_x(path_vertices[processed_idx].x());
                        let y = transform_y(path_vertices[processed_idx].y());
                        path_commands.push(format!("L {:.2} {:.2}", x, y));
                        processed_idx += 1;
                    }
                }
            }

            // 处理剩余的顶点
            while processed_idx < path_vertices.len() {
                let x = transform_x(path_vertices[processed_idx].x());
                let y = transform_y(path_vertices[processed_idx].y());
                path_commands.push(format!("L {:.2} {:.2}", x, y));
                processed_idx += 1;
            }
        }
        
        // 检查是否需要闭合路径
        // 如果最后一个顶点不是起始顶点，则添加闭合线段
        if path_vertices.len() > 3 {
            let first = &path_vertices[0];
            let last = &path_vertices[path_vertices.len() - 1];
            let distance = ((first.x() - last.x()).powi(2) + (first.y() - last.y()).powi(2)).sqrt();

            if distance < 1.0 {
                // 如果最后一个顶点就是起始顶点，使用Z命令闭合
                path_commands.push("Z".to_string());
                debug_println!("    使用Z命令闭合路径");
            } else {
                debug_println!("    路径已经通过顶点自然闭合");
            }
        }

        Ok(path_commands.join(" "))
    }
    
    /// 添加坐标表格
    fn add_coordinate_table(&self, svg_content: &mut String, vertices: &[Vertex]) {
        let table_width = 360; // 增加宽度以容纳 FRADIUS 列
        let table_height = (vertices.len().min(20) * 12 + 40).min(280);
        let table_x = self.width - table_width - 10;
        let table_y = self.height - table_height as u32 - 10;
        
        // 表格背景
        svg_content.push_str(&format!(
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"white\" stroke=\"gray\" stroke-width=\"1\" opacity=\"0.95\" rx=\"5\"/>\n",
            table_x, table_y, table_width, table_height
        ));
        
        // 表格标题
        svg_content.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" text-anchor=\"middle\" font-family=\"Arial\" font-size=\"12\" font-weight=\"bold\" fill=\"black\">顶点坐标表</text>\n",
            table_x + table_width / 2, table_y + 15
        ));
        
        // 表头
        let header_y = table_y + 30;
        svg_content.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" font-family=\"Arial\" font-size=\"10\" font-weight=\"bold\" fill=\"gray\">序号</text>\n",
            table_x + 10, header_y
        ));
        svg_content.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" font-family=\"Arial\" font-size=\"10\" font-weight=\"bold\" fill=\"gray\">X (mm)</text>\n",
            table_x + 50, header_y
        ));
        svg_content.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" font-family=\"Arial\" font-size=\"10\" font-weight=\"bold\" fill=\"gray\">Y (mm)</text>\n",
            table_x + 130, header_y
        ));
        svg_content.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" font-family=\"Arial\" font-size=\"10\" font-weight=\"bold\" fill=\"orange\">FRADIUS</text>\n",
            table_x + 210, header_y
        ));
        
        // 分隔线
        svg_content.push_str(&format!(
            "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"lightgray\" stroke-width=\"1\"/>\n",
            table_x + 5, header_y + 3, table_x + table_width - 5, header_y + 3
        ));
        
        // 坐标数据
        let max_display = vertices.len().min(20);
        for (i, vertex) in vertices.iter().take(max_display).enumerate() {
            let row_y = header_y + 15 + (i as u32) * 12;

            svg_content.push_str(&format!(
                "<text x=\"{}\" y=\"{}\" text-anchor=\"middle\" font-family=\"Arial\" font-size=\"9\" fill=\"black\">{}</text>\n",
                table_x + 15, row_y, i
            ));
            svg_content.push_str(&format!(
                "<text x=\"{}\" y=\"{}\" font-family=\"Arial\" font-size=\"9\" fill=\"black\">{:.2}</text>\n",
                table_x + 55, row_y, vertex.x()
            ));
            svg_content.push_str(&format!(
                "<text x=\"{}\" y=\"{}\" font-family=\"Arial\" font-size=\"9\" fill=\"black\">{:.2}</text>\n",
                table_x + 135, row_y, vertex.y()
            ));

            // 添加 FRADIUS 信息
            if let Some(fradius) = vertex.fradius {
                svg_content.push_str(&format!(
                    "<text x=\"{}\" y=\"{}\" font-family=\"Arial\" font-size=\"9\" font-weight=\"bold\" fill=\"orange\">{:.1}</text>\n",
                    table_x + 215, row_y, fradius
                ));
            } else {
                svg_content.push_str(&format!(
                    "<text x=\"{}\" y=\"{}\" font-family=\"Arial\" font-size=\"9\" fill=\"gray\">-</text>\n",
                    table_x + 215, row_y
                ));
            }
        }
    }
}

impl Default for SimpleSvgGenerator {
    fn default() -> Self {
        Self::new()
    }
}
