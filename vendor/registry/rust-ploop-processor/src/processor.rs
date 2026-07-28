use crate::{vertex::*, ploop::*, fradius::*, fradius_consecutive::*, error::*, parser::*, DEDUP_TOLERANCE, COLLINEAR_TOLERANCE, debug_println};
use glam::DVec2;

/// DVec2的扩展trait，添加cross方法
trait DVec2Ext {
    fn cross(&self, other: DVec2) -> f64;
}

impl DVec2Ext for DVec2 {
    fn cross(&self, other: DVec2) -> f64 {
        self.x * other.y - self.y * other.x
    }
}

/// PLOOP处理器 - 实现完整的处理流程
pub struct PLoopProcessor {
    tolerance: f64,
    dedup_tolerance: f64,
    collinear_tolerance: f64,
    parser: PLoopParser,
    fradius_processor: FradiusProcessor,
    consecutive_fradius_processor: ConsecutiveFradiusProcessor,
}

impl PLoopProcessor {
    /// 创建新的处理器
    pub fn new() -> Self {
        let tolerance = crate::DEFAULT_TOLERANCE;
        Self {
            tolerance,
            dedup_tolerance: DEDUP_TOLERANCE,
            collinear_tolerance: COLLINEAR_TOLERANCE,
            parser: PLoopParser::new(tolerance),
            fradius_processor: FradiusProcessor::new(tolerance),
            consecutive_fradius_processor: ConsecutiveFradiusProcessor::new(tolerance),
        }
    }

    /// 使用自定义容差创建处理器
    pub fn with_tolerance(tolerance: f64, dedup_tolerance: f64, collinear_tolerance: f64) -> Self {
        Self {
            tolerance,
            dedup_tolerance,
            collinear_tolerance,
            parser: PLoopParser::new(tolerance),
            fradius_processor: FradiusProcessor::new(tolerance),
            consecutive_fradius_processor: ConsecutiveFradiusProcessor::new(tolerance),
        }
    }

    /// 解析PLOOP文件
    pub fn parse_file(&self, content: &str) -> Result<Vec<PLoop>> {
        self.parser.parse_file(content)
    }

    /// 处理PLOOP，生成最终的profile顶点
    pub fn process_ploop(&self, ploop: &PLoop) -> Result<Vec<Vertex>> {
        if ploop.vertices.len() < 3 {
            return Ok(ploop.vertices.clone());
        }

        debug_println!("开始处理PLOOP: {}", ploop.name);

        // 第0步：检测和特殊处理连续FRADIUS
        debug_println!("🔍 检测连续FRADIUS情况...");
        let consecutive_processed = self.consecutive_fradius_processor.process_consecutive_groups(&ploop.vertices);
        debug_println!("  连续FRADIUS处理: {} → {} 个顶点", ploop.vertices.len(), consecutive_processed.len());

        // 创建临时PLOOP用于后续处理
        let mut temp_ploop = ploop.clone();
        temp_ploop.vertices = consecutive_processed;

        // 第一步：处理FRADIUS顶点
        let fradius_processed = self.process_fradius_vertices(&temp_ploop)?;
        debug_println!("  FRADIUS处理完成: {} → {} 个顶点", temp_ploop.vertices.len(), fradius_processed.len());

        // 1.5：自相交检测与修复（Set2D风格）
        let mut pre_aveva_vertices = fradius_processed.clone();
        let intersections = crate::set2d::detect_self_intersections(&pre_aveva_vertices, 1e-6);
        if !intersections.is_empty() {
            debug_println!("  自相交检测：发现 {} 处交点，尝试全局半径缩放回退...", intersections.len());
            if let Some((alpha, fixed)) = self.resolve_self_intersections_with_global_scale(ploop, 0.05, 1.0, 12)? {
                debug_println!("  自相交修复：采用全局缩放 α={:.4} 后无自相交 (顶点: {})", alpha, fixed.len());
                pre_aveva_vertices = fixed;
            } else {
                debug_println!("  自相交修复：全局缩放未找到无交叉解，保留原始结果并进入后续处理");
            }
        } else {
            debug_println!("  自相交检测：未发现交叉");
        }

        // 第二步：AVEVA风格共线处理
        let aveva_processed = self.aveva_collinear_processing(&pre_aveva_vertices)?;
        debug_println!("  AVEVA共线处理完成: {} → {} 个顶点", pre_aveva_vertices.len(), aveva_processed.len());

        Ok(aveva_processed)
    }

    /// 处理FRADIUS顶点 - 正确处理圆弧切点
    fn process_fradius_vertices(&self, ploop: &PLoop) -> Result<Vec<Vertex>> {
        let mut processed_vertices = Vec::new();
        let mut vertices = ploop.vertices.clone();

        // 确保闭合
        if !vertices.is_empty() {
            let first = vertices[0].clone();
            let last = vertices[vertices.len() - 1].clone();
            if !first.is_near_2d(&last, self.tolerance) {
                vertices.push(first);
            }
        }

        let n = vertices.len();
        if n < 3 {
            return Ok(vertices);
        }

        // 检测连续FRADIUS的情况
        let consecutive_groups = self.consecutive_fradius_processor.detect_consecutive_fradius_groups(&vertices);

        if !consecutive_groups.is_empty() {
            debug_println!("🎯 检测到{}个连续FRADIUS组，采用特殊处理策略", consecutive_groups.len());

            // 对于连续FRADIUS，采用特殊策略：保持原始顶点不变
            let mut processed_indices = std::collections::HashSet::new();

            for (idx, vertex) in vertices.iter().enumerate() {
                if idx >= n - 1 { break; } // 跳过最后一个重复点

                if processed_indices.contains(&idx) {
                    continue;
                }

                // 检查是否属于连续FRADIUS组
                if let Some(group) = consecutive_groups.iter().find(|g| idx >= g.start_idx && idx <= g.end_idx) {
                    if idx == group.start_idx {
                        debug_println!("🔧 特殊处理连续FRADIUS组[{}-{}]，保持原始顶点",
                            group.start_idx, group.end_idx);

                        // 策略：保持所有连续FRADIUS顶点的原始状态
                        for group_idx in group.start_idx..=group.end_idx.min(n-2) {
                            let group_vertex = &vertices[group_idx];
                            debug_println!("      保留原始FRADIUS顶点[{}]: ({:.2}, {:.2}) FRADIUS: {:.1}mm",
                                group_idx, group_vertex.x(), group_vertex.y(), group_vertex.get_fradius());
                            processed_vertices.push(group_vertex.clone());
                            processed_indices.insert(group_idx);
                        }
                    }
                } else {
                    // 单独的FRADIUS顶点，正常处理
                    let current = vertex;
                    let prev_vertex = if idx > 0 { &vertices[idx - 1] } else { &vertices[n - 2] };
                    let next_vertex = &vertices[idx + 1];

                    if current.has_fradius() {
                        debug_println!("    处理单独FRADIUS顶点[{}]: ({:.2}, {:.2}) FRADIUS: {:.1}mm",
                            idx, current.x(), current.y(), current.get_fradius());

                        // 正常的FRADIUS处理
                        let arc_vertices = self.fradius_processor.process_fradius_vertex(
                            prev_vertex, current, next_vertex
                        )?;

                        if arc_vertices.len() == 2 {
                            processed_vertices.push(arc_vertices[0].clone());
                            processed_vertices.push(arc_vertices[1].clone());
                        } else {
                            processed_vertices.push(current.clone());
                        }
                    } else {
                        processed_vertices.push(current.clone());
                    }
                }
            }

            debug_println!("    连续FRADIUS特殊处理完成: {} → {} 个顶点", n - 1, processed_vertices.len());
            return Ok(processed_vertices);
        }

        // 标准处理流程（无连续FRADIUS）
        debug_println!("🔧 标准FRADIUS处理，生成切点");

        for i in 0..n - 1 {  // 跳过最后一个重复点
            let current = &vertices[i];
            let prev_vertex = if i > 0 { &vertices[i - 1] } else { &vertices[n - 2] };
            let next_vertex = &vertices[i + 1];

            if current.has_fradius() {
                debug_println!("    处理FRADIUS顶点[{}]: ({:.2}, {:.2}) FRADIUS: {:.1}mm",
                    i, current.x(), current.y(), current.get_fradius());

                // 处理FRADIUS，生成切点
                let arc_vertices = self.fradius_processor.process_fradius_vertex(
                    prev_vertex, current, next_vertex
                )?;

                if arc_vertices.len() == 2 {
                    debug_println!("      生成切点: 起点({:.2}, {:.2}) → 终点({:.2}, {:.2})",
                        arc_vertices[0].x(), arc_vertices[0].y(),
                        arc_vertices[1].x(), arc_vertices[1].y());

                    // 添加起始切点（FRADIUS信息已经在process_fradius_vertex中正确设置）
                    processed_vertices.push(arc_vertices[0].clone());

                    // 添加结束切点
                    processed_vertices.push(arc_vertices[1].clone());
                } else {
                    // FRADIUS处理失败，使用原始顶点
                    processed_vertices.push(current.clone());
                }
            } else {
                // 非FRADIUS顶点，直接添加
                processed_vertices.push(current.clone());
            }
        }

        debug_println!("    标准FRADIUS处理完成: {} → {} 个顶点", n - 1, processed_vertices.len());
        Ok(processed_vertices)
    }

    /// AVEVA风格共线处理
    fn aveva_collinear_processing(&self, vertices: &[Vertex]) -> Result<Vec<Vertex>> {
        debug_println!("  开始AVEVA风格共线处理...");

        // 步骤1: 共线顶点裁剪
        let step1_result = self.collinear_trimming(vertices)?;
        debug_println!("    步骤1: 共线顶点裁剪 (输入: {} 个顶点)", vertices.len());

        // 步骤2: 共线顶点合并
        let step2_result = self.collinear_merging(&step1_result)?;
        debug_println!("    步骤2: 共线顶点合并 (输入: {} 个顶点)", step1_result.len());

        // 步骤3: 直线连接优化
        let step3_result = self.line_connection_optimization(&step2_result)?;
        debug_println!("    步骤3: 直线连接优化 (输入: {} 个顶点)", step2_result.len());

        debug_println!("  AVEVA共线处理完成: {} → {} 个顶点 (移除 {} 个)",
            vertices.len(), step3_result.len(), vertices.len() - step3_result.len());

    ///
    ///  
    /// 
    /// 
    ///
    ///
    ///
    ///
    ///
    ///
    ///
    ///
    ///

        // 输出最终顶点列表进行调试
        debug_println!("\n最终Profile顶点坐标:");
        for (i, vertex) in step3_result.iter().enumerate() {
            if vertex.has_fradius() {
                debug_println!("  [{:2}] ({:.2}, {:.2}, {:.2}, FRAD:{:.2})",
                    i, vertex.x(), vertex.y(), vertex.z(), vertex.get_fradius());
            } else {
                debug_println!("  [{:2}] ({:.2}, {:.2}, {:.2})",
                    i, vertex.x(), vertex.y(), vertex.z());
            }
        }

        Ok(step3_result)
    }

    /// 共线顶点裁剪 - 与Python版本保持一致
    fn collinear_trimming(&self, vertices: &[Vertex]) -> Result<Vec<Vertex>> {
        let mut result = Vec::new();
        let mut removed_count = 0;

        for (_i, vertex) in vertices.iter().enumerate() {
            // 检查是否与已有顶点重复
            let is_duplicate = result.iter().any(|v: &Vertex| v.is_near_2d(vertex, self.dedup_tolerance));

            if is_duplicate {
                debug_println!("    共线裁剪: 移除顶点 ({:.2}, {:.2})", vertex.x(), vertex.y());
                removed_count += 1;
            } else {
                result.push(vertex.clone());
            }
        }

        // Python版本的特殊逻辑：检查首尾顶点的连接性
        // 如果第一个顶点和最后一个顶点重合，或者存在无效的起始顶点，进行清理
        if result.len() > 2 {
            // 检查是否有首尾重复的情况
            let first = &result[0];
            let last = &result[result.len() - 1];

            // 如果首尾重合，移除最后一个
            if first.is_near_2d(last, self.tolerance) {
                let removed_vertex = result.pop().unwrap();
                debug_println!("    共线裁剪: 移除首尾重复顶点 ({:.2}, {:.2})", removed_vertex.x(), removed_vertex.y());
                removed_count += 1;
            }

            // 使用与Python版本相同的三点共线检测逻辑
            // 检查每个顶点是否与前后顶点共线，如果共线则移除
            let mut i = 0;
            while i < result.len() {
                let n = result.len();
                if n < 3 {
                    break;
                }

                let current = &result[i];
                let prev_vertex = if i > 0 { &result[i - 1] } else { &result[n - 1] };
                let next_vertex = if i < n - 1 { &result[i + 1] } else { &result[0] };

                // 使用严格的共线检测
                if self.is_collinear_strict(prev_vertex, current, next_vertex) {
                    let removed_vertex = result.remove(i);
                    debug_println!("    共线裁剪: 移除顶点 ({:.2}, {:.2})", removed_vertex.x(), removed_vertex.y());
                    removed_count += 1;
                    // 不增加i，因为移除了一个元素
                } else {
                    i += 1;
                }
            }
        }

        debug_println!("    共线裁剪完成，移除 {} 个顶点", removed_count);
        Ok(result)
    }

    /// 共线顶点合并
    fn collinear_merging(&self, vertices: &[Vertex]) -> Result<Vec<Vertex>> {
        if vertices.len() < 3 {
            debug_println!("    共线合并完成，合并 0 个顶点");
            return Ok(vertices.to_vec());
        }

        let mut result = Vec::new();
        let mut merged_count = 0;

        let mut i = 0;
        while i < vertices.len() {
            let current = &vertices[i];

            // 检查是否与前后顶点共线
            if i > 0 && i < vertices.len() - 1 {
                let prev = &vertices[i - 1];
                let next = &vertices[i + 1];

                if self.are_collinear(prev, current, next) && !current.has_fradius() {
                    // 跳过共线的中间顶点
                    merged_count += 1;
                    i += 1;
                    continue;
                }
            }

            result.push(current.clone());
            i += 1;
        }

        debug_println!("    共线合并完成，合并 {} 个顶点", merged_count);
        Ok(result)
    }

    /// 直线连接优化
    fn line_connection_optimization(&self, vertices: &[Vertex]) -> Result<Vec<Vertex>> {
        // 这里可以添加更多的优化逻辑
        // 目前只是简单返回输入
        debug_println!("    直线连接完成，优化 0 个顶点");
        Ok(vertices.to_vec())
    }

    /// 检查三个点是否共线
    fn are_collinear(&self, p1: &Vertex, p2: &Vertex, p3: &Vertex) -> bool {
        let v1 = DVec2::new(p2.x() - p1.x(), p2.y() - p1.y());
        let v2 = DVec2::new(p3.x() - p2.x(), p3.y() - p2.y());

        // 使用叉积检查共线性
        let cross_product = v1.cross(v2).abs();
        cross_product < self.collinear_tolerance

    }

    /// 在给定 α 缩放下重算 FRADIUS 结果
    fn process_with_fradius_scale(&self, ploop: &PLoop, alpha: f64) -> Result<Vec<Vertex>> {
        let mut scaled = ploop.clone();
        for v in &mut scaled.vertices {
            if let Some(r) = v.fradius {
                v.fradius = Some(r * alpha);
            }
        }
        self.process_fradius_vertices(&scaled)
    }

    /// 通过全局半径缩放（二分搜索）消除自相交；返回 (α, 顶点)；若失败返回 None
    fn resolve_self_intersections_with_global_scale(
        &self,
        ploop: &PLoop,
        mut low: f64,
        mut high: f64,
        iters: u32,
    ) -> Result<Option<(f64, Vec<Vertex>)>> {
        let mut best_alpha: Option<f64> = None;
        let mut best_vertices: Option<Vec<Vertex>> = None;

        // 先快速检查 α=1 是否已经无交叉
        let v1 = self.process_with_fradius_scale(ploop, 1.0)?;
        if crate::set2d::detect_self_intersections(&v1, 1e-6).is_empty() {
            return Ok(Some((1.0, v1)));
        }

        // 二分搜索最大可行 α
        let mut lo = low.max(0.0);
        let mut hi = high.min(1.0);
        let mut last_ok_alpha = None;
        let mut last_ok_vertices: Option<Vec<Vertex>> = None;

        for _ in 0..iters {
            let mid = (lo + hi) * 0.5;
            let v = self.process_with_fradius_scale(ploop, mid)?;
            let inters = crate::set2d::detect_self_intersections(&v, 1e-6);
            if inters.is_empty() {
                last_ok_alpha = Some(mid);
                last_ok_vertices = Some(v);
                lo = mid; // 尝试变大
            } else {
                hi = mid; // 变小
            }
            if (hi - lo) < 1e-4 { break; }
        }

        if let (Some(a), Some(v)) = (last_ok_alpha, last_ok_vertices) {
            best_alpha = Some(a);
            best_vertices = Some(v);
        }

        if let (Some(a), Some(v)) = (best_alpha, best_vertices) {
            Ok(Some((a, v)))
        } else {
            Ok(None)
        }
    }



    /// 严格的共线检测，用于裁剪 - 与Python版本保持一致
    fn is_collinear_strict(&self, p1: &Vertex, p2: &Vertex, p3: &Vertex) -> bool {
        // 计算向量
        let v1_x = p2.x() - p1.x();
        let v1_y = p2.y() - p1.y();
        let v2_x = p3.x() - p2.x();
        let v2_y = p3.y() - p2.y();

        // 计算叉积
        let cross_product = v1_x * v2_y - v1_y * v2_x;

        // 计算向量长度
        let len1 = (v1_x * v1_x + v1_y * v1_y).sqrt();
        let len2 = (v2_x * v2_x + v2_y * v2_y).sqrt();

        // 如果任一边长度为0，不认为共线（避免删除重要顶点）
        let tolerance = 1.0; // 与Python版本保持一致
        if len1 < tolerance || len2 < tolerance {
            return false;
        }

        // 使用更严格的容差进行共线检测
        let relative_area = cross_product.abs() / (len1 * len2);
        let strict_tolerance = 0.001; // 0.1% 的角度容差，与Python版本一致

        relative_area < strict_tolerance
    }

    /// 生成处理报告
    pub fn generate_profile_report(&self, ploop: &PLoop, processed_vertices: &[Vertex]) -> String {
        let mut report = String::new();

        report.push_str(&format!("PLOOP PROFILE REPORT: {}\n", ploop.name));
        report.push_str("================================================================\n");
        report.push_str(&format!("Height: {:.1}mm\n", ploop.height));
        report.push_str(&format!("Original vertices: {}\n", ploop.vertices.len()));
        report.push_str(&format!("Processed vertices: {}\n", processed_vertices.len()));
        report.push_str("\n");

        // FRADIUS信息
        let fradius_vertices: Vec<_> = ploop.vertices.iter().enumerate()
            .filter(|(_, v)| v.has_fradius())
            .collect();

        if !fradius_vertices.is_empty() {
            report.push_str(&format!("FRADIUS处理: {} 个圆角顶点\n", fradius_vertices.len()));
            for (_i, (idx, vertex)) in fradius_vertices.iter().enumerate() {
                report.push_str(&format!("  顶点[{}]: 圆角半径 {:.1}mm\n",
                    idx, vertex.get_fradius()));
            }
            report.push_str("\n");
        }

        // 边界信息
        if let Some((min_x, max_x, min_y, max_y)) = ploop.bounding_box() {
            report.push_str("几何边界信息:\n");
            report.push_str(&format!("  X范围: {:.2} ~ {:.2} mm (宽度: {:.2}mm)\n",
                min_x, max_x, max_x - min_x));
            report.push_str(&format!("  Y范围: {:.2} ~ {:.2} mm (高度: {:.2}mm)\n",
                min_y, max_y, max_y - min_y));
        }

        report
    }
}

impl Default for PLoopProcessor {
    fn default() -> Self {
        Self::new()
    }
}
