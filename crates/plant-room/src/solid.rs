//! 房间体，以及「一根线段有哪几截落在房间体内」（plant-ui ADR-0028 / ADR-0029）。
//!
//! 一间房由它全部在册面板的棱柱合成。这里**不做布尔并**，而是每块面板各判各的，得到的区间再
//! 取并集。这与 gen-model 归属计算「每块面板各自出一条边」是同一个口径。
//!
//! 区间的求法：沿线段找出它与每块面板三角网的全部穿越点（f64 Möller–Trumbore，与点包含同一套
//! 求交），用穿越点把 [0, 1] 切成几段，每段在自己的中点问一次 [`point_inside_panel`]。穿越点只
//! 决定在哪儿切，内外只听点包含的判定。公共边上的重复穿越、擦过一条棱这类巧合，最多多切出一段
//! 零长度，不会把内外判反。
//!
//! 求穿越点不走 BVH 的射线遍历。管轴多半沿坐标轴走，而棱柱的墙面也沿坐标轴立着；轴向射线打在
//! 零厚度包围盒的边界上，就是浮点里 `0 × ∞` 的那种边角。所以这里改为对每片三角先做一次 f64
//! 包围盒粗筛，再求交，不做除法也就没有这个边角。点包含那三条射线是歪斜的，不受这个影响。

use glam::DVec3;
use parry3d::bounding_volume::{Aabb, BoundingVolume};
use parry3d::math::{Point, Real};
use parry3d::shape::TriMesh;

use crate::predicate::point_inside_panel;

/// 保留段的下限、截点并到管端的门槛、两段之间补缝的门槛，用的都是这一个数（mm）。
///
/// 取 e3d-model 的最小管长（`ZERO_LENGTH_TOL_MM = 1.0`，恰好 1 mm 的管有效）：短于它的一截在
/// E3D 里本来就不成管。
pub const MIN_PIECE_MM: f64 = 1.0;

/// 穿越点离线段两端多近就当作落在端点上（线段参数），用来挡掉数值上「恰好在墙上」的管端。
const END_EPS: f64 = 1e-9;
/// 三角包围盒粗筛的外扩量（mm）。只是为了不让恰好擦边的三角被浮点误差筛掉。
const BOX_EPS_MM: f64 = 1e-6;

struct Panel {
    mesh: TriMesh,
    aabb: Aabb,
    /// 同一张三角网的 f64 副本，求穿越点时用。
    triangles: Vec<[DVec3; 3]>,
}

impl Panel {
    fn new(mesh: TriMesh) -> Self {
        let aabb = *mesh.local_aabb();
        let vertices = mesh.vertices();
        let to_d = |index: u32| {
            let p = vertices[index as usize];
            DVec3::new(p.x as f64, p.y as f64, p.z as f64)
        };
        let triangles = mesh
            .indices()
            .iter()
            .map(|[a, b, c]| [to_d(*a), to_d(*b), to_d(*c)])
            .collect();
        Self {
            mesh,
            aabb,
            triangles,
        }
    }
}

/// 一间房的房间体：全部在册面板的判定三角网（世界系，毫米）。
pub struct RoomSolid {
    panels: Vec<Panel>,
    bounds: Aabb,
}

impl RoomSolid {
    /// 把一间房的面板三角网（[`crate::panel_tri_mesh`] 的产物，已在世界系）合成房间体。
    /// 一块都没有时为 `None`：没有墙就谈不上房间体。
    pub fn new(panels: impl IntoIterator<Item = TriMesh>) -> Option<Self> {
        let panels: Vec<Panel> = panels.into_iter().map(Panel::new).collect();
        let first = panels.first()?.aabb;
        let bounds = panels
            .iter()
            .skip(1)
            .fold(first, |bounds, panel| bounds.merged(&panel.aabb));
        Some(Self { panels, bounds })
    }

    /// 房间体的世界包围盒（全部面板包围盒的并）。
    pub fn bounds(&self) -> Aabb {
        self.bounds
    }

    pub fn panel_count(&self) -> usize {
        self.panels.len()
    }

    /// 这一点落不落在任何一块面板内。
    pub fn contains_point(&self, point: DVec3) -> bool {
        self.panels.iter().any(|panel| {
            box_overlaps(&panel.aabb, point, point) && point_inside_panel(&panel.mesh, &to_point(point))
        })
    }

    /// 线段 `a → b` 落在房间体内的那几截，用线段参数 `[t0, t1]` 表示（0 = `a`，1 = `b`），
    /// 升序且互不相接。
    ///
    /// 按 [`MIN_PIECE_MM`] 收尾：短于它的截段丢掉；离端点不到它的截点并到端点上；两截之间的缝
    /// 小于它就连起来（两块面板共面相接处因此不会断开）。空表表示整段都在房间体外，`[[0, 1]]`
    /// 表示整段在内。
    pub fn inside_intervals(&self, a: DVec3, b: DVec3) -> Vec<[f64; 2]> {
        if !a.is_finite() || !b.is_finite() {
            return Vec::new();
        }
        let delta = b - a;
        let length = delta.length();
        if length <= f64::EPSILON {
            return if self.contains_point(a) {
                vec![[0.0, 1.0]]
            } else {
                Vec::new()
            };
        }
        let direction = delta / length;
        let (lo, hi) = (a.min(b), a.max(b));
        let mut spans = Vec::new();
        for panel in &self.panels {
            if !box_overlaps(&panel.aabb, lo, hi) {
                continue;
            }
            let mut cuts = vec![0.0];
            cuts.extend(crossings(&panel.triangles, a, direction, length, lo, hi));
            cuts.push(1.0);
            for window in cuts.windows(2) {
                let (t0, t1) = (window[0], window[1]);
                if t1 <= t0 {
                    continue;
                }
                let middle = a + delta * ((t0 + t1) * 0.5);
                if point_inside_panel(&panel.mesh, &to_point(middle)) {
                    spans.push([t0, t1]);
                }
            }
        }
        normalize(spans, MIN_PIECE_MM / length)
    }
}

fn to_point(p: DVec3) -> Point<Real> {
    Point::new(p.x as f32, p.y as f32, p.z as f32)
}

/// f32 包围盒与 f64 的 `[lo, hi]` 是否相交（闭区间，外扩 [`BOX_EPS_MM`]）。
fn box_overlaps(aabb: &Aabb, lo: DVec3, hi: DVec3) -> bool {
    let (mins, maxs) = (aabb.mins, aabb.maxs);
    mins.x as f64 <= hi.x + BOX_EPS_MM
        && mins.y as f64 <= hi.y + BOX_EPS_MM
        && mins.z as f64 <= hi.z + BOX_EPS_MM
        && maxs.x as f64 >= lo.x - BOX_EPS_MM
        && maxs.y as f64 >= lo.y - BOX_EPS_MM
        && maxs.z as f64 >= lo.z - BOX_EPS_MM
}

/// 线段与一块面板全部三角的穿越参数，落在 `(0, 1)` 内部、升序去重。
fn crossings(
    triangles: &[[DVec3; 3]],
    origin: DVec3,
    direction: DVec3,
    length: f64,
    lo: DVec3,
    hi: DVec3,
) -> Vec<f64> {
    let lo = lo - DVec3::splat(BOX_EPS_MM);
    let hi = hi + DVec3::splat(BOX_EPS_MM);
    let mut ts = Vec::new();
    for triangle in triangles {
        let [a, b, c] = *triangle;
        let (tmin, tmax) = (a.min(b).min(c), a.max(b).max(c));
        if tmax.x < lo.x
            || tmax.y < lo.y
            || tmax.z < lo.z
            || tmin.x > hi.x
            || tmin.y > hi.y
            || tmin.z > hi.z
        {
            continue;
        }
        if let Some(distance) = ray_hit_distance(origin, direction, *triangle) {
            let t = distance / length;
            if t > END_EPS && t < 1.0 - END_EPS {
                ts.push(t);
            }
        }
    }
    ts.sort_by(f64::total_cmp);
    ts.dedup_by(|later, earlier| (*later - *earlier).abs() <= END_EPS);
    ts
}

/// 求穿越点时重心坐标往外放的量。
///
/// 管轴恰好打在两片三角的公共边上（例如沿 x 的管在 y = z = 500 处穿过 1000 见方墙面的对角线）
/// 是常态而不是巧合。严格闭区间在浮点下可能两片都判「差一点」，这一刀就丢了。放宽一点，最坏是
/// 同一处被两片各报一次，去重之后还是一刀。穿越点只决定在哪儿切，不决定内外，多报不会报错。
const EDGE_EPS: f64 = 1e-9;

/// Möller–Trumbore（f64），`direction` 为单位向量，交出沿射线的距离（mm）。
///
/// 行列式判据与 [`crate::predicate`] 里点包含用的那一份相同（相对边长为零不计），重心坐标的
/// 边界按 [`EDGE_EPS`] 放宽，并多交出距离。
fn ray_hit_distance(origin: DVec3, direction: DVec3, [a, b, c]: [DVec3; 3]) -> Option<f64> {
    let e1 = b - a;
    let e2 = c - a;
    let h = direction.cross(e2);
    let det = e1.dot(h);
    let scale = e1.length() * e2.length();
    if !(scale > 0.0) || det.abs() <= 1e-12 * scale {
        return None;
    }
    let inv = 1.0 / det;
    let s = origin - a;
    let u = inv * s.dot(h);
    if !(-EDGE_EPS..=1.0 + EDGE_EPS).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = inv * direction.dot(q);
    if v < -EDGE_EPS || u + v > 1.0 + EDGE_EPS {
        return None;
    }
    let distance = inv * e2.dot(q);
    (distance > 0.0).then_some(distance)
}

/// 各面板的截段取并集，再按 `tol`（线段参数，即 [`MIN_PIECE_MM`] 折算）收尾。
fn normalize(mut spans: Vec<[f64; 2]>, tol: f64) -> Vec<[f64; 2]> {
    spans.sort_by(|x, y| x[0].total_cmp(&y[0]));
    let mut merged: Vec<[f64; 2]> = Vec::with_capacity(spans.len());
    for span in spans {
        match merged.last_mut() {
            Some(last) if span[0] <= last[1] + tol => last[1] = last[1].max(span[1]),
            _ => merged.push(span),
        }
    }
    for span in &mut merged {
        if span[0] <= tol {
            span[0] = 0.0;
        }
        if span[1] >= 1.0 - tol {
            span[1] = 1.0;
        }
    }
    merged.retain(|span| span[1] - span[0] >= tol);
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{box_mesh, prism};
    use crate::panel_tri_mesh;
    use glam::{Mat4, Vec3};

    fn solid_of(meshes: &[(Vec<Vec3>, Vec<u32>)]) -> RoomSolid {
        RoomSolid::new(meshes.iter().map(|(vertices, indices)| {
            panel_tri_mesh(vertices, indices, Mat4::IDENTITY).expect("panel")
        }))
        .expect("room")
    }

    fn cube_room() -> RoomSolid {
        solid_of(&[box_mesh(Vec3::ZERO, Vec3::splat(1000.0))])
    }

    fn assert_spans(actual: &[[f64; 2]], expected: &[[f64; 2]]) {
        assert_eq!(actual.len(), expected.len(), "{actual:?} vs {expected:?}");
        for (got, want) in actual.iter().zip(expected) {
            assert!(
                (got[0] - want[0]).abs() < 1e-6 && (got[1] - want[1]).abs() < 1e-6,
                "{actual:?} vs {expected:?}"
            );
        }
    }

    fn p(x: f64, y: f64, z: f64) -> DVec3 {
        DVec3::new(x, y, z)
    }

    #[test]
    fn no_panels_no_room() {
        assert!(RoomSolid::new(Vec::<TriMesh>::new()).is_none());
    }

    #[test]
    fn a_segment_inside_the_room_is_kept_whole() {
        let room = cube_room();
        assert_spans(
            &room.inside_intervals(p(100.0, 500.0, 500.0), p(900.0, 500.0, 500.0)),
            &[[0.0, 1.0]],
        );
    }

    #[test]
    fn a_segment_outside_the_room_is_dropped() {
        let room = cube_room();
        assert!(room
            .inside_intervals(p(1500.0, 500.0, 500.0), p(2500.0, 500.0, 500.0))
            .is_empty());
        // 包围盒相交、却从房间体旁边擦过去的也一样。
        assert!(room
            .inside_intervals(p(-500.0, -10.0, 500.0), p(1500.0, -10.0, 500.0))
            .is_empty());
    }

    #[test]
    fn crossing_one_wall_keeps_the_inner_part() {
        let room = cube_room();
        assert_spans(
            &room.inside_intervals(p(500.0, 500.0, 500.0), p(1500.0, 500.0, 500.0)),
            &[[0.0, 0.5]],
        );
        // 反着走，同一截。
        assert_spans(
            &room.inside_intervals(p(1500.0, 500.0, 500.0), p(500.0, 500.0, 500.0)),
            &[[0.5, 1.0]],
        );
    }

    #[test]
    fn crossing_both_walls_keeps_the_middle() {
        let room = cube_room();
        assert_spans(
            &room.inside_intervals(p(-500.0, 500.0, 500.0), p(1500.0, 500.0, 500.0)),
            &[[0.25, 0.75]],
        );
        // 竖管穿楼板与顶板。
        assert_spans(
            &room.inside_intervals(p(500.0, 500.0, -1000.0), p(500.0, 500.0, 3000.0)),
            &[[0.25, 0.5]],
        );
    }

    #[test]
    fn an_oblique_crossing_cuts_at_the_wall() {
        let room = cube_room();
        // 从 (500, 500) 斜着走到 (1500, 1500)：x、y 同时在 t = 0.5 出墙（恰好过棱）。
        assert_spans(
            &room.inside_intervals(p(500.0, 500.0, 500.0), p(1500.0, 1500.0, 500.0)),
            &[[0.0, 0.5]],
        );
        // 只穿 x = 1000 那面墙：出墙点 t = 0.625。
        assert_spans(
            &room.inside_intervals(p(500.0, 200.0, 500.0), p(1300.0, 600.0, 500.0)),
            &[[0.0, 0.625]],
        );
    }

    /// L 形凹房：管子穿出缺角再穿回来，得两截。
    #[test]
    fn leaving_and_re_entering_a_concave_room_gives_two_pieces() {
        let l_shape = prism(
            &[
                [0.0, 0.0],
                [2000.0, 0.0],
                [2000.0, 1000.0],
                [1000.0, 1000.0],
                [1000.0, 2000.0],
                [0.0, 2000.0],
            ],
            0.0,
            1000.0,
        );
        let room = solid_of(&[l_shape]);
        // (1800, 500) → (500, 1800)：t ∈ (500/1300, 800/1300) 落在缺角里。
        assert_spans(
            &room.inside_intervals(p(1800.0, 500.0, 500.0), p(500.0, 1800.0, 500.0)),
            &[[0.0, 500.0 / 1300.0], [800.0 / 1300.0, 1.0]],
        );
    }

    /// 两块面板面对面相接：穿过公共面的管子不断开。
    #[test]
    fn two_panels_meeting_face_to_face_do_not_split_a_pipe() {
        let room = solid_of(&[
            box_mesh(Vec3::ZERO, Vec3::splat(1000.0)),
            box_mesh(Vec3::new(1000.0, 0.0, 0.0), Vec3::new(2000.0, 1000.0, 1000.0)),
        ]);
        assert_spans(
            &room.inside_intervals(p(500.0, 500.0, 500.0), p(1500.0, 500.0, 500.0)),
            &[[0.0, 1.0]],
        );
        assert_spans(
            &room.inside_intervals(p(-1000.0, 500.0, 500.0), p(3000.0, 500.0, 500.0)),
            &[[0.25, 0.75]],
        );
    }

    #[test]
    fn a_pipe_ending_on_the_wall_is_whole() {
        let room = cube_room();
        assert_spans(
            &room.inside_intervals(p(500.0, 500.0, 500.0), p(1000.0, 500.0, 500.0)),
            &[[0.0, 1.0]],
        );
        // 从墙外正好走到墙上：整段在外。
        assert!(room
            .inside_intervals(p(1500.0, 500.0, 500.0), p(1000.0, 500.0, 500.0))
            .is_empty());
    }

    /// 擦墙只看轴线：轴线在墙内 0.5 mm 整段留，在墙外 0.5 mm 整段不画。
    #[test]
    fn grazing_parallel_to_a_wall_follows_the_axis() {
        let room = cube_room();
        assert_spans(
            &room.inside_intervals(p(100.0, 999.5, 500.0), p(900.0, 999.5, 500.0)),
            &[[0.0, 1.0]],
        );
        assert!(room
            .inside_intervals(p(100.0, 1000.5, 500.0), p(900.0, 1000.5, 500.0))
            .is_empty());
    }

    /// 轴线正好躺在墙面里：内外说不清，但结论只能是整段在内或整段在外，不许切碎。
    #[test]
    fn a_pipe_lying_in_a_wall_plane_is_not_fragmented() {
        let room = cube_room();
        let spans = room.inside_intervals(p(100.0, 1000.0, 500.0), p(900.0, 1000.0, 500.0));
        assert!(
            spans.is_empty() || spans == vec![[0.0, 1.0]],
            "{spans:?}"
        );
    }

    #[test]
    fn slivers_shorter_than_a_millimetre_are_dropped_and_near_ends_snap() {
        let room = cube_room();
        // 管端伸出墙外 0.5 mm：并到管端，整段在内。
        assert_spans(
            &room.inside_intervals(p(500.0, 500.0, 500.0), p(1000.5, 500.0, 500.0)),
            &[[0.0, 1.0]],
        );
        // 只探进墙里 0.5 mm：那一截丢掉。
        assert!(room
            .inside_intervals(p(-1000.0, 500.0, 500.0), p(0.5, 500.0, 500.0))
            .is_empty());
    }

    #[test]
    fn a_degenerate_sliver_does_not_change_the_answer() {
        let (mut vertices, mut indices) = box_mesh(Vec3::ZERO, Vec3::splat(1000.0));
        let base = vertices.len() as u32;
        vertices.push(Vec3::new(0.0, 500.0, 1000.0));
        vertices.push(Vec3::new(500.0, 500.0, 1000.0));
        vertices.push(Vec3::new(1000.0, 500.0, 1000.0));
        indices.extend([base, base + 1, base + 2]);
        let room = solid_of(&[(vertices, indices)]);
        assert_spans(
            &room.inside_intervals(p(500.0, 500.0, 500.0), p(500.0, 500.0, 1500.0)),
            &[[0.0, 0.5]],
        );
    }

    #[test]
    fn a_zero_length_segment_is_a_point_test() {
        let room = cube_room();
        assert_spans(
            &room.inside_intervals(p(500.0, 500.0, 500.0), p(500.0, 500.0, 500.0)),
            &[[0.0, 1.0]],
        );
        assert!(room
            .inside_intervals(p(5000.0, 500.0, 500.0), p(5000.0, 500.0, 500.0))
            .is_empty());
        assert!(room
            .inside_intervals(p(f64::NAN, 0.0, 0.0), p(1.0, 0.0, 0.0))
            .is_empty());
    }

    #[test]
    fn bounds_cover_every_panel() {
        let room = solid_of(&[
            box_mesh(Vec3::ZERO, Vec3::splat(1000.0)),
            box_mesh(Vec3::new(1000.0, 0.0, 0.0), Vec3::new(2000.0, 1000.0, 3000.0)),
        ]);
        let bounds = room.bounds();
        assert_eq!([bounds.mins.x, bounds.mins.y, bounds.mins.z], [0.0, 0.0, 0.0]);
        assert_eq!(
            [bounds.maxs.x, bounds.maxs.y, bounds.maxs.z],
            [2000.0, 1000.0, 3000.0]
        );
        assert_eq!(room.panel_count(), 2);
    }
}
