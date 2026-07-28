use glam::DVec2;
use crate::vertex::Vertex;

#[derive(Debug, Clone)]
pub struct Intersection {
    pub i1: usize, // 段1起点索引
    pub i2: usize, // 段2起点索引
    pub point: DVec2,
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

/// 线段相交测试，返回交点（近似）
fn segment_intersection(p1: DVec2, p2: DVec2, q1: DVec2, q2: DVec2, eps: f64) -> Option<DVec2> {
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
            // 共线：检查投影重叠，取中点近似
            // 这里不返回共线重叠，以免把相邻共线段当作交叉
            return None;
        }
        return None;
    }

    let t = ((q1 - p1).x * s.y - (q1 - p1).y * s.x) / rxs;
    let u = ((q1 - p1).x * r.y - (q1 - p1).y * r.x) / rxs;

    if t >= -eps && t <= 1.0 + eps && u >= -eps && u <= 1.0 + eps {
        let p = DVec2::new(p1.x + t * r.x, p1.y + t * r.y);
        // 端点重合按容差处理
        if on_segment(p1, p2, p, eps) && on_segment(q1, q2, p, eps) {
            return Some(p);
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

/// 检测自相交，返回所有交点
pub fn detect_self_intersections(vertices: &[Vertex], eps: f64) -> Vec<Intersection> {
    let segs = build_segments(vertices);
    let mut inters = Vec::new();
    for a in 0..segs.len() {
        for b in a+1..segs.len() {
            let (i1, p1, p2) = segs[a];
            let (i2, q1, q2) = segs[b];
            // 跳过相邻段（共享端点）
            let adjacent = (i1 + 1 == i2) || (i2 + 1 == i1) ||
                (i1 == 0 && (i2 == vertices.len()-1)) || (i2 == 0 && (i1 == vertices.len()-1));
            if adjacent { continue; }
            if let Some(pt) = segment_intersection(p1, p2, q1, q2, eps) {
                inters.push(Intersection { i1, i2, point: pt });
            }
        }
    }
    inters
}

