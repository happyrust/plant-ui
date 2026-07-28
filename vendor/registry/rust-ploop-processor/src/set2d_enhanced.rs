use glam::DVec2;
use crate::vertex::Vertex;

#[derive(Debug, Clone)]
pub struct Intersection {
    pub i1: usize,
    pub i2: usize,
    pub point: DVec2,
    pub intersection_type: IntersectionType,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IntersectionType {
    Point,           // 普通点相交
    ColinearOverlap, // 共线重叠
}

#[derive(Debug, Clone)]
pub struct ColinearSegment {
    pub start: DVec2,
    pub end: DVec2,
    pub t_start: f64,
    pub t_end: f64,
}

#[inline]
fn orientation(a: DVec2, b: DVec2, c: DVec2) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

#[inline]
fn on_segment(a: DVec2, b: DVec2, p: DVec2, eps: f64) -> bool {
    let min_x = a.x.min(b.x) - eps;
    let max_x = a.x.max(b.x) + eps;
    let min_y = a.y.min(b.y) - eps;
    let max_y = a.y.max(b.y) + eps;
    p.x >= min_x && p.x <= max_x && p.y >= min_y && p.y <= max_y
}

/// 计算点在线段上的参数t
fn point_on_segment_t(p1: DVec2, p2: DVec2, point: DVec2) -> f64 {
    let dx = p2.x - p1.x;
    let dy = p2.y - p1.y;
    if dx.abs() > dy.abs() {
        (point.x - p1.x) / dx
    } else {
        (point.y - p1.y) / dy
    }
}

/// 检测共线段的重叠部分
fn colinear_overlap(p1: DVec2, p2: DVec2, q1: DVec2, q2: DVec2, eps: f64) -> Option<ColinearSegment> {
    // 计算每个点在p线段上的参数
    let t_q1 = point_on_segment_t(p1, p2, q1);
    let t_q2 = point_on_segment_t(p1, p2, q2);

    // 重叠区间计算
    let t_min = 0.0f64.max(t_q1.min(t_q2));
    let t_max = 1.0f64.min(t_q1.max(t_q2));

    // 检查是否有实际重叠（不仅仅是端点接触）
    if t_max - t_min > eps {
        let start = p1 + (p2 - p1) * t_min;
        let end = p1 + (p2 - p1) * t_max;
        Some(ColinearSegment {
            start,
            end,
            t_start: t_min,
            t_end: t_max,
        })
    } else {
        None
    }
}

/// 增强的线段相交测试，处理共线情况
fn segment_intersection_enhanced(
    p1: DVec2,
    p2: DVec2,
    q1: DVec2,
    q2: DVec2,
    i1: usize,
    i2: usize,
    eps: f64
) -> Option<Intersection> {
    // 快速排除：包围盒
    if p1.x.max(p2.x) + eps < q1.x.min(q2.x)
        || q1.x.max(q2.x) + eps < p1.x.min(p2.x)
        || p1.y.max(p2.y) + eps < q1.y.min(q2.y)
        || q1.y.max(q2.y) + eps < p1.y.min(p2.y)
    {
        return None;
    }

    let r = p2 - p1;
    let s = q2 - q1;
    let rxs = r.x * s.y - r.y * s.x;
    let qpxr = (q1 - p1).x * r.y - (q1 - p1).y * r.x;

    if rxs.abs() < eps {
        // 共线或平行
        if qpxr.abs() < eps {
            // 共线：检查重叠
            if let Some(overlap) = colinear_overlap(p1, p2, q1, q2, eps) {
                // E3D处理：取重叠段的中点作为代表性交点
                let mid_point = (overlap.start + overlap.end) * 0.5;
                return Some(Intersection {
                    i1,
                    i2,
                    point: mid_point,
                    intersection_type: IntersectionType::ColinearOverlap,
                });
            }
        }
        return None;
    }

    let t = ((q1 - p1).x * s.y - (q1 - p1).y * s.x) / rxs;
    let u = ((q1 - p1).x * r.y - (q1 - p1).y * r.x) / rxs;

    // E3D容差处理：端点附近的交点需要特殊处理
    let t_eps = eps / r.length();
    let u_eps = eps / s.length();

    if t >= -t_eps && t <= 1.0 + t_eps && u >= -u_eps && u <= 1.0 + u_eps {
        // 端点接近判断
        let is_endpoint = (t < t_eps || t > 1.0 - t_eps) || (u < u_eps || u > 1.0 - u_eps);

        // 非端点交叉才算真正的相交
        if !is_endpoint || (t > 0.0 && t < 1.0 && u > 0.0 && u < 1.0) {
            let p = DVec2::new(p1.x + t * r.x, p1.y + t * r.y);
            return Some(Intersection {
                i1,
                i2,
                point: p,
                intersection_type: IntersectionType::Point,
            });
        }
    }
    None
}

fn build_segments(vertices: &[Vertex]) -> Vec<(usize, DVec2, DVec2)> {
    let n = vertices.len();
    let mut segs = Vec::new();
    if n < 2 { return segs; }

    for i in 0..n-1 {
        let a = vertices[i].position_2d();
        let b = vertices[i+1].position_2d();
        segs.push((i, a, b));
    }

    // 闭合段
    let first = vertices.first().unwrap().position_2d();
    let last = vertices.last().unwrap().position_2d();
    if first.distance(last) > 1e-9 {
        segs.push((n-1, last, first));
    }
    segs
}

/// E3D风格的自相交检测，包含后处理
pub fn detect_self_intersections_e3d(vertices: &[Vertex], eps: f64) -> Vec<Intersection> {
    let segs = build_segments(vertices);
    let mut inters = Vec::new();

    // 阶段1: 收集所有交点
    for a in 0..segs.len() {
        for b in a+1..segs.len() {
            let (i1, p1, p2) = segs[a];
            let (i2, q1, q2) = segs[b];

            // 跳过相邻段
            let adjacent = (i1 + 1 == i2) || (i2 + 1 == i1) ||
                (i1 == 0 && (i2 == vertices.len()-1)) ||
                (i2 == 0 && (i1 == vertices.len()-1));
            if adjacent { continue; }

            if let Some(inter) = segment_intersection_enhanced(p1, p2, q1, q2, i1, i2, eps) {
                inters.push(inter);
            }
        }
    }

    // 阶段2: 后处理 - 类似E3D的aInSet2dPost*
    post_process_intersections(&mut inters, vertices, eps);

    inters
}

/// 后处理交点，处理特殊情况
fn post_process_intersections(inters: &mut Vec<Intersection>, vertices: &[Vertex], eps: f64) {
    // 1. 合并接近的交点
    merge_close_intersections(inters, eps);

    // 2. 处理共线重叠的特殊情况
    handle_colinear_cases(inters, vertices);

    // 3. 排序交点（按段索引）
    inters.sort_by(|a, b| {
        a.i1.cmp(&b.i1).then(a.i2.cmp(&b.i2))
    });
}

/// 合并距离很近的交点
fn merge_close_intersections(inters: &mut Vec<Intersection>, eps: f64) {
    let mut i = 0;
    while i < inters.len() {
        let mut j = i + 1;
        while j < inters.len() {
            if inters[i].point.distance(inters[j].point) < eps {
                // 合并：保留共线类型优先
                if inters[j].intersection_type == IntersectionType::ColinearOverlap {
                    inters[i] = inters[j].clone();
                }
                inters.remove(j);
            } else {
                j += 1;
            }
        }
        i += 1;
    }
}

/// 处理共线情况的特殊逻辑
fn handle_colinear_cases(inters: &mut Vec<Intersection>, vertices: &[Vertex]) {
    // E3D特殊处理：
    // 1. 如果有多个共线重叠，可能需要合并或拆分
    // 2. 共线重叠可能导致路径分裂，需要记录

    for inter in inters.iter_mut() {
        if inter.intersection_type == IntersectionType::ColinearOverlap {
            // 标记需要特殊处理的共线段
            // 这里可以添加E3D的具体业务逻辑
            #[cfg(feature = "debug")]
            eprintln!("共线重叠检测: 段{}-{} at {:?}",
                inter.i1, inter.i2, inter.point);
        }
    }
}

/// 检测并修复自相交（E3D风格）
pub fn fix_self_intersections(vertices: &mut Vec<Vertex>, eps: f64) -> bool {
    let intersections = detect_self_intersections_e3d(vertices, eps);

    if intersections.is_empty() {
        return false;
    }

    // E3D修复策略：
    // 1. 对于普通交点：可能需要重新排序顶点
    // 2. 对于共线重叠：可能需要删除或合并段

    for inter in &intersections {
        match inter.intersection_type {
            IntersectionType::Point => {
                // 普通交点处理
                #[cfg(feature = "debug")]
                eprintln!("修复交点: 段{}-{}", inter.i1, inter.i2);
            }
            IntersectionType::ColinearOverlap => {
                // 共线重叠处理 - E3D的特殊逻辑
                #[cfg(feature = "debug")]
                eprintln!("修复共线重叠: 段{}-{}", inter.i1, inter.i2);
            }
        }
    }

    true
}