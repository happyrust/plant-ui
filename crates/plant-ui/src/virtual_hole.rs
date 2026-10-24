//! Shared virtual-hole preview contract.
//! The importer and both rendering backends consume this module; it deliberately
//! contains no database or renderer dependencies.
//!
//! 虚拟孔洞（ADR-0031）：AMS 提资表里的开孔记录。`Position` 是孔洞中心，`SizeWidth`
//! 是圆孔直径 / 矩形宽，`SizeHeight` 是矩形高，`SizeThrowWall` 是切削体长度（沿孔洞
//! 局部 Z 轴，正负各一半）。只支持 CIR 和 RECT；只切除 `RelyItemREF` 映射到的实体；
//! 布尔结果只在内存里，不写回正式模型。

use glam::{DMat4, DVec3, Mat3, Vec3};
use serde::{Deserialize, Serialize};

use crate::RefU64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VirtualHoleShape { Circle, Rect }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VirtualHole {
    pub id: String,
    pub shape: VirtualHoleShape,
    pub center_mm: [f32; 3],
    pub orientation: [[f32; 3]; 3],
    pub diameter_mm: Option<f32>,
    pub width_mm: Option<f32>,
    pub height_mm: Option<f32>,
    pub depth_mm: f32,
    pub rely_item_ref: String,
    /// RelyItemREF 解析出的参考号（`24381_145018` / `24381/145018` / `=24381_145018`）。
    pub rely_refno: Option<RefU64>,
    pub item_ref: String,
    pub source_row: usize,
    pub hole_work: String,
    pub h_type: String,
    /// SubsMeterial / SubsMaterial 原文（两列名都认）。
    pub subs_material: String,
    pub raw_shape: String,
    pub raw_orientation: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VirtualHoleStatus {
    /// 已解析、已映射，还没进布尔。
    Ready,
    /// 布尔作业已下发，等结果。
    Calculating,
    /// 与关联实体没有实质相交。
    NoIntersection,
    /// 布尔完成，预览网格已替换。
    Changed,
    /// 失败（实体缺失 / 非流形 / 空结果），保留原网格。
    Failed,
}

impl VirtualHoleStatus {
    /// 汇总多个结果时的轻重：Failed > Changed > NoIntersection > Calculating > Ready。
    pub fn severity(self) -> u8 {
        match self {
            VirtualHoleStatus::Failed => 4,
            VirtualHoleStatus::Changed => 3,
            VirtualHoleStatus::NoIntersection => 2,
            VirtualHoleStatus::Calculating => 1,
            VirtualHoleStatus::Ready => 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VirtualHolePreviewResult {
    pub entity_id: String,
    pub hole_ids: Vec<String>,
    pub status: VirtualHoleStatus,
    pub error: Option<String>,
    pub elapsed_ms: u32,
}

/// 宿主交给视口的虚拟孔洞图层指令（ADR-0031 阶段二 / 阶段四）。
///
/// 与 `ModelAction` 分开：那张表里的每一项改的都是「三维此刻画成什么样」、
/// 撤销它发一条相反的命令就行；布尔预览那条链路（下发布尔作业、回执、
/// 原始网格恢复）绕不开视口内部的网格与实体状态，放不进那张表。
#[derive(Debug, Clone)]
pub enum VirtualHoleOp {
    /// 整层替换切削体（导入 / 重导入之后调用）。未映射的孔洞也画——
    /// 阶段二「未匹配实体的孔洞仍独立显示」。
    SetLayer { holes: Vec<VirtualHole> },
    /// 切削体选中高亮（页签行点击）；`None` 取消高亮。
    SelectHole(Option<String>),
    /// 一个映射目标的布尔预览：`holes` 是该目标下的全部孔洞（含不相交的，
    /// 视口先拿切削体 OBB 粗筛）。`epoch` 是宿主会话帧号，回执原样带回。
    Preview {
        epoch: u64,
        target: RefU64,
        holes: Vec<VirtualHole>,
    },
    /// 相机对准一条孔洞的切削体（宿主已经知道孔洞在哪，视口算包围盒取景）。
    FocusHole(String),
    /// 原始 / 布尔两态开关（只对已出结果的实体有意义）。
    SetShowOriginal(bool),
    /// 孔洞预览 X-Ray 开关（被开孔的实体半透明）。与房间 X-Ray 互斥，宿主负责关另一家。
    SetXRay(bool),
    /// 撤掉整层：恢复所有被替换的原始网格，撤掉切削体。孔洞列表（会话）不动。
    Clear,
}

/// View3d 回给宿主的一次预览回执（进程内传递，不序列化）。
#[derive(Debug, Clone, PartialEq)]
pub struct VirtualHoleReceipt {
    /// 预览请求的帧号；宿主拿它丢弃过期结果。
    pub epoch: u64,
    /// 关联实体的参考号。
    pub refno: RefU64,
    /// 参与这次布尔的孔洞 id（同一实体上全部相交的孔洞）。
    pub hole_ids: Vec<String>,
    pub status: VirtualHoleStatus,
    /// 场景里一个实例都没有（宿主去显示它，落地后重试一次）。
    pub no_instances: bool,
    /// 作业期间实体被卸载 / 重装。
    pub dropped: bool,
    pub error: Option<String>,
    pub elapsed_ms: u32,
}

/// A local-space cutter description. The actual Manifold backend turns this
/// into a cylinder or cuboid and applies `orientation` and `center_mm`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CutterSpec {
    pub shape: VirtualHoleShape,
    pub center_mm: Vec3,
    pub orientation: Mat3,
    pub size_mm: Vec3,
}

/// libgm 的 `restol`（绝对量，mm）：负体每边向外让这么多，共壁时不碎成薄片。
/// 与 gen-model `manifold_csg.rs` 的 `NEGATIVE_INFLATE_MM` 同值。
pub const NEGATIVE_INFLATE_MM: f64 = 0.051;

/// 孔洞切削体的有向包围盒（世界毫米），宽相筛选用。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HoleObb {
    pub center: Vec3,
    /// 三根半轴方向（单位向量）。
    pub axes: [Vec3; 3],
    /// 三根半轴长度。
    pub half: Vec3,
}

impl VirtualHole {
    pub fn cutter(&self) -> Result<CutterSpec, String> {
        if !self.depth_mm.is_finite() || self.depth_mm <= 0.0 {
            return Err("SizeThrowWall must be a positive finite number".into());
        }
        let size_mm = match self.shape {
            VirtualHoleShape::Circle => {
                let d = self.diameter_mm.ok_or("circle hole has no diameter")?;
                if !d.is_finite() || d <= 0.0 { return Err("circle diameter must be positive".into()); }
                Vec3::new(d, d, self.depth_mm)
            }
            VirtualHoleShape::Rect => {
                let w = self.width_mm.ok_or("rect hole has no width")?;
                let h = self.height_mm.ok_or("rect hole has no height")?;
                if !w.is_finite() || !h.is_finite() || w <= 0.0 || h <= 0.0 {
                    return Err("rect width and height must be positive".into());
                }
                Vec3::new(w, h, self.depth_mm)
            }
        };
        let orientation = Mat3::from_cols(
            Vec3::from(self.orientation[0]),
            Vec3::from(self.orientation[1]),
            Vec3::from(self.orientation[2]),
        );
        if !orientation.is_finite() { return Err("orientation contains non-finite values".into()); }
        Ok(CutterSpec { shape: self.shape, center_mm: Vec3::from(self.center_mm), orientation, size_mm })
    }

    pub fn world_aabb_mm(&self) -> Result<([f32; 3], [f32; 3]), String> {
        let c = self.cutter()?;
        let half = c.size_mm * 0.5;
        let ext = c.orientation.abs() * half;
        let min = c.center_mm - ext;
        let max = c.center_mm + ext;
        Ok((min.to_array(), max.to_array()))
    }

    /// 切削体（未膨胀）的 OBB，世界毫米。
    pub fn obb(&self) -> Result<HoleObb, String> {
        let c = self.cutter()?;
        Ok(HoleObb {
            center: c.center_mm,
            axes: [c.orientation.x_axis, c.orientation.y_axis, c.orientation.z_axis],
            half: c.size_mm * 0.5,
        })
    }

    /// 膨胀后的切削体世界矩阵（毫米，f64）。基元是以原点为中心、轴沿 Z 的单位
    /// 圆柱（半径 0.5、高 1）或单位箱；矩阵 = T(center) · R · S(size + 2·让出)。
    pub fn cutter_mat4(&self) -> Result<DMat4, String> {
        let c = self.cutter()?;
        let size = c.size_mm.as_dvec3() + DVec3::splat(2.0 * NEGATIVE_INFLATE_MM);
        let rotation = DMat4::from_mat3(c.orientation.as_dmat3());
        Ok(DMat4::from_translation(c.center_mm.as_dvec3()) * rotation * DMat4::from_scale(size))
    }

    /// 膨胀后的切削体世界仿射（列主序 4×3，毫米）——布尔后端直接消费的形式。
    pub fn cutter_affine(&self) -> Result<[f64; 12], String> {
        Ok(dmat4_to_affine4x3(self.cutter_mat4()?))
    }

    /// 尺寸标签：圆孔 `Φ325`，矩形 `300×400`。
    pub fn size_label(&self) -> String {
        match self.shape {
            VirtualHoleShape::Circle => format!("Φ{}", fmt_mm(self.diameter_mm.unwrap_or(0.0))),
            VirtualHoleShape::Rect => format!(
                "{}×{}",
                fmt_mm(self.width_mm.unwrap_or(0.0)),
                fmt_mm(self.height_mm.unwrap_or(0.0))
            ),
        }
    }
}

/// glam 列主序 4×4 → 列主序 4×3 仿射（末列平移）。
pub fn dmat4_to_affine4x3(m: DMat4) -> [f64; 12] {
    [
        m.x_axis.x, m.x_axis.y, m.x_axis.z, m.y_axis.x, m.y_axis.y, m.y_axis.z, m.z_axis.x,
        m.z_axis.y, m.z_axis.z, m.w_axis.x, m.w_axis.y, m.w_axis.z,
    ]
}

/// OBB 与 AABB 的分离轴测试（15 根轴）。宽相筛选用：宁可误报，不可漏报。
pub fn obb_intersects_aabb(obb: &HoleObb, min: [f32; 3], max: [f32; 3]) -> bool {
    let min = Vec3::from(min);
    let max = Vec3::from(max);
    let d = (min + max) * 0.5 - obb.center; // AABB 中心相对 OBB 中心
    let e = (max - min) * 0.5; // AABB 半长
    let mut axes = Vec::with_capacity(15);
    axes.extend([Vec3::X, Vec3::Y, Vec3::Z, obb.axes[0], obb.axes[1], obb.axes[2]]);
    for i in 0..3 {
        for j in 0..3 {
            axes.push(obb.axes[i].cross(obb.axes[j]));
        }
    }
    for axis in axes {
        let len = axis.length();
        if len <= f32::EPSILON {
            continue; // 重合轴（i == j 的叉积）跳过
        }
        let a = axis / len;
        let r = e.x * a.x.abs() + e.y * a.y.abs() + e.z * a.z.abs();
        let t = d.dot(a).abs();
        let rb = obb.half.x * obb.axes[0].dot(a).abs()
            + obb.half.y * obb.axes[1].dot(a).abs()
            + obb.half.z * obb.axes[2].dot(a).abs();
        if t > r + rb {
            return false;
        }
    }
    true
}

/// 圆孔离散段数：libgm 口径（弦高容差 0.05mm，45° 步长封顶，收敛到 4 的倍数，
/// 最少 8 段、上限 1000）。渲染层显示与布尔切削体共用这个数，两端一致。
pub fn circle_segments(diameter_mm: f32) -> u32 {
    const CHORD_TOL_MM: f64 = 0.05;
    const MAX_SEGMENTS: i32 = 1000;
    let radius = diameter_mm as f64 / 2.0;
    if !(radius > 0.0) {
        return 8;
    }
    let x = (1.0 - (CHORD_TOL_MM / radius).abs()).max(0.0);
    let mut step_deg = x.acos().to_degrees() * 2.0;
    if !(step_deg > 0.0) || step_deg > 45.0 {
        step_deg = 45.0;
    }
    let n = (360.0 / step_deg).ceil() as i32;
    let n = ((n + 3) & !3).clamp(8, MAX_SEGMENTS);
    n as u32
}

/// 解析 RelyItemREF 里的参考号：`24381_145018`、`24381/145018`、`=24381_145018`。
/// 只认「两段数字」的写法——裸数字可能是别的东西（名字 / 流水号），不猜。
pub fn parse_rely_refno(raw: &str) -> Option<RefU64> {
    let s = raw.trim();
    let s = s.strip_prefix('=').unwrap_or(s);
    let mut parts = s.split(['_', '/']);
    let a = parts.next()?.parse::<u32>().ok()?;
    let b = parts.next()?.parse::<u32>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(RefU64::from_two_nums(a, b))
}

/// 整数毫米的显示：去掉多余的 `.0`。
pub fn fmt_mm(value: f32) -> String {
    if (value - value.round()).abs() < 1e-4 {
        format!("{}", value.round() as i64)
    } else {
        format!("{value:.1}")
    }
}

pub fn aabbs_intersect(a: ([f32; 3], [f32; 3]), b: ([f32; 3], [f32; 3])) -> bool {
    (0..3).all(|i| a.0[i] <= b.1[i] && b.0[i] <= a.1[i])
}

#[cfg(test)]
mod tests {
    use super::*;
    fn hole(shape: VirtualHoleShape) -> VirtualHole {
        VirtualHole { id: "h".into(), shape, center_mm: [0.; 3], orientation: [[1.,0.,0.],[0.,1.,0.],[0.,0.,1.]], diameter_mm: Some(300.), width_mm: Some(300.), height_mm: Some(400.), depth_mm: 800., rely_item_ref: "r".into(), rely_refno: None, item_ref: "i".into(), source_row: 2, hole_work: "CREATE".into(), h_type: "K".into(), subs_material: String::new(), raw_shape: "".into(), raw_orientation: "".into() }
    }
    #[test] fn circle_uses_diameter_and_depth() { assert_eq!(hole(VirtualHoleShape::Circle).cutter().unwrap().size_mm, Vec3::new(300.,300.,800.)); }
    #[test] fn rotated_aabb_is_wider() { let mut h=hole(VirtualHoleShape::Rect); h.orientation=[[0.,1.,0.],[1.,0.,0.],[0.,0.,1.]]; let (min,max)=h.world_aabb_mm().unwrap(); assert_eq!(max[0]-min[0],400.); }
    #[test] fn invalid_zero_dimension_is_rejected() { let mut h=hole(VirtualHoleShape::Rect); h.height_mm=Some(0.); assert!(h.cutter().is_err()); }

    #[test]
    fn status_severity_orders_failures_first() {
        assert!(VirtualHoleStatus::Failed.severity() > VirtualHoleStatus::Changed.severity());
        assert!(VirtualHoleStatus::Changed.severity() > VirtualHoleStatus::NoIntersection.severity());
        assert!(VirtualHoleStatus::NoIntersection.severity() > VirtualHoleStatus::Calculating.severity());
        assert!(VirtualHoleStatus::Calculating.severity() > VirtualHoleStatus::Ready.severity());
    }

    #[test]
    fn obb_of_identity_rect_is_axis_aligned() {
        let h = hole(VirtualHoleShape::Rect);
        let obb = h.obb().unwrap();
        assert_eq!(obb.center, Vec3::ZERO);
        assert_eq!(obb.half, Vec3::new(150., 200., 400.));
        assert!(obb_intersects_aabb(&obb, [-1., -1., -1.], [1., 1., 1.]));
        assert!(!obb_intersects_aabb(&obb, [1000., 0., 0.], [1100., 100., 100.]));
    }

    #[test]
    fn sat_catches_rotated_obb_miss() {
        // OBB 绕 Z 转 45°：角上翘出 AABB 轴对齐盒，但与一个角盒仍相交 / 不相交要分清。
        let mut h = hole(VirtualHoleShape::Rect);
        let s = 1.0 / 2.0_f32.sqrt();
        h.orientation = [[s, s, 0.], [-s, s, 0.], [0., 0., 1.]];
        h.center_mm = [100., 0., 0.];
        let obb = h.obb().unwrap();
        // 远处的盒不相交。
        assert!(!obb_intersects_aabb(&obb, [500., 500., 500.], [600., 600., 600.]));
        // 重叠的盒相交。
        assert!(obb_intersects_aabb(&obb, [0., -1000., -1000.], [200., 1000., 1000.]));
    }

    #[test]
    fn cutter_mat4_inflates_by_restol() {
        let h = hole(VirtualHoleShape::Circle);
        let m = h.cutter_mat4().unwrap();
        // 直径 300 + 2×0.051 = 300.102；高度 800 + 0.102 = 800.102。
        assert!((m.x_axis.x - 300.102).abs() < 1e-9);
        assert!((m.z_axis.z - 800.102).abs() < 1e-9);
        let affine = h.cutter_affine().unwrap();
        assert_eq!(affine.len(), 12);
        assert!((affine[0] - 300.102).abs() < 1e-9);
    }

    #[test]
    fn circle_segments_follow_libgm_rule() {
        assert_eq!(circle_segments(325.0) % 4, 0);
        assert!(circle_segments(325.0) >= 8);
        assert!(circle_segments(325.0) <= 1000);
        assert_eq!(circle_segments(325.0), 128); // 弦高 0.05mm 口径
        assert_eq!(circle_segments(0.0), 8);
        assert_eq!(circle_segments(1e7), 1000); // 封顶
    }

    #[test]
    fn rely_refno_parses_two_num_forms() {
        let a = parse_rely_refno("24381_145018").unwrap();
        let b = parse_rely_refno("24381/145018").unwrap();
        let c = parse_rely_refno("=24381_145018").unwrap();
        assert_eq!(a, b);
        assert_eq!(b, c);
        assert_eq!(a, RefU64::from_two_nums(24381, 145018));
        assert!(parse_rely_refno("WALL-01").is_none());
        assert!(parse_rely_refno("123").is_none()); // 裸数字不猜
        assert!(parse_rely_refno("1_2_3").is_none());
        assert!(parse_rely_refno("").is_none());
    }

    #[test]
    fn size_label_formats() {
        assert_eq!(hole(VirtualHoleShape::Circle).size_label(), "Φ300");
        assert_eq!(hole(VirtualHoleShape::Rect).size_label(), "300×400");
    }
}
