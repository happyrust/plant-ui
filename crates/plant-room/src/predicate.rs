//! 元素是否落在房间面板内：正反两个方向共用的唯一判定口径（gen-model ADR-010 §3）。
//!
//! 搬自 gen-model `src/fast_model/room_predicate.rs`（plant-ui ADR-0029）。那边模块头的来龙去脉摘要如下：
//!
//! 在此之前有两套不一致的规则。正向 `cal_room_refnos` 用「AABB 八顶点全在内，否则取实际几何点
//! 逐点兜底」，反向 `query_room_panel_by_point` 只测单点、命中即返回。同一个横跨两室的构件，两边
//! 会给出不同答案。本模块把正向那套口径固化成唯一实现，两个方向都只调它。
//!
//! **点包含怎么判（2026-09-18 起）**：沿三条歪斜方向各数一次射线与面板三角网的穿越次数，奇数为内，
//! 三条里有两条算内才算内。此前走 parry 的 `TriMesh::contains_point`：点落在**退化三角**（零面积）
//! 的面 Voronoi 区时，会滑进为 2D 留的 `OnSolid` 分支被判成在内。房间棱柱的盖面是两百多个船体
//! 型线点的三角剖分，共线处就有零面积碎片，于是**盒外一米的点也能被判成成员**；而且哪些碎片在
//! f32 变换 / 焊接后恰好零面积，决定了结论，几何逐字相同的两次运行边集不同。射线奇偶对退化三角
//! 免疫（行列式为零直接跳过），面板网格又在 [`panel_tri_mesh`] 里先焊接、剔退化，闭合棱柱上这条
//! 判定是精确的。
//!
//! 搬迁只改了一处：[`panel_tri_mesh`] 的入参从 `&PlantMesh` 换成顶点 / 索引切片。其余照旧；要改
//! 口径得先改 gen-model 那边的决策，不能在这里悄悄改。

use std::collections::HashMap;

use glam::{DVec3, Mat4, Vec3};
use parry3d::bounding_volume::Aabb;
use parry3d::math::{Point, Real, Vector};
use parry3d::query::Ray;
use parry3d::query::visitors::RayIntersectionsVisitor;
use parry3d::shape::{TriMesh, TriMeshFlags};

/// 只看包围盒能得出的结论。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AabbVerdict {
    /// 八个顶点全部在面板内：直接算成员，不必再取几何点。
    Inside,
    /// 只有部分顶点在内：需要用实际几何点再判一次。
    NeedsPointCheck,
    /// 一个顶点都不在内。
    ///
    /// 已知盲区：横穿凹形房间的长管，八个角点可能全在房间外，却确实穿过房间。
    /// 保留这个行为是为了与全量路径逐字一致；要改口径得另立决策，不能在这里悄悄改。
    Outside,
}

/// 包围盒本身是否可用。NaN / Inf 的盒子在库里真实存在，必须先挡掉。
pub fn aabb_is_usable(aabb: &Aabb) -> bool {
    let magnitude = aabb.extents().magnitude();
    !magnitude.is_nan() && !magnitude.is_infinite()
}

/// 面板顶点焊接的格距（mm）。同一角点经不同算路得到的坐标只差末几位，按这个格距归成一个顶点，
/// 盖面与侧壁的缝才闭得上。房间以米计，0.01 mm 远小于任何几何意义。
pub const PANEL_WELD_STEP_MM: f32 = 0.01;

/// 退化三角的判据：两条边夹角的正弦小于它就算共线（零面积碎片）。
pub const PANEL_DEGENERATE_SIN: f64 = 1e-7;

/// 面板网格 → 判定用三角网。**全仓只有这一处定义**，所以同一块面板经哪条路建出来的网格都逐位相同。
///
/// 三步：
/// 1. 顶点按 [`PANEL_WELD_STEP_MM`] 焊接：首见者留原坐标，按三角遍历序编号，没被引用的顶点不进来；
/// 2. 剔掉顶点重合或三点共线（[`PANEL_DEGENERATE_SIN`]）的三角；
/// 3. 乘世界矩阵（f32，与构件取点同一条数值路径）建 BVH。
///
/// 不带 `ORIENTED`：奇偶判定不用伪法线，也就不再为半边拓扑错误付代价。剔完一个三角都不剩时返回 `None`。
/// `vertices` / `indices` 是面板网格局部系的原样（`indices` 每三个一片三角）。
pub fn panel_tri_mesh(vertices: &[Vec3], indices: &[u32], matrix: Mat4) -> Option<TriMesh> {
    if indices.len() < 3 || indices.len() % 3 != 0 {
        return None;
    }
    if indices.iter().any(|&i| (i as usize) >= vertices.len()) {
        return None;
    }
    let key_of = |p: &Vec3| -> [i64; 3] {
        [
            (p.x / PANEL_WELD_STEP_MM).round() as i64,
            (p.y / PANEL_WELD_STEP_MM).round() as i64,
            (p.z / PANEL_WELD_STEP_MM).round() as i64,
        ]
    };
    let mut welded: HashMap<[i64; 3], u32> = HashMap::new();
    let mut local: Vec<Vec3> = Vec::new();
    let mut triangles: Vec<[u32; 3]> = Vec::new();
    for tri in indices.chunks(3) {
        let corners = [
            vertices[tri[0] as usize],
            vertices[tri[1] as usize],
            vertices[tri[2] as usize],
        ];
        let [a, b, c] = corners.map(|p| DVec3::new(p.x as f64, p.y as f64, p.z as f64));
        let e1 = b - a;
        let e2 = c - a;
        let cross = e1.cross(e2).length();
        let scale = e1.length() * e2.length();
        if !(scale > 0.0) || cross <= PANEL_DEGENERATE_SIN * scale {
            continue;
        }
        let mut ids = [0u32; 3];
        for (slot, p) in corners.iter().enumerate() {
            let next = local.len() as u32;
            ids[slot] = *welded.entry(key_of(p)).or_insert_with(|| {
                local.push(*p);
                next
            });
        }
        if ids[0] == ids[1] || ids[1] == ids[2] || ids[0] == ids[2] {
            continue;
        }
        triangles.push(ids);
    }
    if triangles.is_empty() {
        return None;
    }
    let points: Vec<Point<Real>> = local
        .iter()
        .map(|p| {
            let world = matrix.transform_point3(*p);
            Point::new(world.x, world.y, world.z)
        })
        .collect();
    Some(TriMesh::with_flags(
        points,
        triangles,
        TriMeshFlags::MERGE_DUPLICATE_VERTICES,
    ))
}

/// 三条互不平行、避开坐标轴与常见 45° 面的方向；一次判定各投一票。
const PARITY_RAY_DIRECTIONS: [[f64; 3]; 3] = [
    [0.7730, 0.3319, 0.5411],
    [-0.2000, 0.9000, 0.3000],
    [0.1000, -0.4000, 0.8500],
];

/// 点是否在面板实体内：三条方向各数一次穿越，奇数为内，三取二。
///
/// 每条射线只对 BVH 挑出的候选三角做 f64 Möller–Trumbore；行列式相对边长为零（退化三角、
/// 射线与面平行）的一律不计。射线恰好穿过两片三角的公共边会被数两次，多数票就是为这一类
/// 巧合准备的。
pub fn point_inside_panel(panel: &TriMesh, point: &Point<Real>) -> bool {
    let votes = PARITY_RAY_DIRECTIONS
        .iter()
        .filter(|direction| crossings_are_odd(panel, point, DVec3::from_array(**direction)))
        .count();
    votes >= 2
}

fn crossings_are_odd(panel: &TriMesh, point: &Point<Real>, direction: DVec3) -> bool {
    let direction = direction.normalize();
    let origin = DVec3::new(point.x as f64, point.y as f64, point.z as f64);
    let ray = Ray::new(
        *point,
        Vector::new(direction.x as f32, direction.y as f32, direction.z as f32),
    );
    let mut crossings = 0usize;
    let mut on_candidate = |index: &u32| -> bool {
        let tri = panel.triangle(*index);
        let to_d = |p: Point<Real>| DVec3::new(p.x as f64, p.y as f64, p.z as f64);
        if ray_crosses_triangle(origin, direction, [to_d(tri.a), to_d(tri.b), to_d(tri.c)]) {
            crossings += 1;
        }
        true
    };
    let mut visitor = RayIntersectionsVisitor::new(&ray, Real::MAX, &mut on_candidate);
    panel.qbvh().traverse_depth_first(&mut visitor);
    crossings % 2 == 1
}

/// Möller–Trumbore（f64）。闭区间取边界：公共边上的重复计数交给多数票。
fn ray_crosses_triangle(origin: DVec3, direction: DVec3, [a, b, c]: [DVec3; 3]) -> bool {
    let e1 = b - a;
    let e2 = c - a;
    let h = direction.cross(e2);
    let det = e1.dot(h);
    let scale = e1.length() * e2.length();
    if !(scale > 0.0) || det.abs() <= 1e-12 * scale {
        return false;
    }
    let inv = 1.0 / det;
    let s = origin - a;
    let u = inv * s.dot(h);
    if !(0.0..=1.0).contains(&u) {
        return false;
    }
    let q = s.cross(e1);
    let v = inv * direction.dot(q);
    if v < 0.0 || u + v > 1.0 {
        return false;
    }
    inv * e2.dot(q) > 0.0
}

/// 八个顶点里有几个落在面板内（0–8）。
///
/// 这个计数同时兼作**归属强度**：一个件同时落在两间房时，包住它更多顶点的那间更像是它的房间。
/// `room_relate.inside_count` 存的就是它，供 `fn::room_code` 排序取首条（gen-model ADR-010 §5）。
pub fn count_vertices_inside(panel: &TriMesh, aabb: &Aabb) -> u8 {
    aabb.vertices()
        .iter()
        .filter(|v| point_inside_panel(panel, v))
        .count() as u8
}

/// 顶点计数到结论的映射。调用方若已经拿到计数（要用它排序），可以直接用这个，
/// 不必为了拿结论再跑一遍八次点包含。
pub fn verdict_of(inside_count: u8) -> AabbVerdict {
    match inside_count {
        8 => AabbVerdict::Inside,
        0 => AabbVerdict::Outside,
        _ => AabbVerdict::NeedsPointCheck,
    }
}

/// 第一轮：拿元素的世界包围盒对面板做八顶点测试。
pub fn classify_by_aabb(panel: &TriMesh, aabb: &Aabb) -> AabbVerdict {
    verdict_of(count_vertices_inside(panel, aabb))
}

/// 归属强度的次键：元素包围盒中心到面板包围盒中心的距离，越小越像「主房间」。
/// 与 `inside_count` 一样，在判定过程中顺手就能算出来，没有额外几何开销。
pub fn center_distance(panel_aabb: &Aabb, element_aabb: &Aabb) -> f32 {
    (panel_aabb.center() - element_aabb.center()).magnitude()
}

/// 第二轮：任一实际几何点落在面板内即算成员。点必须已经变换到世界坐标系。
pub fn any_point_inside(panel: &TriMesh, world_pts: impl IntoIterator<Item = Point<f32>>) -> bool {
    world_pts.into_iter().any(|p| point_inside_panel(panel, &p))
}

/// 第一轮的完整结果：包围盒结论 + 顶点计数，盒子不可用时为 `None`。
///
/// 计数与结论一起给出，是因为反向路径两样都要：计数同时是 `room_relate.inside_count` 的排序键。
/// 分两次问的话，同一块面板的八次点包含测试要跑两遍。
pub fn membership_by_aabb(panel: &TriMesh, aabb: &Aabb) -> Option<(AabbVerdict, u8)> {
    if !aabb_is_usable(aabb) {
        return None;
    }
    let inside = count_vertices_inside(panel, aabb);
    Some((verdict_of(inside), inside))
}

/// 两轮合一，供反向（元素 → 面板）使用。
///
/// `world_pts` 取惰性闭包：绝大多数元素在第一轮就有结论，不该为它们付取几何点的代价。
/// 队列路径先用 [`membership_by_aabb`] 把整批候选筛一遍，只有确实需要第二轮的才去库里取点，
/// 再回到这里；两条路走的是同一份规则，因为本函数就是由那个函数与 [`any_point_inside`] 组合出来的。
pub fn element_in_panel<F, I>(panel: &TriMesh, aabb: &Aabb, world_pts: F) -> bool
where
    F: FnOnce() -> I,
    I: IntoIterator<Item = Point<f32>>,
{
    match membership_by_aabb(panel, aabb) {
        None => false,
        Some((AabbVerdict::Inside, _)) => true,
        Some((AabbVerdict::Outside, _)) => false,
        Some((AabbVerdict::NeedsPointCheck, _)) => any_point_inside(panel, world_pts()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::box_mesh;

    fn panel(min: Vec3, max: Vec3) -> TriMesh {
        let (vertices, indices) = box_mesh(min, max);
        panel_tri_mesh(&vertices, &indices, Mat4::IDENTITY).expect("box -> trimesh")
    }

    fn aabb(min: Vec3, max: Vec3) -> Aabb {
        Aabb::new(
            Point::new(min.x, min.y, min.z),
            Point::new(max.x, max.y, max.z),
        )
    }

    #[test]
    fn fully_contained_box_short_circuits_on_aabb() {
        let room = panel(Vec3::ZERO, Vec3::splat(1000.0));
        let verdict = classify_by_aabb(&room, &aabb(Vec3::splat(400.0), Vec3::splat(600.0)));
        assert_eq!(verdict, AabbVerdict::Inside);
    }

    #[test]
    fn disjoint_box_is_outside() {
        let room = panel(Vec3::ZERO, Vec3::splat(1000.0));
        let verdict = classify_by_aabb(&room, &aabb(Vec3::splat(2000.0), Vec3::splat(2100.0)));
        assert_eq!(verdict, AabbVerdict::Outside);
    }

    /// 跨界构件必须走到第二轮，否则多归属判不出来。
    #[test]
    fn straddling_box_defers_to_point_check() {
        let room = panel(Vec3::ZERO, Vec3::splat(1000.0));
        let straddle = aabb(
            Vec3::new(900.0, 400.0, 400.0),
            Vec3::new(1100.0, 600.0, 600.0),
        );
        assert_eq!(
            classify_by_aabb(&room, &straddle),
            AabbVerdict::NeedsPointCheck
        );

        assert!(element_in_panel(&room, &straddle, || {
            vec![Point::new(950.0, 500.0, 500.0)]
        }));
        assert!(!element_in_panel(&room, &straddle, || {
            vec![Point::new(1050.0, 500.0, 500.0)]
        }));
    }

    #[test]
    fn unusable_aabb_is_rejected_before_any_geometry_work() {
        let room = panel(Vec3::ZERO, Vec3::splat(1000.0));
        let broken = aabb(Vec3::splat(f32::NAN), Vec3::splat(f32::NAN));
        assert!(!aabb_is_usable(&broken));
        assert!(!element_in_panel(&room, &broken, || {
            vec![Point::new(500.0, 500.0, 500.0)]
        }));
        assert_eq!(membership_by_aabb(&room, &broken), None);
    }

    /// 分两段问与一次问必须得出同一个结论。队列路径为了推迟取点走的是前者，
    /// 两者一旦分叉，跨界构件在直调路径与队列路径下就会拿到不同的房间。
    #[test]
    fn the_two_phase_form_agrees_with_the_single_shot_predicate() {
        let room = panel(Vec3::ZERO, Vec3::splat(1000.0));
        let inside_pt = || vec![Point::new(950.0, 500.0, 500.0)];
        let cases = [
            (
                aabb(Vec3::splat(400.0), Vec3::splat(600.0)),
                AabbVerdict::Inside,
                8,
            ),
            (
                aabb(Vec3::splat(2000.0), Vec3::splat(2100.0)),
                AabbVerdict::Outside,
                0,
            ),
            (
                aabb(
                    Vec3::new(900.0, 400.0, 400.0),
                    Vec3::new(1100.0, 600.0, 600.0),
                ),
                AabbVerdict::NeedsPointCheck,
                4,
            ),
        ];
        for (box_of, expected, inside_count) in cases {
            let (verdict, count) =
                membership_by_aabb(&room, &box_of).expect("盒子可用时必须有结论");
            assert_eq!(verdict, expected);
            assert_eq!(count, inside_count);
            let two_phase = match verdict {
                AabbVerdict::Inside => true,
                AabbVerdict::Outside => false,
                AabbVerdict::NeedsPointCheck => any_point_inside(&room, inside_pt()),
            };
            assert_eq!(two_phase, element_in_panel(&room, &box_of, inside_pt));
        }
    }

    /// 面板网格里掺一片零面积三角（盖面三角剖分在共线型线点处的常态）与一组只差末位的重复顶点：
    /// 建网时退化三角剔掉、近重复顶点焊成一个，奇偶判定不受影响，盒外的点仍是盒外。
    #[test]
    fn a_degenerate_cap_triangle_cannot_turn_an_outside_point_into_a_member() {
        let (mut vertices, mut indices) = box_mesh(Vec3::ZERO, Vec3::splat(1000.0));
        // 顶盖上一片共线三角：(0,0,1000)-(500,0,1000)-(1000,0,1000)
        let base = vertices.len() as u32;
        vertices.push(Vec3::new(0.0, 0.0, 1000.0));
        vertices.push(Vec3::new(500.0, 0.0, 1000.0));
        vertices.push(Vec3::new(1000.0, 0.0, 1000.0));
        indices.extend([base, base + 1, base + 2]);
        // +x 面的两片三角改用一份只差 1e-4 mm 的角点副本：不焊接就是一条开缝。
        let near = vertices.len() as u32;
        vertices.push(Vec3::new(1000.0001, 1000.0, 1000.0));
        for i in indices[30..36].iter_mut() {
            if *i == 6 {
                *i = near;
            }
        }
        let far_outside = Point::new(500.0, -3000.0, 900.0);

        let room = panel_tri_mesh(&vertices, &indices, Mat4::IDENTITY)
            .expect("box + sliver -> trimesh");
        assert_eq!(room.indices().len(), 12, "退化三角在建网时就剔掉");
        assert_eq!(room.vertices().len(), 8, "近重复角点焊成一个");
        assert!(!point_inside_panel(&room, &far_outside));
        assert!(point_inside_panel(&room, &Point::new(500.0, 500.0, 500.0)));
        assert_eq!(
            count_vertices_inside(&room, &aabb(Vec3::splat(400.0), Vec3::splat(600.0))),
            8
        );
    }

    /// 顶点焊接与剔退化只改拓扑不改坐标：首见顶点留原位，单位盒 8 个角点一个不多一个不少，
    /// 12 片三角照旧。
    #[test]
    fn welding_keeps_first_seen_coordinates_and_drops_nothing_from_a_clean_box() {
        let room = panel(Vec3::ZERO, Vec3::splat(1000.0));
        assert_eq!(room.vertices().len(), 8);
        assert_eq!(room.indices().len(), 12);
        let mut coords: Vec<[f32; 3]> = room.vertices().iter().map(|p| [p.x, p.y, p.z]).collect();
        coords.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(coords[0], [0.0, 0.0, 0.0]);
        assert_eq!(coords[7], [1000.0, 1000.0, 1000.0]);
    }

    /// 世界矩阵在焊接之后才乘：同一块面板挪到别处，判定跟着挪，焊接结果不变。
    #[test]
    fn the_world_matrix_moves_the_panel_after_welding() {
        let (vertices, indices) = box_mesh(Vec3::ZERO, Vec3::splat(1000.0));
        let moved = panel_tri_mesh(
            &vertices,
            &indices,
            Mat4::from_translation(Vec3::new(5000.0, 0.0, 0.0)),
        )
        .expect("moved box");
        assert_eq!(moved.vertices().len(), 8);
        assert!(point_inside_panel(&moved, &Point::new(5500.0, 500.0, 500.0)));
        assert!(!point_inside_panel(&moved, &Point::new(500.0, 500.0, 500.0)));
    }

    #[test]
    fn broken_index_buffers_build_nothing() {
        let (vertices, _) = box_mesh(Vec3::ZERO, Vec3::splat(1000.0));
        assert!(panel_tri_mesh(&vertices, &[0, 1], Mat4::IDENTITY).is_none());
        assert!(panel_tri_mesh(&vertices, &[0, 1, 99], Mat4::IDENTITY).is_none());
        // 三个点全重合：剔完一片不剩。
        assert!(panel_tri_mesh(&vertices, &[0, 0, 0], Mat4::IDENTITY).is_none());
    }
}
