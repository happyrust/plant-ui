//! MBD 契约 → 视口尺寸标注层的线段（`gen-model/.planning/2026-09-09-plant-ui-mbd-dimensions` B3）。
//!
//! 契约 `source_mm` 几何直接当 PDMS 毫米用，**不吃** `meta.source_to_design`——那是网页设计系
//! 的事；几何空间不是 `source_mm` 的响应在 `mbd_api::classify` 就被拒收了，到这里的一定是毫米。
//! 这里只做「图元 → 线段」的翻译，视口（`plant_ui_view3d::DimensionBatch`）只管画：
//!
//! - `linear_dim`：尺寸线 start→end + 延长线 + 箭头小线段；
//! - `angle_dim`：弧按 ADR-0011 框架采样成折线 + 两条腿；
//! - `leader_line` / `slope_mark`：一条线；
//! - `aid_line`：一条线，`dashed` / `dash_dot` 切成真实长度的虚线段（`dash_dot` 首版按 `dashed` 画）；
//! - `aid_arc` / `aid_circle`：弧 / 整圆采样成折线；
//! - `aid_point` / `weld_mark`：位置上一枚小十字（三轴各一段，半长按字高定）；
//! - 文字：`linear_dim` / `angle_dim` 的 `text` 落在 `label_anchor`，`label` / `aid_text` 落在
//!   `position`，`slope_mark` 的 `text` 落在线段中点——都只交锚点 + 字，视口逐帧投影成纹理 UV，
//!   egui 侧画字（`axis_labels` 同机制）；空字符串不出字。
//!
//! 几何不合法（端点非有限、半径非正、轴退化、扫角非正、锚点非有限）的图元**整个**丢掉并计数
//! ——fail-closed，不画半截：少一条箭头的尺寸线看着像量到了别处。

use bevy::math::Vec3;
use plant_mbd::contract::Vec3V2;
use plant_mbd::{MbdPrimitive, MbdV2LineSegment, MbdV2PipeData};
use plant_ui_view3d::{DimensionBatch, DimensionLabel, DimensionLine};

/// 弧采样的角步长（度）。整圆 72 段肉眼看不出棱，再细只会让大 BRAN 的顶点数白翻倍（计划 R2）。
const ARC_STEP_DEG: f32 = 5.0;
/// 虚线的真实长度节距（毫米），与无效 TUBI 诊断线同一量级。
const DASH_MM: f32 = 80.0;
const GAP_MM: f32 = 50.0;
/// 契约没给字高时的兜底：PML 校准值 `cheight = 25`（plant-mbd 箭头比例就锚在它上）。
const DEFAULT_CHEIGHT_MM: f32 = 25.0;
/// 小十字（`aid_point` / `weld_mark`）半长与字高的比。
const MARK_HALF_PER_CHEIGHT: f32 = 0.3;

/// 一次翻译的产物。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Mapped {
    pub batch: DimensionBatch,
    /// 因几何不合法被整个丢掉的图元数。宿主把它说进日志，别让画面悄悄少东西。
    pub skipped: usize,
}

/// 把一条 BRAN 的契约翻成线段批次。
pub fn batch_of(data: &MbdV2PipeData) -> Mapped {
    let cheight = data
        .meta
        .cheight_mm
        .filter(|h| h.is_finite() && *h > 0.0)
        .unwrap_or(DEFAULT_CHEIGHT_MM);
    let mut mapped = Mapped::default();
    for primitive in &data.primitives {
        match pieces_of(primitive, cheight) {
            Some(pieces) => {
                mapped.batch.lines.extend(pieces.lines);
                mapped.batch.labels.extend(pieces.labels);
            }
            None => mapped.skipped += 1,
        }
    }
    mapped
}

/// 一个图元翻出来的线与字。
#[derive(Default)]
struct Pieces {
    lines: Vec<DimensionLine>,
    labels: Vec<DimensionLabel>,
}

impl Pieces {
    /// 记一条文字；空字符串不出字，锚点非有限回 None（整个图元丢弃）。
    fn label(&mut self, anchor: Vec3, text: &str) -> Option<()> {
        if !anchor.is_finite() {
            return None;
        }
        if !text.is_empty() {
            self.labels.push(DimensionLabel {
                anchor: anchor.to_array(),
                text: text.to_owned(),
            });
        }
        Some(())
    }
}

/// 一个图元的全部线段与文字；几何不合法回 None（整个图元丢弃）。
fn pieces_of(primitive: &MbdPrimitive, cheight: f32) -> Option<Pieces> {
    let mut pieces = Pieces::default();
    let out = &mut pieces.lines;
    match primitive {
        MbdPrimitive::LinearDim {
            start,
            end,
            text,
            extension_lines,
            arrow_lines,
            label_anchor,
            ..
        } => {
            push_line(out, *start, *end)?;
            push_segments(out, extension_lines)?;
            push_segments(out, arrow_lines)?;
            pieces.label(Vec3::from_array(*label_anchor), text)?;
        }
        MbdPrimitive::AngleDim {
            text,
            center,
            x_axis,
            normal,
            radius,
            start_angle_deg,
            sweep_angle_deg,
            leg_lines,
            label_anchor,
            ..
        } => {
            let frame = ArcFrame::new(*center, *x_axis, *normal, *radius)?;
            push_polyline(out, &frame.sample(*start_angle_deg, *sweep_angle_deg)?);
            push_segments(out, leg_lines)?;
            pieces.label(Vec3::from_array(*label_anchor), text)?;
        }
        MbdPrimitive::LeaderLine { start, end, .. } => {
            push_line(out, *start, *end)?;
        }
        // 坡度符号的字没有自己的锚点：PML 把它贴在符号旁边，这里取线段中点。
        MbdPrimitive::SlopeMark {
            start, end, text, ..
        } => {
            let (from, to) = finite_pair(*start, *end)?;
            out.push(line(from, to));
            pieces.label((from + to) * 0.5, text)?;
        }
        MbdPrimitive::AidLine {
            start, end, style, ..
        } => {
            let (from, to) = finite_pair(*start, *end)?;
            if is_dashed(style.as_deref()) {
                push_dashed(out, from, to);
            } else {
                out.push(line(from, to));
            }
        }
        MbdPrimitive::AidArc {
            center,
            x_axis,
            normal,
            radius,
            start_angle_deg,
            sweep_angle_deg,
            ..
        } => {
            let frame = ArcFrame::new(*center, *x_axis, *normal, *radius)?;
            push_polyline(out, &frame.sample(*start_angle_deg, *sweep_angle_deg)?);
        }
        MbdPrimitive::AidCircle {
            center,
            normal,
            radius,
            ..
        } => {
            let frame = ArcFrame::circle(*center, *normal, *radius)?;
            push_polyline(out, &frame.sample(0.0, 360.0)?);
        }
        MbdPrimitive::AidPoint { position, .. } | MbdPrimitive::WeldMark { position, .. } => {
            push_mark(out, *position, cheight * MARK_HALF_PER_CHEIGHT)?;
        }
        MbdPrimitive::Label { text, position, .. }
        | MbdPrimitive::AidText { text, position, .. } => {
            pieces.label(Vec3::from_array(*position), text)?;
        }
    }
    Some(pieces)
}

fn line(from: Vec3, to: Vec3) -> DimensionLine {
    DimensionLine {
        from: from.to_array(),
        to: to.to_array(),
    }
}

/// 两端都有限才算一条线。
fn finite_pair(from: Vec3V2, to: Vec3V2) -> Option<(Vec3, Vec3)> {
    let from = Vec3::from_array(from);
    let to = Vec3::from_array(to);
    (from.is_finite() && to.is_finite()).then_some((from, to))
}

fn push_line(out: &mut Vec<DimensionLine>, from: Vec3V2, to: Vec3V2) -> Option<()> {
    let (from, to) = finite_pair(from, to)?;
    out.push(line(from, to));
    Some(())
}

fn push_segments(out: &mut Vec<DimensionLine>, segments: &[MbdV2LineSegment]) -> Option<()> {
    for segment in segments {
        push_line(out, segment.from, segment.to)?;
    }
    Some(())
}

fn push_polyline(out: &mut Vec<DimensionLine>, points: &[Vec3]) {
    out.extend(points.windows(2).map(|pair| line(pair[0], pair[1])));
}

/// 真实长度虚线：与无效 TUBI 诊断线同一套切法——两端都落在线段端点上，短于一个节距的
/// 线退化成一整条，不会彻底消失。
fn push_dashed(out: &mut Vec<DimensionLine>, from: Vec3, to: Vec3) {
    let length = from.distance(to);
    if !length.is_finite() || length <= DASH_MM + GAP_MM {
        out.push(line(from, to));
        return;
    }
    let count = ((length + GAP_MM) / (DASH_MM + GAP_MM)).ceil().max(2.0) as usize;
    let dash = (length - GAP_MM * (count - 1) as f32) / count as f32;
    let dir = (to - from) / length;
    for index in 0..count {
        let start = index as f32 * (dash + GAP_MM);
        out.push(line(from + dir * start, from + dir * (start + dash)));
    }
}

fn is_dashed(style: Option<&str>) -> bool {
    matches!(style, Some("dashed" | "dash_dot"))
}

/// 位置上的小十字：三轴各一段，半长 `half`。
fn push_mark(out: &mut Vec<DimensionLine>, position: Vec3V2, half: f32) -> Option<()> {
    let center = Vec3::from_array(position);
    if !center.is_finite() || !half.is_finite() || half <= 0.0 {
        return None;
    }
    for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
        out.push(line(center - axis * half, center + axis * half));
    }
    Some(())
}

/// ADR-0011 的弧框架：`center` 弧心，`x` 0° 方向，`y = normal × x`，正扫角从 `x` 转向 `y`。
struct ArcFrame {
    center: Vec3,
    x: Vec3,
    y: Vec3,
    radius: f32,
}

impl ArcFrame {
    /// 契约要求 `x_axis ⟂ normal` 且都是单位向量，但前端仍自己保证：对 `normal` 归一、对
    /// `x_axis` 做一次 Gram-Schmidt——产出方哪天差了千分之一，弧也不该歪成椭圆。
    fn new(center: Vec3V2, x_axis: Vec3V2, normal: Vec3V2, radius: f32) -> Option<Self> {
        let center = Vec3::from_array(center);
        if !center.is_finite() || !radius.is_finite() || radius <= 0.0 {
            return None;
        }
        let normal = Vec3::from_array(normal);
        let x_axis = Vec3::from_array(x_axis);
        if !normal.is_finite() || !x_axis.is_finite() {
            return None;
        }
        let n = normal.try_normalize()?;
        let x = (x_axis - n * x_axis.dot(n)).try_normalize()?;
        Some(Self {
            center,
            x,
            y: n.cross(x),
            radius,
        })
    }

    /// 整圆没有面内参考轴：随便取一个与法向不平行的轴叉出来当 0° 方向，反正从哪儿起画都是同一个圆。
    fn circle(center: Vec3V2, normal: Vec3V2, radius: f32) -> Option<Self> {
        let n = Vec3::from_array(normal);
        if !n.is_finite() {
            return None;
        }
        let n = n.try_normalize()?;
        let helper = if n.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
        Self::new(center, n.cross(helper).to_array(), n.to_array(), radius)
    }

    /// 从 `start_deg` 起、扫 `sweep_deg`（度，正向 `x → y`）采样成折线顶点。扫角非正或非有限
    /// 回 None；超过 360 按整圆封顶，整圆的收尾顶点直接取首顶点，闭合处不留浮点缝。
    fn sample(&self, start_deg: f32, sweep_deg: f32) -> Option<Vec<Vec3>> {
        if !start_deg.is_finite() || !sweep_deg.is_finite() || sweep_deg <= 0.0 {
            return None;
        }
        let sweep = sweep_deg.min(360.0);
        let segments = ((sweep / ARC_STEP_DEG).ceil() as usize).max(1);
        let mut points: Vec<Vec3> = (0..=segments)
            .map(|index| {
                let angle = (start_deg + sweep * index as f32 / segments as f32).to_radians();
                self.center + (self.x * angle.cos() + self.y * angle.sin()) * self.radius
            })
            .collect();
        if sweep >= 360.0 {
            points[segments] = points[0];
        }
        Some(points)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plant_mbd::MbdV2Meta;

    fn data(primitives: Vec<MbdPrimitive>, cheight_mm: Option<f32>) -> MbdV2PipeData {
        MbdV2PipeData {
            primitives,
            meta: MbdV2Meta {
                cheight_mm,
                ..Default::default()
            },
            ..MbdV2PipeData::empty("1_1", "1_1")
        }
    }

    fn seg(from: Vec3V2, to: Vec3V2) -> MbdV2LineSegment {
        MbdV2LineSegment { from, to }
    }

    fn linear(
        start: Vec3V2,
        end: Vec3V2,
        extension_lines: Vec<MbdV2LineSegment>,
        arrow_lines: Vec<MbdV2LineSegment>,
    ) -> MbdPrimitive {
        MbdPrimitive::LinearDim {
            id: "d1".into(),
            start,
            end,
            text: "1000".into(),
            sub_kind: None,
            extension_lines,
            arrow_lines,
            label_anchor: [500.0, 0.0, 0.0],
            reference: None,
        }
    }

    /// 尺寸线 = 主线 + 延长线 + 箭头，按这个顺序全进批次；文字不出线。
    #[test]
    fn a_linear_dim_becomes_its_line_extension_and_arrow_segments() {
        let dim = linear(
            [0.0, 0.0, 0.0],
            [1000.0, 0.0, 0.0],
            vec![seg([0.0, 0.0, 0.0], [0.0, 0.0, 200.0])],
            vec![
                seg([0.0, 0.0, 0.0], [24.0, 7.0, 0.0]),
                seg([0.0, 0.0, 0.0], [24.0, -7.0, 0.0]),
            ],
        );
        let mapped = batch_of(&data(vec![dim], Some(25.0)));
        assert_eq!(mapped.skipped, 0);
        let lines = &mapped.batch.lines;
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0].from, [0.0, 0.0, 0.0]);
        assert_eq!(lines[0].to, [1000.0, 0.0, 0.0]);
        assert_eq!(lines[1].to, [0.0, 0.0, 200.0]);
        assert_eq!(lines[2].to, [24.0, 7.0, 0.0]);
        assert_eq!(lines[3].to, [24.0, -7.0, 0.0]);
        // 尺寸数值落在求解器给的 label_anchor 上，不自己算中点。
        assert_eq!(
            mapped.batch.labels,
            vec![DimensionLabel {
                anchor: [500.0, 0.0, 0.0],
                text: "1000".into(),
            }]
        );
    }

    /// 文字的锚点：`label` / `aid_text` 用自己的 `position`，`slope_mark` 取线段中点，
    /// `angle_dim` 用 `label_anchor`；空字符串不出字；锚点非有限整个图元丢掉并计数。
    #[test]
    fn labels_sit_on_their_anchors_and_bad_anchors_drop_the_primitive() {
        let mapped = batch_of(&data(
            vec![
                MbdPrimitive::Label {
                    id: "lb".into(),
                    text: "/PIPE-1".into(),
                    position: [1.0, 2.0, 3.0],
                },
                MbdPrimitive::AidText {
                    id: "t".into(),
                    text: "N".into(),
                    position: [4.0, 5.0, 6.0],
                },
                MbdPrimitive::SlopeMark {
                    id: "s".into(),
                    text: "1:100".into(),
                    start: [0.0, 0.0, 0.0],
                    end: [200.0, 0.0, 100.0],
                },
                MbdPrimitive::AidText {
                    id: "empty".into(),
                    text: String::new(),
                    position: [0.0; 3],
                },
                MbdPrimitive::Label {
                    id: "bad".into(),
                    text: "x".into(),
                    position: [f32::NAN, 0.0, 0.0],
                },
                linear([0.0; 3], [10.0, 0.0, 0.0], vec![], vec![]),
            ],
            None,
        ));
        assert_eq!(mapped.skipped, 1, "只有 NaN 锚点那条 label 被丢");
        let texts: Vec<(&str, [f32; 3])> = mapped
            .batch
            .labels
            .iter()
            .map(|l| (l.text.as_str(), l.anchor))
            .collect();
        assert_eq!(
            texts,
            vec![
                ("/PIPE-1", [1.0, 2.0, 3.0]),
                ("N", [4.0, 5.0, 6.0]),
                ("1:100", [100.0, 0.0, 50.0]),
                ("1000", [500.0, 0.0, 0.0]),
            ]
        );
        // 坡度符号自己那条线照出。
        assert_eq!(mapped.batch.lines.len(), 2);

        // 尺寸线的锚点坏了：线也不画，整个图元丢。
        let bad_anchor = MbdPrimitive::LinearDim {
            id: "d".into(),
            start: [0.0; 3],
            end: [10.0, 0.0, 0.0],
            text: "10".into(),
            sub_kind: None,
            extension_lines: vec![],
            arrow_lines: vec![],
            label_anchor: [0.0, f32::INFINITY, 0.0],
            reference: None,
        };
        let mapped = batch_of(&data(vec![bad_anchor], None));
        assert_eq!(mapped.skipped, 1);
        assert!(mapped.batch.lines.is_empty() && mapped.batch.labels.is_empty());
    }

    /// 少一条箭头的尺寸线看着像量到了别处：任何一段不合法，整个图元丢掉并计数。
    #[test]
    fn a_dimension_with_one_bad_piece_is_dropped_whole() {
        let bad_arrow = linear(
            [0.0; 3],
            [1000.0, 0.0, 0.0],
            vec![],
            vec![seg([0.0; 3], [f32::NAN, 7.0, 0.0])],
        );
        let good = MbdPrimitive::LeaderLine {
            id: "l".into(),
            start: [0.0; 3],
            end: [0.0, 0.0, 300.0],
        };
        let mapped = batch_of(&data(vec![bad_arrow, good], None));
        assert_eq!(mapped.skipped, 1);
        assert_eq!(mapped.batch.lines.len(), 1);
        assert_eq!(mapped.batch.lines[0].to, [0.0, 0.0, 300.0]);
    }

    /// 虚线切成真实长度的段：两端落在原线段端点上，段与段之间有缝；实线与短线保持一条。
    #[test]
    fn dashed_aid_lines_are_cut_into_real_length_dashes() {
        let aid = |style: Option<&str>, length: f32| MbdPrimitive::AidLine {
            id: "a".into(),
            start: [0.0; 3],
            end: [length, 0.0, 0.0],
            style: style.map(str::to_owned),
        };
        let dashed = batch_of(&data(vec![aid(Some("dashed"), 1000.0)], None))
            .batch
            .lines;
        assert!(dashed.len() > 2, "{dashed:?}");
        assert_eq!(dashed.first().unwrap().from, [0.0; 3]);
        assert!((dashed.last().unwrap().to[0] - 1000.0).abs() < 1e-3);
        for pair in dashed.windows(2) {
            assert!(pair[1].from[0] > pair[0].to[0], "段间要有缝：{pair:?}");
        }
        assert_eq!(
            batch_of(&data(vec![aid(Some("dash_dot"), 1000.0)], None))
                .batch
                .lines
                .len(),
            dashed.len(),
            "dash_dot 首版按 dashed 画"
        );
        assert_eq!(
            batch_of(&data(vec![aid(Some("solid"), 1000.0)], None))
                .batch
                .lines
                .len(),
            1
        );
        assert_eq!(
            batch_of(&data(vec![aid(None, 1000.0)], None))
                .batch
                .lines
                .len(),
            1
        );
        // 短于一个节距：退化成一整条，不会彻底消失。
        assert_eq!(
            batch_of(&data(vec![aid(Some("dashed"), 100.0)], None))
                .batch
                .lines
                .len(),
            1
        );
    }

    /// 弧按框架采样：起终点落在 `start` / `start + sweep` 的方向上，半径处处相等，正扫角从
    /// `x_axis` 转向 `normal × x_axis`；整圆首尾闭合；`angle_dim` 的两条腿跟在弧后面。
    #[test]
    fn arcs_are_sampled_on_the_adr_0011_frame() {
        let arc = MbdPrimitive::AidArc {
            id: "arc".into(),
            center: [100.0, 200.0, 300.0],
            x_axis: [1.0, 0.0, 0.0],
            normal: [0.0, 0.0, 1.0],
            radius: 50.0,
            start_angle_deg: 0.0,
            sweep_angle_deg: 90.0,
            style: None,
        };
        let lines = batch_of(&data(vec![arc], None)).batch.lines;
        assert_eq!(lines.len(), 18, "90° / 5° = 18 段");
        assert_eq!(lines[0].from, [150.0, 200.0, 300.0]);
        let end = Vec3::from_array(lines.last().unwrap().to);
        assert!(
            end.abs_diff_eq(Vec3::new(100.0, 250.0, 300.0), 1e-3),
            "{end}"
        );
        for l in &lines {
            let r = Vec3::from_array(l.to).distance(Vec3::new(100.0, 200.0, 300.0));
            assert!((r - 50.0).abs() < 1e-3, "{r}");
        }

        let circle = MbdPrimitive::AidCircle {
            id: "c".into(),
            center: [0.0; 3],
            normal: [0.0, 1.0, 0.0],
            radius: 10.0,
            style: None,
        };
        let lines = batch_of(&data(vec![circle], None)).batch.lines;
        assert_eq!(lines.len(), 72);
        assert_eq!(lines.first().unwrap().from, lines.last().unwrap().to);
        for l in &lines {
            assert!(l.to[1].abs() < 1e-4, "整圆要躺在法向的垂面里：{l:?}");
        }

        let angle = MbdPrimitive::AngleDim {
            id: "ang".into(),
            text: "45°".into(),
            center: [0.0; 3],
            // 故意给一个不正交、不归一的 x 轴：Gram-Schmidt 要把它扶正。
            x_axis: [2.0, 0.0, 0.5],
            normal: [0.0, 0.0, 2.0],
            radius: 100.0,
            start_angle_deg: 0.0,
            sweep_angle_deg: 45.0,
            leg_lines: vec![seg([0.0; 3], [150.0, 0.0, 0.0])],
            label_anchor: [0.0; 3],
            sub_kind: None,
        };
        let lines = batch_of(&data(vec![angle], None)).batch.lines;
        assert_eq!(lines.len(), 9 + 1);
        assert_eq!(lines[0].from, [100.0, 0.0, 0.0], "x 轴扶正后 0° 落在 +X 上");
        assert_eq!(lines[9].to, [150.0, 0.0, 0.0]);
    }

    /// 半径非正、法向退化、扫角非正：整个弧丢掉并计数，不画一个点或一条乱线。
    #[test]
    fn degenerate_arcs_are_dropped_and_counted() {
        let arc = |radius: f32, normal: Vec3V2, sweep: f32| MbdPrimitive::AidArc {
            id: "arc".into(),
            center: [0.0; 3],
            x_axis: [1.0, 0.0, 0.0],
            normal,
            radius,
            start_angle_deg: 0.0,
            sweep_angle_deg: sweep,
            style: None,
        };
        let mapped = batch_of(&data(
            vec![
                arc(0.0, [0.0, 0.0, 1.0], 90.0),
                arc(10.0, [0.0, 0.0, 0.0], 90.0),
                arc(10.0, [0.0, 0.0, 1.0], 0.0),
                // x 轴与法向平行：Gram-Schmidt 后没有面内方向。
                MbdPrimitive::AidArc {
                    id: "p".into(),
                    center: [0.0; 3],
                    x_axis: [0.0, 0.0, 3.0],
                    normal: [0.0, 0.0, 1.0],
                    radius: 10.0,
                    start_angle_deg: 0.0,
                    sweep_angle_deg: 90.0,
                    style: None,
                },
            ],
            None,
        ));
        assert_eq!(mapped.skipped, 4);
        assert!(mapped.batch.lines.is_empty());
    }

    /// 焊缝 / 辅助点是一枚小十字，半长跟字高走（没给字高按 PML 校准值 25 兜底）；
    /// 文字图元不出线也不算丢。
    #[test]
    fn marks_are_small_crosses_and_text_draws_no_lines() {
        let mark = MbdPrimitive::WeldMark {
            id: "w".into(),
            position: [10.0, 20.0, 30.0],
            weld_type: None,
        };
        let point = MbdPrimitive::AidPoint {
            id: "p".into(),
            position: [0.0; 3],
        };
        let text = MbdPrimitive::AidText {
            id: "t".into(),
            text: "N".into(),
            position: [0.0; 3],
        };
        let label = MbdPrimitive::Label {
            id: "lb".into(),
            text: "/PIPE-1".into(),
            position: [0.0; 3],
        };
        let mapped = batch_of(&data(vec![mark, point, text, label], Some(50.0)));
        assert_eq!(mapped.skipped, 0);
        assert_eq!(mapped.batch.lines.len(), 6);
        let near = |actual: [f32; 3], expected: [f32; 3]| {
            assert!(
                Vec3::from_array(actual).abs_diff_eq(Vec3::from_array(expected), 1e-3),
                "{actual:?} vs {expected:?}"
            );
        };
        near(mapped.batch.lines[0].from, [-5.0, 20.0, 30.0]);
        near(mapped.batch.lines[0].to, [25.0, 20.0, 30.0]);
        near(mapped.batch.lines[2].from, [10.0, 20.0, 15.0]);

        let fallback = batch_of(&data(
            vec![MbdPrimitive::AidPoint {
                id: "p".into(),
                position: [0.0; 3],
            }],
            None,
        ));
        // 25 × 0.3
        near(fallback.batch.lines[0].to, [7.5, 0.0, 0.0]);
    }
}
