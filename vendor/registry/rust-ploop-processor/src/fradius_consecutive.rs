use crate::{vertex::*, debug_println};
use glam::DVec2;
use std::collections::HashMap;

/// 连续FRADIUS组
#[derive(Debug, Clone)]
pub struct ConsecutiveFradiusGroup {
    pub start_idx: usize,
    pub end_idx: usize,
    pub vertices: Vec<Vertex>,
    pub original_fradius: f64,
}

/// 连续FRADIUS处理器
pub struct ConsecutiveFradiusProcessor {
    tolerance: f64,
}

impl ConsecutiveFradiusProcessor {
    pub fn new(tolerance: f64) -> Self {
        Self { tolerance }
    }

    /// 检测连续的FRADIUS顶点组
    pub fn detect_consecutive_fradius_groups(&self, vertices: &[Vertex]) -> Vec<ConsecutiveFradiusGroup> {
        let mut groups = Vec::new();
        let n = vertices.len();
        if n < 3 {
            return groups;
        }

        let mut i = 0;
        while i < n {
            if vertices[i].has_fradius() {
                let start_idx = i;
                let fradius_value = vertices[i].get_fradius();
                let mut consecutive_vertices = vec![vertices[i].clone()];

                // 查找连续的FRADIUS顶点
                i += 1;
                while i < n && vertices[i].has_fradius() &&
                      (vertices[i].get_fradius() - fradius_value).abs() < self.tolerance {
                    consecutive_vertices.push(vertices[i].clone());
                    i += 1;
                }

                let end_idx = i - 1;

                // 如果有2个或更多连续的FRADIUS顶点，记录为组
                if consecutive_vertices.len() >= 2 {
                    debug_println!("🔍 检测到连续FRADIUS组: 索引[{} - {}], {} 个顶点, 半径: {:.2}mm",
                        start_idx, end_idx, consecutive_vertices.len(), fradius_value);

                    groups.push(ConsecutiveFradiusGroup {
                        start_idx,
                        end_idx,
                        vertices: consecutive_vertices,
                        original_fradius: fradius_value,
                    });
                }
            } else {
                i += 1;
            }
        }

        groups
    }

    /// 特殊处理连续FRADIUS组（E3D风格）
    pub fn process_consecutive_groups(&self, vertices: &[Vertex]) -> Vec<Vertex> {
        let groups = self.detect_consecutive_fradius_groups(vertices);

        if groups.is_empty() {
            debug_println!("📌 无连续FRADIUS组，使用标准处理");
            return vertices.to_vec();
        }

        debug_println!("🎯 应用连续FRADIUS特殊处理，共{}组", groups.len());

        let mut result = Vec::new();
        let mut processed_indices = std::collections::HashSet::new();

        for (idx, vertex) in vertices.iter().enumerate() {
            if processed_indices.contains(&idx) {
                continue;
            }

            // 检查当前顶点是否属于某个连续FRADIUS组
            if let Some(group) = groups.iter().find(|g| idx >= g.start_idx && idx <= g.end_idx) {
                // 特殊处理连续FRADIUS组
                if idx == group.start_idx {
                    let processed_group = self.process_single_consecutive_group(group, vertices);
                    result.extend(processed_group);

                    // 标记整个组为已处理
                    for i in group.start_idx..=group.end_idx {
                        processed_indices.insert(i);
                    }
                }
            } else {
                // 普通顶点，直接添加
                result.push(vertex.clone());
            }
        }

        result
    }

    /// 处理单个连续FRADIUS组
    fn process_single_consecutive_group(
        &self,
        group: &ConsecutiveFradiusGroup,
        all_vertices: &[Vertex]
    ) -> Vec<Vertex> {
        debug_println!("🔧 处理连续FRADIUS组: 索引[{} - {}]", group.start_idx, group.end_idx);

        // E3D特殊策略：连续大半径FRADIUS可能形成复合曲线
        // 策略1: 检查是否会形成自相交，如果会，则简化处理
        if self.would_cause_self_intersection(group, all_vertices) {
            debug_println!("⚠️  连续FRADIUS可能导致自相交，采用简化策略");
            return self.simplified_consecutive_processing(group, all_vertices);
        }

        // 策略2: 标准连续处理 - 保持所有FRADIUS顶点
        self.standard_consecutive_processing(group)
    }

    /// 检查连续FRADIUS是否会导致自相交
    fn would_cause_self_intersection(
        &self,
        group: &ConsecutiveFradiusGroup,
        all_vertices: &[Vertex]
    ) -> bool {
        // 简化的自相交检测：检查FRADIUS半径是否过大
        let total_perimeter = self.calculate_group_perimeter(group, all_vertices);
        let average_radius = group.original_fradius;

        // 如果平均半径超过周长的30%，可能导致问题
        let threshold = total_perimeter * 0.3;
        let would_intersect = average_radius > threshold;

        if would_intersect {
            debug_println!("🔍 自相交检测: 半径{:.2}mm > 阈值{:.2}mm", average_radius, threshold);
        }

        would_intersect
    }

    /// 计算组的周长
    fn calculate_group_perimeter(&self, group: &ConsecutiveFradiusGroup, all_vertices: &[Vertex]) -> f64 {
        let mut perimeter = 0.0;

        for i in 0..group.vertices.len() - 1 {
            let curr = &group.vertices[i];
            let next = &group.vertices[i + 1];
            perimeter += curr.distance_2d(next);
        }

        // 添加与组外顶点的连接
        if group.start_idx > 0 {
            let prev = &all_vertices[group.start_idx - 1];
            perimeter += prev.distance_2d(&group.vertices[0]);
        }

        if group.end_idx < all_vertices.len() - 1 {
            let next = &all_vertices[group.end_idx + 1];
            perimeter += group.vertices.last().unwrap().distance_2d(next);
        }

        perimeter
    }

    /// 简化的连续FRADIUS处理（避免自相交）
    fn simplified_consecutive_processing(
        &self,
        group: &ConsecutiveFradiusGroup,
        all_vertices: &[Vertex]
    ) -> Vec<Vertex> {
        debug_println!("📐 简化处理: 保留首尾FRADIUS，移除中间顶点");

        // 策略：只保留第一个和最后一个FRADIUS顶点
        let mut result = Vec::new();

        if !group.vertices.is_empty() {
            result.push(group.vertices[0].clone()); // 保留第一个
            if group.vertices.len() > 1 {
                result.push(group.vertices.last().unwrap().clone()); // 保留最后一个
            }
        }

        result
    }

    /// 标准连续FRADIUS处理
    fn standard_consecutive_processing(&self, group: &ConsecutiveFradiusGroup) -> Vec<Vertex> {
        debug_println!("📐 标准处理: 保留所有{}个FRADIUS顶点", group.vertices.len());

        // 标准策略：保持所有FRADIUS顶点不变
        group.vertices.clone()
    }

    /// 检查处理后的结果是否合理
    pub fn validate_result(&self, original: &[Vertex], processed: &[Vertex]) -> bool {
        let original_fradius_count = original.iter().filter(|v| v.has_fradius()).count();
        let processed_fradius_count = processed.iter().filter(|v| v.has_fradius()).count();

        debug_println!("✅ 结果验证: FRADIUS顶点 {} → {}", original_fradius_count, processed_fradius_count);

        // 如果FRADIUS数量大幅减少（超过50%），可能有问题
        if processed_fradius_count < original_fradius_count / 2 {
            debug_println!("⚠️  FRADIUS顶点数量大幅减少，可能需要调整处理策略");
            return false;
        }

        true
    }
}

/// 扩展vertex的距离计算方法
impl Vertex {
    pub fn distance_2d(&self, other: &Vertex) -> f64 {
        let dx = self.x() - other.x();
        let dy = self.y() - other.y();
        (dx * dx + dy * dy).sqrt()
    }
}