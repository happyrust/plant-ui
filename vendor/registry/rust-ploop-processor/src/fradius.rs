use crate::{vertex::*, error::*, debug_println};
use glam::DVec2;
use std::f64::consts::PI;

/// DVec2的扩展trait，添加cross方法
trait DVec2Ext {
    fn cross(&self, other: DVec2) -> f64;
}

impl DVec2Ext for DVec2 {
    fn cross(&self, other: DVec2) -> f64 {
        self.x * other.y - self.y * other.x
    }
}

/// 圆弧信息结构
#[derive(Debug, Clone)]
pub struct ArcInfo {
    pub center: DVec2,
    pub radius: f64,
    pub start_point: DVec2,
    pub end_point: DVec2,
    pub large_arc: bool,
    pub sweep_flag: bool,
    pub angle_diff: f64,
}

/// FRADIUS处理器
pub struct FradiusProcessor {
    tolerance: f64,
}

impl FradiusProcessor {
    pub fn new(tolerance: f64) -> Self {
        Self { tolerance }
    }
    
    /// 计算圆角弧线信息
    pub fn calculate_fillet_arc_info(
        &self,
        prev_vertex: &Vertex,
        vertex: &Vertex,
        next_vertex: &Vertex,
    ) -> Result<Option<ArcInfo>> {
        if !vertex.has_fradius() {
            return Ok(None);
        }
        
        let radius = vertex.get_fradius();
        if radius <= 0.0 {
            return Ok(None);
        }
        
        // 计算前后边的向量
        let v1 = DVec2::new(vertex.x() - prev_vertex.x(), vertex.y() - prev_vertex.y());
        let v2 = DVec2::new(next_vertex.x() - vertex.x(), next_vertex.y() - vertex.y());

        let len1 = v1.length();
        let len2 = v2.length();
        
        if len1 < self.tolerance || len2 < self.tolerance {
            return Ok(None);
        }
        
        // 标准化向量
        let v1_norm = v1.normalize();
        let v2_norm = v2.normalize();

        // 计算夹角
        let dot_product = v1_norm.dot(v2_norm).clamp(-1.0, 1.0);
        let half_angle = dot_product.abs().acos() / 2.0;
        
        if half_angle < 0.001 {
            return Ok(None); // 角度太小，跳过圆角
        }
        
        // 计算切点距离
        let mut tangent_distance = radius / half_angle.tan();

        debug_println!("    圆弧计算: 半径={:.2}mm, 原始切点距离={:.2}mm, 边长1={:.2}mm, 边长2={:.2}mm",
            radius, tangent_distance, len1, len2);

        // 限制切点距离，防止超出边长（与Python版本保持一致）
        let max_dist1 = len1 * 0.9;
        let max_dist2 = len2 * 0.9;
        let original_distance = tangent_distance;
        tangent_distance = tangent_distance.min(max_dist1).min(max_dist2);

        if tangent_distance != original_distance {
            debug_println!("    切点距离限制: {:.2} → {:.2} (边长限制: {:.2}, {:.2})",
                original_distance, tangent_distance, max_dist1, max_dist2);
        }
        
        // 计算切点
        let tangent1 = DVec2::new(
            vertex.x() - v1_norm.x * tangent_distance,
            vertex.y() - v1_norm.y * tangent_distance,
        );

        let tangent2 = DVec2::new(
            vertex.x() + v2_norm.x * tangent_distance,
            vertex.y() + v2_norm.y * tangent_distance,
        );

        // 计算角平分线
        let bisector = (v1_norm + (-v2_norm)).normalize();

        // 判断凸凹性
        let cross_product = v1_norm.cross(v2_norm);
        let radius_sign = if cross_product > 0.0 { 1.0 } else { -1.0 };

        // 计算圆心
        let center_dist = radius / half_angle.sin();
        let center = DVec2::new(
            vertex.x() + bisector.x * center_dist * radius_sign,
            vertex.y() + bisector.y * center_dist * radius_sign,
        );
        
        // 计算角度差 - 注意：为了与Python版本保持一致，交换角度顺序
        let angle1 = (tangent2.y - center.y).atan2(tangent2.x - center.x);
        let angle2 = (tangent1.y - center.y).atan2(tangent1.x - center.x);

        let mut angle_diff = angle2 - angle1;

        debug_println!("    角度计算: angle1={:.4}, angle2={:.4}, 原始angle_diff={:.4}",
            angle1, angle2, angle_diff);

        // 标准化角度差
        if angle_diff.abs() > PI {
            if angle_diff > 0.0 {
                angle_diff -= 2.0 * PI;
            } else {
                angle_diff += 2.0 * PI;
            }
        }

        debug_println!("    标准化后: angle_diff={:.4}, sweep_flag={}",
            angle_diff, angle_diff > 0.0);

        // 确定SVG弧线标志
        let large_arc = angle_diff.abs() > PI;
        let sweep_flag = angle_diff > 0.0;
        
        Ok(Some(ArcInfo {
            center,
            radius,
            start_point: tangent1,
            end_point: tangent2,
            large_arc,
            sweep_flag,
            angle_diff,
        }))
    }
    
    /// 处理FRADIUS顶点，返回切点顶点
    pub fn process_fradius_vertex(
        &self,
        prev_vertex: &Vertex,
        vertex: &Vertex,
        next_vertex: &Vertex,
    ) -> Result<Vec<Vertex>> {
        if !vertex.has_fradius() {
            return Ok(vec![vertex.clone()]);
        }
        
        let arc_info = self.calculate_fillet_arc_info(prev_vertex, vertex, next_vertex)?;
        
        if let Some(arc_info) = arc_info {
            // 返回切点，第一个切点保留FRADIUS信息用于SVG绘制
            let start_vertex = Vertex::with_fradius(
                arc_info.start_point.x,
                arc_info.start_point.y,
                vertex.z(),
                vertex.get_fradius(),
            );

            let end_vertex = Vertex::new(
                arc_info.end_point.x,
                arc_info.end_point.y,
                vertex.z(),
            );

            Ok(vec![start_vertex, end_vertex])
        } else {
            Ok(vec![vertex.clone()])
        }
    }
}
