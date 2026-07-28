use crate::vertex::Vertex;
use serde::{Deserialize, Serialize};

/// PLOOP数据结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PLoop {
    pub name: String,
    pub height: f64,
    pub vertices: Vec<Vertex>,
}

impl PLoop {
    /// 创建新的PLOOP
    pub fn new(name: String, height: f64) -> Self {
        Self {
            name,
            height,
            vertices: Vec::new(),
        }
    }
    
    /// 添加顶点
    pub fn add_vertex(&mut self, vertex: Vertex) {
        self.vertices.push(vertex);
    }
    
    /// 获取顶点数量
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }
    
    /// 获取FRADIUS顶点数量
    pub fn fradius_count(&self) -> usize {
        self.vertices.iter().filter(|v| v.has_fradius()).count()
    }
    
    /// 获取所有FRADIUS顶点
    pub fn fradius_vertices(&self) -> Vec<&Vertex> {
        self.vertices.iter().filter(|v| v.has_fradius()).collect()
    }
    
    /// 计算边界框
    pub fn bounding_box(&self) -> Option<(f64, f64, f64, f64)> {
        if self.vertices.is_empty() {
            return None;
        }

        let mut min_x = self.vertices[0].x();
        let mut max_x = self.vertices[0].x();
        let mut min_y = self.vertices[0].y();
        let mut max_y = self.vertices[0].y();

        for vertex in &self.vertices {
            min_x = min_x.min(vertex.x());
            max_x = max_x.max(vertex.x());
            min_y = min_y.min(vertex.y());
            max_y = max_y.max(vertex.y());
        }

        Some((min_x, max_x, min_y, max_y))
    }
    
    /// 计算几何中心
    pub fn centroid(&self) -> Option<(f64, f64)> {
        if self.vertices.is_empty() {
            return None;
        }

        let sum_x: f64 = self.vertices.iter().map(|v| v.x()).sum();
        let sum_y: f64 = self.vertices.iter().map(|v| v.y()).sum();
        let count = self.vertices.len() as f64;

        Some((sum_x / count, sum_y / count))
    }
    
    /// 检查是否为闭合多边形
    pub fn is_closed(&self, tolerance: f64) -> bool {
        if self.vertices.len() < 3 {
            return false;
        }
        
        let first = &self.vertices[0];
        let last = &self.vertices[self.vertices.len() - 1];
        first.is_near_2d(last, tolerance)
    }
    
    /// 确保多边形闭合
    pub fn ensure_closed(&mut self, tolerance: f64) {
        if !self.is_closed(tolerance) && !self.vertices.is_empty() {
            let first = self.vertices[0].clone();
            self.vertices.push(first);
        }
    }
}

impl std::fmt::Display for PLoop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "PLOOP {}: H={:.1}mm, {} vertices",
            self.name,
            self.height,
            self.vertices.len()
        )
    }
}
