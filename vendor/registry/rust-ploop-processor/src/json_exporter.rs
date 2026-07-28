use crate::{PLoop, Vertex, Result, debug_println};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// 用于3D显示的顶点数据结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlotVertex {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub fradius: Option<f64>,
}

impl From<&Vertex> for PlotVertex {
    fn from(vertex: &Vertex) -> Self {
        Self {
            x: vertex.x(),
            y: vertex.y(),
            z: vertex.z(),
            fradius: if vertex.has_fradius() {
                Some(vertex.get_fradius())
            } else {
                None
            },
        }
    }
}

/// 用于3D显示的PLOOP数据结构
#[derive(Debug, Serialize, Deserialize)]
pub struct PlotData {
    pub name: String,
    pub height: f64,
    pub vertices: Vec<PlotVertex>,
    pub fradius_count: usize,
}

impl PlotData {
    /// 从PLOOP和处理后的顶点创建PlotData
    pub fn from_ploop(ploop: &PLoop, processed_vertices: &[Vertex]) -> Self {
        let vertices: Vec<PlotVertex> = processed_vertices.iter().map(|v| v.into()).collect();
        let fradius_count = vertices.iter().filter(|v| v.fradius.is_some()).count();
        
        Self {
            name: ploop.name.clone(),
            height: ploop.height,
            vertices,
            fradius_count,
        }
    }
    
    /// 保存到JSON文件
    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)?;
        Ok(())
    }
    
    /// 从JSON文件加载
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        let data: PlotData = serde_json::from_str(&content)?;
        Ok(data)
    }
}

/// JSON导出器
pub struct JsonExporter;

impl JsonExporter {
    /// 导出PLOOP数据为JSON格式
    pub fn export_ploop(
        ploop: &PLoop,
        processed_vertices: &[Vertex],
        output_path: &str,
    ) -> Result<()> {
        let plot_data = PlotData::from_ploop(ploop, processed_vertices);
        plot_data.save_to_file(output_path)?;
        
        debug_println!("JSON数据已保存到: {}", output_path);
        debug_println!("- 案例名称: {}", plot_data.name);
        debug_println!("- 拉伸高度: {:.1}mm", plot_data.height);
        debug_println!("- 顶点数量: {}", plot_data.vertices.len());
        debug_println!("- FRADIUS数量: {}", plot_data.fradius_count);
        
        Ok(())
    }
}
