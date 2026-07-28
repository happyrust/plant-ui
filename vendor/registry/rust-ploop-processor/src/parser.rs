use crate::{vertex::Vertex, ploop::PLoop, error::*, debug_println};
use regex::Regex;

/// PLOOP文件解析器
pub struct PLoopParser {
    tolerance: f64,
}

impl PLoopParser {
    pub fn new(tolerance: f64) -> Self {
        Self { tolerance }
    }
    
    /// 解析PLOOP文件内容
    pub fn parse_file(&self, content: &str) -> Result<Vec<PLoop>> {
        let mut ploops = Vec::new();
        let lines: Vec<&str> = content.lines().collect();
        
        let mut current_ploop: Option<PLoop> = None;
        let mut current_vertex: Option<VertexBuilder> = None;
        let mut framework_name = "Unknown".to_string();
        let mut panel_count = 0;
        
        for (line_num, line) in lines.iter().enumerate() {
            let line = line.trim();
            
            // 检测FRMWORK名称
            if line.starts_with("NEW FRMWORK") {
                // 如果有当前PLOOP，先完成它
                if let Some(ploop) = current_ploop.take() {
                    debug_println!("  完成PLOOP: {}", ploop);
                    ploops.push(ploop);
                }

                let parts: Vec<&str> = line.split_whitespace().collect();
                framework_name = if parts.len() > 2 {
                    parts[2].to_string()
                } else {
                    format!("Framework_{}", line_num + 1)
                };
                panel_count = 0; // 重置 PANEL 计数
                debug_println!("发现FRMWORK: {}", framework_name);
            }
            // 检测PANEL开始
            else if line == "NEW PANEL" {
                panel_count += 1;
                debug_println!("  发现PANEL: {}", panel_count);
            }
            // 检测PLOOP开始
            else if line == "NEW PLOOP" {
                // 如果有当前PLOOP，先完成它
                if let Some(ploop) = current_ploop.take() {
                    debug_println!("  完成PLOOP: {}", ploop);
                    ploops.push(ploop);
                }

                let ploop_name = if panel_count > 0 {
                    format!("{}_Panel_{}", framework_name, panel_count)
                } else {
                    format!("{}_Panel_1", framework_name)
                };
                current_ploop = Some(PLoop::new(ploop_name.clone(), 0.0));
                debug_println!("  开始PLOOP: {}", ploop_name);
            }
            // 检测PLOOP高度
            else if line.starts_with("HEIG") {
                if let Some(ref mut ploop) = current_ploop {
                    if let Some(height) = self.parse_height(line)? {
                        ploop.height = height;
                        debug_println!("    高度: {}mm", height);
                    }
                }
            }
            // 检测顶点开始
            else if line == "NEW PAVERT" {
                current_vertex = Some(VertexBuilder::new());
            }
            // 检测位置信息
            else if line.starts_with("POS") {
                if let Some(ref mut vertex) = current_vertex {
                    let (x, y, z) = self.parse_position(line)?;
                    vertex.set_position(x, y, z);
                }
            }
            // 检测圆角半径
            else if line.starts_with("FRAD") {
                if let Some(ref mut vertex) = current_vertex {
                    if let Some(fradius) = self.parse_fradius(line)? {
                        vertex.set_fradius(fradius);
                        debug_println!("      发现FRAD: {}mm", fradius);
                    }
                }
            }
            // 检测顶点结束
            else if line == "END" && current_vertex.is_some() {
                if let (Some(ref mut ploop), Some(vertex_builder)) = (&mut current_ploop, current_vertex.take()) {
                    let vertex = vertex_builder.build();
                    debug_println!("      添加顶点: {}", vertex);
                    ploop.add_vertex(vertex);
                }
            }
            // 检测PLOOP结束
            else if line == "END PLOOP" {
                if let Some(ploop) = current_ploop.take() {
                    debug_println!("  完成PLOOP: {}", ploop);
                    ploops.push(ploop);
                }
            }
        }
        
        // 处理最后一个PLOOP（如果没有显式的END PLOOP）
        if let Some(ploop) = current_ploop {
            debug_println!("  完成PLOOP: {}", ploop);
            ploops.push(ploop);
        }
        
        Ok(ploops)
    }
    
    /// 解析位置信息
    fn parse_position(&self, line: &str) -> Result<(f64, f64, f64)> {
        // 首先尝试解析 X/Y/Z 格式: POS X 0mm Y 0.05mm Z 0mm
        let xyz_re = Regex::new(r"POS\s+X\s+([-\d.]+)mm\s+Y\s+([-\d.]+)mm\s+Z\s+([-\d.]+)mm")
            .map_err(|e| PLoopError::ParseError(format!("正则表达式错误: {}", e)))?;

        if let Some(caps) = xyz_re.captures(line) {
            let x = caps[1].parse::<f64>()
                .map_err(|e| PLoopError::ParseError(format!("解析X坐标失败: {}", e)))?;
            let y = caps[2].parse::<f64>()
                .map_err(|e| PLoopError::ParseError(format!("解析Y坐标失败: {}", e)))?;
            let z = caps[3].parse::<f64>()
                .map_err(|e| PLoopError::ParseError(format!("解析Z坐标失败: {}", e)))?;
            return Ok((x, y, z));
        }

        // 如果X/Y/Z格式失败，尝试方向性坐标格式
        let re = Regex::new(r"POS\s+([EWNS])\s+([-\d.]+)mm\s+([EWNS])\s+([-\d.]+)mm\s+([UZ])\s+([-\d.]+)mm")
            .map_err(|e| PLoopError::ParseError(format!("正则表达式错误: {}", e)))?;

        if let Some(caps) = re.captures(line) {
            let axis1 = &caps[1];
            let val1 = caps[2].parse::<f64>()
                .map_err(|e| PLoopError::ParseError(format!("解析坐标1失败: {}", e)))?;
            let axis2 = &caps[3];
            let val2 = caps[4].parse::<f64>()
                .map_err(|e| PLoopError::ParseError(format!("解析坐标2失败: {}", e)))?;
            let _axis3 = &caps[5];
            let val3 = caps[6].parse::<f64>()
                .map_err(|e| PLoopError::ParseError(format!("解析坐标3失败: {}", e)))?;

            // 根据坐标轴标识符确定X和Y坐标
            // 注意：南向(S)坐标应该是负Y，北向(N)坐标是正Y
            let (x, y, z) = match (axis1, axis2) {
                ("E", "N") => (val1, val2, val3),   // 东-北坐标系
                ("W", "N") => (-val1, val2, val3),  // 西-北坐标系（西向为负X）
                ("W", "S") => (-val1, -val2, val3), // 西-南坐标系（西向负X，南向负Y）
                ("E", "S") => (val1, -val2, val3),  // 东-南坐标系（南向为负Y）
                ("N", "E") => (val2, val1, val3),   // 北-东坐标系
                ("N", "W") => (-val2, val1, val3),  // 北-西坐标系（西向为负X）
                ("S", "E") => (val2, -val1, val3),  // 南-东坐标系（南向为负Y）
                ("S", "W") => (-val2, -val1, val3), // 南-西坐标系（西向负X，南向负Y）
                _ => (val1, val2, val3),            // 默认情况
            };

            Ok((x, y, z))
        } else {
            Err(PLoopError::ParseError(format!("无法解析位置信息: {}", line)))
        }
    }
    
    /// 解析高度信息
    fn parse_height(&self, line: &str) -> Result<Option<f64>> {
        let re = Regex::new(r"HEIG\s+([\d.-]+)mm")
            .map_err(|e| PLoopError::ParseError(format!("正则表达式错误: {}", e)))?;
        
        if let Some(caps) = re.captures(line) {
            let height = caps[1].parse::<f64>()
                .map_err(|e| PLoopError::ParseError(format!("解析高度失败: {}", e)))?;
            Ok(Some(height))
        } else {
            Ok(None)
        }
    }
    
    /// 解析FRADIUS信息
    fn parse_fradius(&self, line: &str) -> Result<Option<f64>> {
        let re = Regex::new(r"FRAD\s+([\d.-]+)mm")
            .map_err(|e| PLoopError::ParseError(format!("正则表达式错误: {}", e)))?;
        
        if let Some(caps) = re.captures(line) {
            let fradius = caps[1].parse::<f64>()
                .map_err(|e| PLoopError::ParseError(format!("解析FRADIUS失败: {}", e)))?;
            Ok(Some(fradius))
        } else {
            Ok(None)
        }
    }
}

/// 顶点构建器
#[derive(Debug)]
struct VertexBuilder {
    x: f64,
    y: f64,
    z: f64,
    fradius: Option<f64>,
}

impl VertexBuilder {
    fn new() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            fradius: None,
        }
    }
    
    fn set_position(&mut self, x: f64, y: f64, z: f64) {
        self.x = x;
        self.y = y;
        self.z = z;
    }
    
    fn set_fradius(&mut self, fradius: f64) {
        self.fradius = Some(fradius);
    }
    
    fn build(self) -> Vertex {
        if let Some(fradius) = self.fradius {
            Vertex::with_fradius(self.x, self.y, self.z, fradius)
        } else {
            Vertex::new(self.x, self.y, self.z)
        }
    }
}
