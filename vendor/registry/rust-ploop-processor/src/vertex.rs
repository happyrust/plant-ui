use serde::{Deserialize, Serialize};
use glam::{DVec2, DVec3};

/// 3D顶点数据结构
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Vertex {
    pub position: DVec3,
    pub fradius: Option<f64>,
}

impl Vertex {
    /// 创建新顶点
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self {
            position: DVec3::new(x, y, z),
            fradius: None,
        }
    }

    /// 从DVec3创建顶点
    pub fn from_position(position: DVec3) -> Self {
        Self {
            position,
            fradius: None,
        }
    }

    /// 创建带FRADIUS的顶点
    pub fn with_fradius(x: f64, y: f64, z: f64, fradius: f64) -> Self {
        Self {
            position: DVec3::new(x, y, z),
            fradius: Some(fradius),
        }
    }

    /// 从DVec3创建带FRADIUS的顶点
    pub fn from_position_with_fradius(position: DVec3, fradius: f64) -> Self {
        Self {
            position,
            fradius: Some(fradius),
        }
    }
    
    /// 获取X坐标
    pub fn x(&self) -> f64 {
        self.position.x
    }

    /// 获取Y坐标
    pub fn y(&self) -> f64 {
        self.position.y
    }

    /// 获取Z坐标
    pub fn z(&self) -> f64 {
        self.position.z
    }

    /// 获取2D位置向量
    pub fn position_2d(&self) -> DVec2 {
        DVec2::new(self.position.x, self.position.y)
    }

    /// 检查是否有FRADIUS
    pub fn has_fradius(&self) -> bool {
        self.fradius.is_some() && self.fradius.unwrap() > 0.0
    }

    /// 获取FRADIUS值
    pub fn get_fradius(&self) -> f64 {
        self.fradius.unwrap_or(0.0)
    }

    /// 设置FRADIUS值
    pub fn set_fradius(&mut self, radius: f64) {
        self.fradius = Some(radius);
    }

    /// 计算到另一个顶点的距离
    pub fn distance_to(&self, other: &Vertex) -> f64 {
        self.position.distance(other.position)
    }

    /// 计算2D距离（忽略Z坐标）
    pub fn distance_2d_to(&self, other: &Vertex) -> f64 {
        self.position_2d().distance(other.position_2d())
    }

    /// 检查两个顶点是否在容差范围内相等
    pub fn is_near(&self, other: &Vertex, tolerance: f64) -> bool {
        self.distance_to(other) < tolerance
    }

    /// 检查两个顶点在2D平面上是否在容差范围内相等
    pub fn is_near_2d(&self, other: &Vertex, tolerance: f64) -> bool {
        self.distance_2d_to(other) < tolerance
    }
}

impl std::fmt::Display for Vertex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(fradius) = self.fradius {
            write!(f, "({:.2}, {:.2}, {:.2}, FRAD:{:.2})", self.x(), self.y(), self.z(), fradius)
        } else {
            write!(f, "({:.2}, {:.2}, {:.2})", self.x(), self.y(), self.z())
        }
    }
}

/// 2D向量扩展trait
pub trait DVec2Ext {
    fn from_vertex(vertex: &Vertex) -> DVec2;
    fn cross(&self, other: DVec2) -> f64;
}

impl DVec2Ext for DVec2 {
    fn from_vertex(vertex: &Vertex) -> DVec2 {
        DVec2::new(vertex.x(), vertex.y())
    }

    fn cross(&self, other: DVec2) -> f64 {
        self.x * other.y - self.y * other.x
    }
}
