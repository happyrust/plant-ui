//! 房间视图的房间体图层与直段截断（2026-09-29 直段截断计划 M2 / M3；ADR-0028 / ADR-0029）。
//!
//! 放在一处，是因为后一件完全依赖前一件：
//!
//! - **房间体图层**：宿主交来这间房在册面板的模型记录，这里把它们画成半透明（与 X-Ray 同一枚
//!   材质）。它不挂拾取、不进可见性账，与尺寸标注层一样挂在 `SceneRoot` 旁边，所以隔离、显示 /
//!   隐藏、整场重装都碰不到它。面板网格全部就位后，用 `plant_room` 建出房间体（判定三角网）。
//! - **直段截断**（ADR-0028）：直段是共享单位原型按实例变换摆放的，截断就是把原型沿管轴的区间
//!   重映射到保留的那一截。直管件（FTUB / STRT）是整件烘焙的网格，管轴从网格读，同样只改变换。
//!   只动网格那一级子实体：保留段沿管轴重映射，整段在外的子实体隐藏，一段留下几截就补几个同原型的
//!   兄弟实体；每个截点盖一片同原型压成 2 mm 的断口，刷剖切色。模型根的变换、可见性与包围盒都不动，
//!   eye、隔离快照与 `view.bounds` 照旧。拾取是主世界三角形上的 CPU 射线，改了变换、藏了子实体，
//!   墙外那截自然点不中。
//!
//! 截断只动截断目标：宿主给的候选分支（BRAN / HANG）名下的直段，与宿主给的直管件。每帧限量处理，
//! 重装进来的自动补截。关掉、换房间、退出都先整批复原：变换还原、隐藏撤销、补出来的实体撤掉。

use std::collections::{HashMap, HashSet, VecDeque};

use aios_core::{GeomInstQuery, RefU64};
use bevy::asset::{AssetId, AssetServer, Assets, LoadState};
use bevy::math::DVec3;
use bevy::pbr::{NotShadowCaster, NotShadowReceiver};
use bevy::prelude::*;
use bevy::render::mesh::{PrimitiveTopology, VertexAttributeValues};
use plant_room::{RoomSolid, panel_tri_mesh};

use super::{
    ModelMesh, ModelPickTarget, View3d, XRayMaterial, invalid_tubi_fallback_line_mesh,
    mesh_source, scene_bounds_from_mm, scene_transform,
};

/// 断口的厚度（mm）：薄到读作一个面，又厚到在任何观察距离上都不与墙面、管壁争深度。
const CAP_THICKNESS_MM: f32 = 2.0;
/// 断口的截面比管子放大一点，盖住原型自带的端面和截点附近那一小段管壁。
const CAP_SCALE: f32 = 1.02;
/// 剖切色：一档洋红，与选中的橙红、X-Ray 的淡蓝、标注的近白和管道 / 结构 / 设备各档类型色都分得开。
const SECTION_COLOR: Color = Color::srgb_u8(0xD9, 0x46, 0xEF);
/// 每帧最多截几段。一根主管可能穿过半个厂，候选上千段时分几帧做完，HUD 报进度。
const TUBES_PER_FRAME: usize = 256;
/// 「沿某根局部轴的棱柱」判据的相对容差：顶点离两个端面的距离不超过长度的这个比例。
const PRISM_TOL: f32 = 1e-4;
/// 直管件的管轴判据：网格沿几根轴都算棱柱时（实心方盒三根都算），最长的那根至少是次长的这么多倍
/// 才认它是管轴。短而粗的方盒判不出方向，宁可原样画，也不沿截面方向截错。
const COMPONENT_AXIS_RATIO: f32 = 2.0;

/// 宿主交给视口的一次房间体（直段截断计划 §4.3）。
pub struct RoomVolumeBatch {
    /// 宿主这一次房间视图的帧号，原样回在 [`RoomClipStatus::generation`] 上，晚到的回执靠它认。
    pub generation: u64,
    /// 在册面板的模型记录（毫米、PDMS 世界系），与模型装载用的是同一种记录。
    pub panels: Vec<GeomInstQuery>,
    /// 截断候选：直段挂在它们名下的 BRAN / HANG。
    pub carriers: Vec<RefU64>,
    /// 截断候选里的直管件（FTUB / STRT）：网格挂在它们自己名下。
    pub components: Vec<RefU64>,
    /// 「截断直段」开关此刻的值。
    pub truncate: bool,
    /// 宿主已经认定房间体不全时带上原因（例如有的在册面板没有模型）。照样画半透明，但不截断。
    pub unavailable: Option<String>,
}

/// 房间体与截断此刻走到哪一步。
#[derive(Debug, Clone, PartialEq)]
pub enum RoomClipPhase {
    /// 面板网格在加载（按网格数计）。
    Loading { done: usize, total: usize },
    /// 房间体就绪，直段在逐段截。
    Computing { done: usize, total: usize },
    /// 做完了；截断关着时计数全零。
    Ready,
    /// 房间体取不全，不截断。
    Unavailable(String),
}

/// 房间体图层上一拍的回执，宿主抄进 HUD 与日志。
#[derive(Debug, Clone, PartialEq)]
pub struct RoomClipStatus {
    pub generation: u64,
    pub truncate: bool,
    pub phase: RoomClipPhase,
    /// 截断的段数（留下一截或几截，截点有断口）。
    pub cut: usize,
    /// 整段在房间体外、不画的段数。
    pub hidden: usize,
    /// 整段在房间体内、原样画的段数。
    pub whole: usize,
    /// 判不出管轴（网格不是棱柱、方盒短而粗，或没加载成）、截不了而原样画的段数。
    pub skipped: usize,
    /// 宿主交来的截断候选（去重后）：分支几根、直管件几件。
    pub carriers: usize,
    pub components: usize,
    /// 场景里找到的、挂在候选名下的直段与直管件网格。截断开着、房间体就绪而它为 0，就是候选
    /// 一个都没装进场景（所在的 SITE / ZONE 没打开），什么都截不到。
    pub found: usize,
}

/// 宿主交来的房间体指令，由 [`apply_room_clip_ops`] 按序执行。
pub(crate) enum RoomClipOp {
    Set(RoomVolumeBatch),
    Truncate(bool),
    Clear,
}

impl View3d {
    /// 挂上一间房的房间体（整层替换）：上一间的直段先整批复原，房间体图层撤掉，再按这一间来。
    /// 房间视图里「视图适应」与 Home 从此对准这间房的房间体。
    pub fn set_room_volume(&mut self, batch: RoomVolumeBatch) {
        self.room_frame_bounds = panel_bounds_mm(&batch.panels)
            .and_then(|(min_mm, max_mm)| scene_bounds_from_mm(min_mm, max_mm));
        self.room_clip_ops.push_back(RoomClipOp::Set(batch));
    }

    /// 拨「截断直段」：关 = 直段整批复原，开 = 按房间体重新截。没有房间体时是无操作。
    pub fn set_room_truncation(&mut self, on: bool) {
        self.room_clip_ops.push_back(RoomClipOp::Truncate(on));
    }

    /// 撤掉房间体图层，直段整批复原。没挂着时是无操作。
    pub fn clear_room_volume(&mut self) {
        self.room_frame_bounds = None;
        self.room_clip_ops.push_back(RoomClipOp::Clear);
    }

    /// 房间体图层上一拍的回执；没有房间体时为 `None`。
    pub fn room_clip_status(&self) -> Option<RoomClipStatus> {
        self.room_clip_status.clone()
    }
}

/// 一组面板记录的世界包围盒并集（毫米、PDMS 世界系）。宿主拿它对房间体取景，视口拿它做「视图
/// 适应」。非有限或翻转的盒子跳过；一个都不剩时为 `None`。
pub fn panel_bounds_mm(panels: &[GeomInstQuery]) -> Option<([f32; 3], [f32; 3])> {
    let mut bounds: Option<(Vec3, Vec3)> = None;
    for record in panels {
        let aabb = &record.world_aabb;
        let min = Vec3::new(aabb.mins.x, aabb.mins.y, aabb.mins.z);
        let max = Vec3::new(aabb.maxs.x, aabb.maxs.y, aabb.maxs.z);
        if !min.is_finite() || !max.is_finite() || min.cmpgt(max).any() {
            continue;
        }
        bounds = Some(match bounds {
            Some((lo, hi)) => (lo.min(min), hi.max(max)),
            None => (min, max),
        });
    }
    bounds.map(|(min, max)| (min.to_array(), max.to_array()))
}

/// 截断对象是哪一种（`CONTEXT.md`「直段」「直管件」），决定管轴怎么认。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StraightKind {
    /// 直段：共享单位原型按实例变换摆放，管轴按构造就是原型的局部 Z。
    Segment,
    /// 直管件：整件烘焙、按毫米摆的网格，管轴落在哪根局部轴上从网格读。
    Component,
}

/// 直管件的类型名：FTUB（定长直管）与 STRT（风管等的直段）。
pub(crate) fn is_straight_component(generic: &str) -> bool {
    let generic = generic.trim();
    generic.eq_ignore_ascii_case("FTUB") || generic.eq_ignore_ascii_case("STRT")
}

/// 一段直段或一件直管件的网格子实体（`load_models` 挂上）。截断与复原都从这里记下的原样出发。
#[derive(Component, Clone)]
pub(crate) struct TubeBody {
    kind: StraightKind,
    /// 子实体原本的局部变换（`inst.transform`）。
    base: Transform,
    /// 所属模型根的 `world_trans`（毫米、PDMS 世界系）。根的变换装进场景之后不再改。
    root: Transform,
    /// 无效直段的虚线网格原样。截断后按新长度换网格（保住 100 / 80 mm 的节距），复原时换回来。
    invalid_line: Option<Handle<Mesh>>,
}

impl TubeBody {
    pub(crate) fn solid(base: Transform, root: Transform) -> Self {
        Self {
            kind: StraightKind::Segment,
            base,
            root,
            invalid_line: None,
        }
    }

    pub(crate) fn invalid(base: Transform, root: Transform, line: Handle<Mesh>) -> Self {
        Self {
            kind: StraightKind::Segment,
            base,
            root,
            invalid_line: Some(line),
        }
    }

    pub(crate) fn component(base: Transform, root: Transform) -> Self {
        Self {
            kind: StraightKind::Component,
            base,
            root,
            invalid_line: None,
        }
    }
}

/// 截断时补出来的兄弟段与断口。复原时整批撤掉。
#[derive(Component)]
struct TubeExtra;

/// 房间体图层根。与 `SceneRoot` 平级，隔离、显示 / 隐藏、整场重装都碰不到它。
#[derive(Component)]
struct RoomVolumeLayer;

/// 全部断口共用的一枚材质。
#[derive(Resource)]
pub(crate) struct SectionMaterial(pub(crate) Handle<StandardMaterial>);

fn section_material() -> StandardMaterial {
    StandardMaterial {
        base_color: SECTION_COLOR,
        unlit: true,
        double_sided: true,
        cull_mode: None,
        ..default()
    }
}

pub(crate) fn setup_room_clip(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(SectionMaterial(materials.add(section_material())));
}

/// 原型网格在自己局部系里的形状：沿某根局部轴的棱柱，两个端面在这根轴上的坐标与截面中心。
///
/// 圆管原型 `CylinderV3` 是 z ∈ [0, 1]，矩形风管原型 `BoxV1` 是居中的 z ∈ [-0.5, 0.5]，
/// 烘焙出来的管身各有各的范围，所以范围只从网格自己读，写死会把风管截偏半根。直管件的网格是整件
/// 烘焙的，管轴不一定在 Z 上：R220 的 FTUB 是 x ∈ [0, 1500] 的实心方盒。
#[derive(Debug, Clone, Copy, PartialEq)]
struct Prism {
    /// 管轴是哪根局部轴：0 = X、1 = Y、2 = Z。
    axis: usize,
    /// 两个端面在管轴上的坐标。
    a0: f32,
    a1: f32,
    /// 截面中心；管轴那一维恒为 0。
    centre: Vec3,
}

impl Prism {
    /// 无效直段的虚线网格按构造就是局部 +Z 的 [0, 1]（`invalid_tubi_fallback_line_mesh`）。
    const UNIT_LINE: Prism = Prism {
        axis: 2,
        a0: 0.0,
        a1: 1.0,
        centre: Vec3::ZERO,
    };

    fn length(&self) -> f32 {
        self.a1 - self.a0
    }

    /// 管轴上坐标为 `a` 的截面中心。
    fn point(&self, a: f32) -> Vec3 {
        let mut point = self.centre;
        point[self.axis] = a;
        point
    }

    /// 三角网全部顶点都落在垂直于 `axis` 的两个端面上、两个端面的截面一样大，才算沿这根轴的棱柱。
    /// 锥（截面两头不一样大）、中间有顶点的（开孔、布尔过的、带法兰的）都不算，截了会走样。
    fn along(positions: &[[f32; 3]], min: Vec3, max: Vec3, axis: usize) -> Option<Prism> {
        let length = max[axis] - min[axis];
        if !length.is_finite() || !(length > 0.0) {
            return None;
        }
        let tol = length * PRISM_TOL;
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        let empty = (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY));
        let (mut bottom, mut top) = (empty, empty);
        for p in positions {
            let section = Vec2::new(p[u], p[v]);
            let end = if (p[axis] - min[axis]).abs() <= tol {
                &mut bottom
            } else if (p[axis] - max[axis]).abs() <= tol {
                &mut top
            } else {
                return None;
            };
            end.0 = end.0.min(section);
            end.1 = end.1.max(section);
        }
        let span = Vec2::new(max[u] - min[u], max[v] - min[v])
            .max_element()
            .max(f32::EPSILON);
        let same = (bottom.0 - top.0).abs().max_element() <= span * 1e-3
            && (bottom.1 - top.1).abs().max_element() <= span * 1e-3;
        (same && bottom.0.is_finite() && top.0.is_finite()).then(|| {
            let mut centre = (min + max) * 0.5;
            centre[axis] = 0.0;
            Prism {
                axis,
                a0: min[axis],
                a1: max[axis],
                centre,
            }
        })
    }
}

/// 原型网格沿三根局部轴各算不算棱柱，按 X、Y、Z 排；`None` = 沿那根轴不算。
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Prisms([Option<Prism>; 3]);

impl Prisms {
    fn of(mesh: &Mesh) -> Prisms {
        if mesh.primitive_topology() != PrimitiveTopology::TriangleList {
            return Prisms::default();
        }
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            return Prisms::default();
        };
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for p in positions {
            let p = Vec3::from_array(*p);
            min = min.min(p);
            max = max.max(p);
        }
        Prisms([0, 1, 2].map(|axis| Prism::along(positions, min, max, axis)))
    }

    /// 按截断对象挑管轴；`None` = 截不了。直段只认局部 Z：原型按构造沿 Z 摆，单位方盒三根轴
    /// 都算棱柱也只能是 Z。直管件取最长的那根，见 [`COMPONENT_AXIS_RATIO`]。
    fn pick(&self, kind: StraightKind) -> Option<Prism> {
        match kind {
            StraightKind::Segment => self.0[2],
            StraightKind::Component => {
                let mut found: Vec<Prism> = self.0.iter().flatten().copied().collect();
                found.sort_by(|a, b| b.length().total_cmp(&a.length()));
                match found.as_slice() {
                    [] => None,
                    [only] => Some(*only),
                    [longest, next, ..] => {
                        let clear = longest.length() >= COMPONENT_AXIS_RATIO * next.length();
                        clear.then_some(*longest)
                    }
                }
            }
        }
    }
}

/// 管轴两端的世界坐标（毫米、PDMS 世界系）：原型两个端面的截面中心，过子实体变换再过模型根。
fn axis_of(body: &TubeBody, prism: Prism) -> (DVec3, DVec3) {
    let end = |a: f32| {
        body.root
            .transform_point(body.base.transform_point(prism.point(a)))
            .as_dvec3()
    };
    (end(prism.a0), end(prism.a1))
}

/// 保留 `[t0, t1]` 那一截的子实体局部变换：原型沿管轴的 [a0, a1] 映到这一截对应的那一段。
///
/// 重映射只沿管轴缩放、平移，没有旋转，所以 `base × 它` 仍是精确的 TRS（缩放两两对角相乘）。
fn piece_transform(base: &Transform, prism: Prism, t0: f64, t1: f64) -> Transform {
    let start = prism.a0 + t0 as f32 * prism.length();
    let k = (t1 - t0) as f32;
    let mut translation = Vec3::ZERO;
    translation[prism.axis] = start - prism.a0 * k;
    let mut scale = Vec3::ONE;
    scale[prism.axis] = k;
    *base
        * Transform {
            translation,
            rotation: Quat::IDENTITY,
            scale,
        }
}

/// 截点 `t` 处断口的局部变换：同一原型压成 [`CAP_THICKNESS_MM`] 厚、以截点为中心，截面绕管轴
/// 放大 [`CAP_SCALE`]。
fn cap_transform(base: &Transform, prism: Prism, t: f64, length_mm: f32) -> Transform {
    let k = CAP_THICKNESS_MM / length_mm.max(f32::EPSILON);
    let centre = prism.a0 + t as f32 * prism.length();
    let half = 0.5 * k * prism.length();
    let s = CAP_SCALE;
    let mut translation = (1.0 - s) * prism.centre;
    translation[prism.axis] = centre - half - prism.a0 * k;
    let mut scale = Vec3::splat(s);
    scale[prism.axis] = k;
    *base
        * Transform {
            translation,
            rotation: Quat::IDENTITY,
            scale,
        }
}

/// 一段直段的去向。
enum Plan {
    Whole,
    Hidden,
    Cut(Vec<[f64; 2]>),
}

fn plan_of(intervals: Vec<[f64; 2]>) -> Plan {
    match intervals.as_slice() {
        [] => Plan::Hidden,
        [[t0, t1]] if *t0 <= 0.0 && *t1 >= 1.0 => Plan::Whole,
        _ => Plan::Cut(intervals),
    }
}

/// 一段直段处理之后的样子，复原时照它撤。
enum Outcome {
    Whole,
    Hidden,
    /// 补出来的兄弟段与断口。
    Cut(Vec<Entity>),
    Skipped,
}

#[derive(Debug, Default, Clone, Copy)]
struct Counts {
    cut: usize,
    hidden: usize,
    whole: usize,
    skipped: usize,
}

struct PanelMesh {
    handle: Handle<Mesh>,
    /// 网格局部 → 毫米世界：`world_trans × inst.transform`。
    matrix: Mat4,
}

enum Solid {
    Loading { done: usize },
    Ready(RoomSolid),
    Unavailable(String),
}

/// 一次房间体的全部状态。
struct Active {
    generation: u64,
    layer: Entity,
    /// 截断目标：候选分支与直管件。直段以所属分支的 refno 渲染、直管件以自己的 refno 渲染，
    /// 所以一张表认两种。
    targets: HashSet<RefU64>,
    /// 去重后的候选分支数与直管件数，原样回在回执上。
    carriers: usize,
    components: usize,
    truncate: bool,
    panels: Vec<PanelMesh>,
    solid: Solid,
    /// 房间体刚就绪、或开关刚拨开：下一拍把候选直段整批排进来。
    enqueue_all: bool,
    queue: VecDeque<Entity>,
    /// 排过队或处理过的直段，同一段不排两次。
    known: HashSet<Entity>,
    done: HashMap<Entity, Outcome>,
    counts: Counts,
}

#[derive(Resource, Default)]
pub(crate) struct RoomClip {
    active: Option<Active>,
    /// 原型网格 → 沿三根轴的棱柱判定。网格文件名是内容哈希，同一个资产的判定不会变。
    prisms: HashMap<AssetId<Mesh>, Prisms>,
}

/// 直段网格子实体上截断要读写的那几样。
type Tubes<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static TubeBody,
        &'static ModelMesh,
        &'static ModelPickTarget,
        &'static ChildOf,
        &'static mut Mesh3d,
        &'static mut Transform,
        &'static mut Visibility,
    ),
>;

impl Active {
    fn new(
        generation: u64,
        layer: Entity,
        panels: Vec<PanelMesh>,
        carriers: impl IntoIterator<Item = RefU64>,
        components: impl IntoIterator<Item = RefU64>,
        truncate: bool,
        unavailable: Option<String>,
    ) -> Self {
        let solid = match unavailable {
            Some(reason) => Solid::Unavailable(reason),
            None if panels.is_empty() => Solid::Unavailable("在册面板没有可画的网格".into()),
            None => Solid::Loading { done: 0 },
        };
        let carriers: HashSet<RefU64> = carriers.into_iter().collect();
        let components: HashSet<RefU64> = components.into_iter().collect();
        Self {
            generation,
            layer,
            carriers: carriers.len(),
            components: components.len(),
            targets: carriers.into_iter().chain(components).collect(),
            truncate,
            panels,
            solid,
            enqueue_all: false,
            queue: VecDeque::new(),
            known: HashSet::new(),
            done: HashMap::new(),
            counts: Counts::default(),
        }
    }

    /// 生成房间体图层：每块面板一个挂点、每个网格实例一个半透明子实体。
    fn spawn(
        commands: &mut Commands,
        assets: &AssetServer,
        xray: &Handle<StandardMaterial>,
        batch: RoomVolumeBatch,
    ) -> Self {
        let layer = commands
            .spawn((scene_transform(), Visibility::Visible, RoomVolumeLayer))
            .id();
        let mut panels = Vec::new();
        for record in &batch.panels {
            let panel = commands
                .spawn((record.world_trans, Visibility::Inherited, ChildOf(layer)))
                .id();
            for inst in &record.insts {
                let handle: Handle<Mesh> = assets.load(mesh_source::asset_path(&inst.geo_hash));
                commands.spawn((
                    Mesh3d(handle.clone()),
                    MeshMaterial3d(xray.clone()),
                    inst.transform,
                    NotShadowCaster,
                    NotShadowReceiver,
                    ChildOf(panel),
                ));
                panels.push(PanelMesh {
                    handle,
                    matrix: record.world_trans.compute_matrix() * inst.transform.compute_matrix(),
                });
            }
        }
        Self::new(
            batch.generation,
            layer,
            panels,
            batch.carriers,
            batch.components,
            batch.truncate,
            batch.unavailable,
        )
    }

    fn status(&self) -> RoomClipStatus {
        let phase = match &self.solid {
            Solid::Loading { done } => RoomClipPhase::Loading {
                done: *done,
                total: self.panels.len(),
            },
            Solid::Unavailable(reason) => RoomClipPhase::Unavailable(reason.clone()),
            Solid::Ready(_) if self.truncate && (self.enqueue_all || !self.queue.is_empty()) => {
                RoomClipPhase::Computing {
                    done: self.done.len(),
                    total: self.done.len() + self.queue.len(),
                }
            }
            Solid::Ready(_) => RoomClipPhase::Ready,
        };
        RoomClipStatus {
            generation: self.generation,
            truncate: self.truncate,
            phase,
            cut: self.counts.cut,
            hidden: self.counts.hidden,
            whole: self.counts.whole,
            skipped: self.counts.skipped,
            carriers: self.carriers,
            components: self.components,
            found: self.known.len(),
        }
    }

    /// 面板网格全部有了结论就建房间体。缺一块就不截：宁可不截，也不按半个房间截。
    fn settle_solid(&mut self, assets: &AssetServer, meshes: &Assets<Mesh>) {
        if !matches!(self.solid, Solid::Loading { .. }) {
            return;
        }
        let mut done = 0;
        let mut failed = 0;
        for panel in &self.panels {
            if meshes.contains(&panel.handle) {
                done += 1;
            } else if !matches!(
                assets.get_load_state(&panel.handle),
                Some(LoadState::Loading | LoadState::NotLoaded)
            ) {
                done += 1;
                failed += 1;
            }
        }
        if done < self.panels.len() {
            self.solid = Solid::Loading { done };
            return;
        }
        if failed > 0 {
            self.solid = Solid::Unavailable(format!("房间体缺 {failed} 块面板网格"));
            return;
        }
        self.solid = match build_solid(&self.panels, meshes) {
            Some(solid) => {
                self.enqueue_all = self.truncate;
                Solid::Ready(solid)
            }
            None => Solid::Unavailable("在册面板的网格里没有可用的三角形".into()),
        };
    }

    /// 把处理过的直段整批复原：变换还原、隐藏撤销、补出来的实体撤掉。排着队的一并作废。
    fn restore_all(&mut self, commands: &mut Commands, tubes: &mut Tubes) {
        for (entity, outcome) in self.done.drain() {
            match outcome {
                Outcome::Hidden => {
                    if let Ok((_, _, _, _, _, _, _, mut visibility)) = tubes.get_mut(entity) {
                        *visibility = Visibility::Inherited;
                    }
                }
                Outcome::Cut(extras) => {
                    if let Ok((_, body, _, _, _, mut mesh, mut transform, _)) =
                        tubes.get_mut(entity)
                    {
                        *transform = body.base;
                        if let Some(line) = &body.invalid_line {
                            mesh.0 = line.clone();
                        }
                    }
                    for extra in extras {
                        if let Ok(mut extra) = commands.get_entity(extra) {
                            extra.despawn();
                        }
                    }
                }
                Outcome::Whole | Outcome::Skipped => {}
            }
        }
        self.queue.clear();
        self.known.clear();
        self.counts = Counts::default();
    }

    fn teardown(&mut self, commands: &mut Commands, tubes: &mut Tubes) {
        self.restore_all(commands, tubes);
        if let Ok(mut layer) = commands.get_entity(self.layer) {
            layer.despawn();
        }
    }

    /// 截这一拍配额内的直段。网格还没加载完的留到下一拍。
    fn process(
        &mut self,
        commands: &mut Commands,
        tubes: &mut Tubes,
        meshes: &mut Assets<Mesh>,
        assets: &AssetServer,
        prisms: &mut HashMap<AssetId<Mesh>, Prisms>,
        section: &Handle<StandardMaterial>,
    ) {
        let Solid::Ready(solid) = &self.solid else {
            return;
        };
        let budget = TUBES_PER_FRAME.min(self.queue.len());
        let mut waiting = Vec::new();
        for _ in 0..budget {
            let Some(entity) = self.queue.pop_front() else {
                break;
            };
            let Ok((_, body, model, pick, child_of, mut mesh, mut transform, mut visibility)) =
                tubes.get_mut(entity)
            else {
                // 这段直段已经随模型卸掉了。
                self.known.remove(&entity);
                continue;
            };
            let prism = match &body.invalid_line {
                Some(_) => Some(Prism::UNIT_LINE),
                None => match prism_of(&mesh.0, meshes, assets, prisms) {
                    PrismLookup::Pending => {
                        waiting.push(entity);
                        continue;
                    }
                    PrismLookup::Found(found) => found.pick(body.kind),
                },
            };
            let Some(prism) = prism else {
                self.counts.skipped += 1;
                self.done.insert(entity, Outcome::Skipped);
                continue;
            };
            let (a, b) = axis_of(body, prism);
            let length_mm = a.distance(b) as f32;
            let outcome = match plan_of(solid.inside_intervals(a, b)) {
                Plan::Whole => {
                    self.counts.whole += 1;
                    Outcome::Whole
                }
                Plan::Hidden => {
                    *visibility = Visibility::Hidden;
                    self.counts.hidden += 1;
                    Outcome::Hidden
                }
                Plan::Cut(pieces) => {
                    let extras = cut(
                        commands,
                        meshes,
                        section,
                        CutTarget {
                            body,
                            model,
                            pick: *pick,
                            parent: child_of.parent(),
                            mesh: &mut mesh,
                            transform: &mut transform,
                        },
                        prism,
                        &pieces,
                        length_mm,
                    );
                    self.counts.cut += 1;
                    Outcome::Cut(extras)
                }
            };
            self.done.insert(entity, outcome);
        }
        self.queue.extend(waiting);
    }
}

enum PrismLookup {
    /// 网格还在加载。
    Pending,
    /// 有结论了；网格没加载成时三根轴都不算。
    Found(Prisms),
}

fn prism_of(
    handle: &Handle<Mesh>,
    meshes: &Assets<Mesh>,
    assets: &AssetServer,
    cache: &mut HashMap<AssetId<Mesh>, Prisms>,
) -> PrismLookup {
    if let Some(found) = cache.get(&handle.id()) {
        return PrismLookup::Found(*found);
    }
    match meshes.get(handle) {
        Some(mesh) => {
            let prisms = Prisms::of(mesh);
            cache.insert(handle.id(), prisms);
            PrismLookup::Found(prisms)
        }
        None => match assets.get_load_state(handle) {
            Some(LoadState::Loading | LoadState::NotLoaded) => PrismLookup::Pending,
            _ => PrismLookup::Found(Prisms::default()),
        },
    }
}

/// 一段要截的直段：子实体上截断要读写的那几样。
struct CutTarget<'a> {
    body: &'a TubeBody,
    model: &'a ModelMesh,
    pick: ModelPickTarget,
    parent: Entity,
    mesh: &'a mut Mesh3d,
    transform: &'a mut Transform,
}

/// 截一段：第一截改写子实体本身，其余每截补一个兄弟实体，每个截点（落在管端之间的）盖一片断口。
/// 交回补出来的实体。兄弟段只带 `ModelMesh` 出生、不抄材质：材质由同帧的 `apply_materials`
/// 按当前外观补上（两种外观是两种组件类型，这里不该知道是哪种）。
fn cut(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    section: &Handle<StandardMaterial>,
    target: CutTarget<'_>,
    prism: Prism,
    pieces: &[[f64; 2]],
    length_mm: f32,
) -> Vec<Entity> {
    let CutTarget {
        body,
        model,
        pick,
        parent,
        mesh,
        transform,
    } = target;
    let prototype = mesh.0.clone();
    let mut extras = Vec::new();
    for (index, &[t0, t1]) in pieces.iter().enumerate() {
        let piece = piece_transform(&body.base, prism, t0, t1);
        // 无效直段换一份截后长度的虚线；只压短原型的话，100 / 80 mm 的节距跟着变密。
        let piece_mesh = body.invalid_line.as_ref().map(|_| {
            meshes.add(invalid_tubi_fallback_line_mesh(
                length_mm * (t1 - t0) as f32,
            ))
        });
        if index == 0 {
            *transform = piece;
            if let Some(line) = piece_mesh {
                mesh.0 = line;
            }
        } else {
            let mut sibling = commands.spawn((
                Mesh3d(piece_mesh.unwrap_or_else(|| prototype.clone())),
                piece,
                model.clone(),
                pick,
                TubeExtra,
                ChildOf(parent),
            ));
            if body.invalid_line.is_some() {
                sibling.insert((NotShadowCaster, NotShadowReceiver));
            }
            extras.push(sibling.id());
        }
        // 断口只给实体管：虚线表示「画不出实体管」，没有截面可言。
        if body.invalid_line.is_none() {
            for t in [t0, t1] {
                if t > 0.0 && t < 1.0 {
                    let cap = commands.spawn((
                        Mesh3d(prototype.clone()),
                        MeshMaterial3d(section.clone()),
                        cap_transform(&body.base, prism, t, length_mm),
                        pick,
                        TubeExtra,
                        NotShadowCaster,
                        ChildOf(parent),
                    ));
                    extras.push(cap.id());
                }
            }
        }
    }
    extras
}

pub(crate) fn triangles_of(mesh: &Mesh) -> Option<(Vec<Vec3>, Vec<u32>)> {
    if mesh.primitive_topology() != PrimitiveTopology::TriangleList {
        return None;
    }
    let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        return None;
    };
    let vertices: Vec<Vec3> = positions.iter().map(|p| Vec3::from_array(*p)).collect();
    let indices = match mesh.indices() {
        Some(indices) => indices.iter().map(|index| index as u32).collect(),
        None => (0..vertices.len() as u32).collect(),
    };
    Some((vertices, indices))
}

/// 面板网格 → 房间体。一块面板的网格建不出三角网（全是退化三角）就跳过，与 gen-model 归属计算
/// 跳过它是同一个口径；一块都建不出来为 `None`。
fn build_solid(panels: &[PanelMesh], meshes: &Assets<Mesh>) -> Option<RoomSolid> {
    RoomSolid::new(panels.iter().filter_map(|panel| {
        let mesh = meshes.get(&panel.handle)?;
        let (vertices, indices) = triangles_of(mesh)?;
        panel_tri_mesh(&vertices, &indices, panel.matrix)
    }))
}

/// 执行宿主交来的房间体指令（挂上 / 拨开关 / 撤掉）。
pub(crate) fn apply_room_clip_ops(
    mut commands: Commands,
    mut view: ResMut<View3d>,
    mut clip: ResMut<RoomClip>,
    assets: Res<AssetServer>,
    xray: Res<XRayMaterial>,
    mut tubes: Tubes,
) {
    if view.room_clip_ops.is_empty() {
        return;
    }
    let ops: Vec<RoomClipOp> = view.room_clip_ops.drain(..).collect();
    for op in ops {
        match op {
            RoomClipOp::Set(batch) => {
                if let Some(mut old) = clip.active.take() {
                    old.teardown(&mut commands, &mut tubes);
                }
                clip.active = Some(Active::spawn(&mut commands, &assets, &xray.0, batch));
            }
            RoomClipOp::Truncate(on) => {
                if let Some(active) = clip.active.as_mut()
                    && active.truncate != on
                {
                    active.restore_all(&mut commands, &mut tubes);
                    active.truncate = on;
                    active.enqueue_all = on && matches!(active.solid, Solid::Ready(_));
                }
            }
            RoomClipOp::Clear => {
                if let Some(mut old) = clip.active.take() {
                    old.teardown(&mut commands, &mut tubes);
                }
            }
        }
    }
    view.room_clip_status = clip.active.as_ref().map(Active::status);
}

/// 每帧推进：面板网格齐了就建房间体，把候选直段排进来，截这一拍的配额，发回执。
#[allow(clippy::too_many_arguments)]
pub(crate) fn advance_room_clip(
    mut commands: Commands,
    mut view: ResMut<View3d>,
    mut clip: ResMut<RoomClip>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    section: Res<SectionMaterial>,
    added: Query<Entity, Added<TubeBody>>,
    mut tubes: Tubes,
) {
    let RoomClip { active, prisms } = &mut *clip;
    let Some(active) = active.as_mut() else {
        return;
    };
    active.settle_solid(&assets, &meshes);
    if active.truncate && matches!(active.solid, Solid::Ready(_)) {
        if std::mem::take(&mut active.enqueue_all) {
            for (entity, _, model, ..) in &tubes {
                if active.targets.contains(&model.refno) && active.known.insert(entity) {
                    active.queue.push_back(entity);
                }
            }
        } else {
            // 重画 / 取回工作之后新装进来的直段：接着按这间房截。
            for entity in &added {
                if let Ok((_, _, model, ..)) = tubes.get(entity)
                    && active.targets.contains(&model.refno)
                    && active.known.insert(entity)
                {
                    active.queue.push_back(entity);
                }
            }
        }
        active.process(
            &mut commands,
            &mut tubes,
            &mut meshes,
            &assets,
            prisms,
            &section.0,
        );
    }
    view.room_clip_status = Some(active.status());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ModelRoot;
    use bevy::asset::AssetPlugin;
    use bevy::render::mesh::Indices;
    use bevy::render::render_asset::RenderAssetUsages;

    /// 把一个简单多边形沿 z 拉成闭合棱柱网格。盖面按顶点 0 扇形三角化。
    fn prism_mesh(polygon: &[[f32; 2]], z0: f32, z1: f32) -> Mesh {
        let n = polygon.len() as u32;
        let mut positions: Vec<[f32; 3]> = polygon.iter().map(|[x, y]| [*x, *y, z0]).collect();
        positions.extend(polygon.iter().map(|[x, y]| [*x, *y, z1]));
        let mut indices = Vec::new();
        for i in 1..n - 1 {
            indices.extend([0, i + 1, i]);
            indices.extend([n, n + i, n + i + 1]);
        }
        for i in 0..n {
            let j = (i + 1) % n;
            indices.extend([i, j, n + j]);
            indices.extend([i, n + j, n + i]);
        }
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
            .with_inserted_indices(Indices::U32(indices))
    }

    /// `CylinderV3` 的形状：半径 1、z ∈ [0, 1]。
    fn cylinder_v3() -> Mesh {
        let ring: Vec<[f32; 2]> = (0..12)
            .map(|i| {
                let a = i as f32 * std::f32::consts::TAU / 12.0;
                [a.cos(), a.sin()]
            })
            .collect();
        prism_mesh(&ring, 0.0, 1.0)
    }

    /// `BoxV1` 的形状：居中单位盒。
    fn box_v1() -> Mesh {
        prism_mesh(
            &[[-0.5, -0.5], [0.5, -0.5], [0.5, 0.5], [-0.5, 0.5]],
            -0.5,
            0.5,
        )
    }

    fn room_box() -> Mesh {
        prism_mesh(
            &[[0.0, 0.0], [1000.0, 0.0], [1000.0, 1000.0], [0.0, 1000.0]],
            0.0,
            1000.0,
        )
    }

    fn l_room() -> Mesh {
        prism_mesh(
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
        )
    }

    /// 一段从 `a` 到 `b`、半径 30 mm 的圆管的模型根变换（原型局部 +Z 对到管轴，长度进 scale.z）。
    fn tube_root(a: Vec3, b: Vec3) -> Transform {
        Transform {
            translation: a,
            rotation: Quat::from_rotation_arc(Vec3::Z, (b - a).normalize()),
            scale: Vec3::new(30.0, 30.0, a.distance(b)),
        }
    }

    /// FTUB 烘焙网格的形状（R220 实测）：实心方盒，x ∈ [0, length]、y ∈ [-50, 50]、z ∈ [0, 50]。
    fn ftub_box(length: f32) -> Mesh {
        turned(
            prism_mesh(
                &[[-50.0, 0.0], [50.0, 0.0], [50.0, 50.0], [-50.0, 50.0]],
                0.0,
                length,
            ),
            0,
        )
    }

    /// 把沿 z 拉出来的棱柱轮换成沿 `axis` 的（0 = X、1 = Y），轮换不改手性。
    fn turned(mut mesh: Mesh, axis: usize) -> Mesh {
        if let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION)
        {
            for p in positions.iter_mut() {
                let [x, y, z] = *p;
                *p = match axis {
                    0 => [z, x, y],
                    1 => [y, z, x],
                    _ => [x, y, z],
                };
            }
        }
        mesh
    }

    fn segment(mesh: &Mesh) -> Option<Prism> {
        Prisms::of(mesh).pick(StraightKind::Segment)
    }

    fn close(a: Vec3, b: Vec3) -> bool {
        a.distance(b) < 0.01
    }

    #[test]
    fn prototypes_are_read_as_prisms_along_local_z() {
        assert_eq!(
            segment(&cylinder_v3()),
            Some(Prism {
                axis: 2,
                a0: 0.0,
                a1: 1.0,
                centre: Vec3::ZERO
            })
        );
        // 风管原型居中：z 从 -0.5 起。写死 [0, 1] 就会截偏半根。
        assert_eq!(
            segment(&box_v1()),
            Some(Prism {
                axis: 2,
                a0: -0.5,
                a1: 0.5,
                centre: Vec3::ZERO
            })
        );
        // 锥：两头截面不一样大。
        let mut cone = cylinder_v3();
        if let Some(VertexAttributeValues::Float32x3(positions)) =
            cone.attribute_mut(Mesh::ATTRIBUTE_POSITION)
        {
            for p in positions.iter_mut().filter(|p| p[2] > 0.5) {
                p[0] *= 0.5;
                p[1] *= 0.5;
            }
        }
        assert_eq!(segment(&cone), None);
        // 中间有顶点：开过孔、布尔过的管身。
        let mut holed = box_v1();
        if let Some(VertexAttributeValues::Float32x3(positions)) =
            holed.attribute_mut(Mesh::ATTRIBUTE_POSITION)
        {
            positions.push([0.0, 0.0, 0.1]);
        }
        assert_eq!(segment(&holed), None);
    }

    /// 直管件按最长的棱柱轴认管轴：FTUB 那样的实心方盒三根轴都算棱柱，Z 只有 50 mm 高，管轴是 X；
    /// 沿 Y 的圆管只有 Y 一根算。短而粗的方盒判不出方向，带法兰的风管（STRT）哪根都不算——都不截。
    /// 直段照旧只认 Z。
    #[test]
    fn straight_components_take_the_longest_prism_axis() {
        let slab = Prisms::of(&ftub_box(1500.0));
        assert_eq!(
            slab.pick(StraightKind::Component),
            Some(Prism {
                axis: 0,
                a0: 0.0,
                a1: 1500.0,
                centre: Vec3::new(0.0, 0.0, 25.0)
            })
        );
        assert_eq!(slab.pick(StraightKind::Segment).map(|prism| prism.axis), Some(2));

        let pipe = Prisms::of(&turned(cylinder_v3(), 1));
        assert_eq!(pipe.pick(StraightKind::Component).map(|prism| prism.axis), Some(1));
        assert_eq!(pipe.pick(StraightKind::Segment), None);

        assert_eq!(
            Prisms::of(&ftub_box(150.0)).pick(StraightKind::Component),
            None,
            "150 mm 长不到截面 100 mm 的两倍"
        );
        assert_eq!(Prisms::of(&box_v1()).pick(StraightKind::Component), None);

        // 风管两端各一圈法兰：离端面 6 mm 处有顶点，截面比管身大。
        let mut flanged = ftub_box(1200.0);
        if let Some(VertexAttributeValues::Float32x3(positions)) =
            flanged.attribute_mut(Mesh::ATTRIBUTE_POSITION)
        {
            positions.extend([[6.0, -60.0, -10.0], [1194.0, 60.0, 60.0]]);
        }
        assert_eq!(Prisms::of(&flanged), Prisms::default());
    }

    /// 截 [0.3, 0.7]：原型两个端面的截面中心过「模型根 × 截段变换」后，落在管轴的 0.3 与 0.7 处，
    /// 误差不到 0.01 mm。圆管与居中的风管原型都一样。
    #[test]
    fn a_piece_maps_the_prototype_ends_onto_the_kept_span() {
        let a = Vec3::new(1000.0, 2000.0, 3000.0);
        let b = Vec3::new(4000.0, 2500.0, 3200.0);
        let root = tube_root(a, b);
        let bases = [
            Transform::IDENTITY,
            Transform::from_translation(Vec3::new(0.0, 0.0, 0.25))
                .with_scale(Vec3::new(1.0, 1.0, 0.5)),
        ];
        for (mesh, base) in [(cylinder_v3(), bases[0]), (box_v1(), bases[0]), (cylinder_v3(), bases[1])] {
            let prism = segment(&mesh).unwrap();
            let body = TubeBody::solid(base, root);
            let (axis_a, axis_b) = axis_of(&body, prism);
            let (axis_a, axis_b) = (axis_a.as_vec3(), axis_b.as_vec3());
            let piece = piece_transform(&base, prism, 0.3, 0.7);
            let start = root.transform_point(piece.transform_point(prism.point(prism.a0)));
            let end = root.transform_point(piece.transform_point(prism.point(prism.a1)));
            assert!(close(start, axis_a.lerp(axis_b, 0.3)), "{start} vs {}", axis_a.lerp(axis_b, 0.3));
            assert!(close(end, axis_a.lerp(axis_b, 0.7)), "{end} vs {}", axis_a.lerp(axis_b, 0.7));
        }
    }

    /// 断口以截点为中心、2 mm 厚，截面比管子大 2%。
    #[test]
    fn a_cap_is_centred_on_the_cut_and_two_millimetres_thick() {
        let a = Vec3::ZERO;
        let b = Vec3::new(0.0, 0.0, 1000.0);
        let root = tube_root(a, b);
        let prism = segment(&cylinder_v3()).unwrap();
        let cap = cap_transform(&Transform::IDENTITY, prism, 0.5, 1000.0);
        let near = root.transform_point(cap.transform_point(Vec3::new(0.0, 0.0, 0.0)));
        let far = root.transform_point(cap.transform_point(Vec3::new(0.0, 0.0, 1.0)));
        assert!(close(near, Vec3::new(0.0, 0.0, 499.0)), "{near}");
        assert!(close(far, Vec3::new(0.0, 0.0, 501.0)), "{far}");
        let rim = root.transform_point(cap.transform_point(Vec3::new(1.0, 0.0, 0.5)));
        assert!((Vec2::new(rim.x, rim.y).length() - 30.6).abs() < 0.01, "{rim}");
    }

    /// 直管件沿局部 X 截（模型根只转了个方位角）：两个端面的截面中心过「模型根 × 截段变换」后
    /// 落在管轴的 0.3 与 0.7 处，截面不动；断口以截点为中心、沿管轴 2 mm 厚。
    #[test]
    fn a_component_piece_is_remapped_along_its_own_axis() {
        let root = Transform::from_translation(Vec3::new(1000.0, 2000.0, 3000.0))
            .with_rotation(Quat::from_rotation_z(0.7));
        let prism = Prisms::of(&ftub_box(1500.0))
            .pick(StraightKind::Component)
            .unwrap();
        let body = TubeBody::component(Transform::IDENTITY, root);
        let (axis_a, axis_b) = axis_of(&body, prism);
        let (axis_a, axis_b) = (axis_a.as_vec3(), axis_b.as_vec3());
        assert!((axis_a.distance(axis_b) - 1500.0).abs() < 0.01);

        let piece = piece_transform(&body.base, prism, 0.3, 0.7);
        let start = root.transform_point(piece.transform_point(prism.point(prism.a0)));
        let end = root.transform_point(piece.transform_point(prism.point(prism.a1)));
        assert!(close(start, axis_a.lerp(axis_b, 0.3)), "{start}");
        assert!(close(end, axis_a.lerp(axis_b, 0.7)), "{end}");
        assert_eq!((piece.scale.y, piece.scale.z), (1.0, 1.0), "截面不动");

        let cap = cap_transform(&body.base, prism, 0.5, 1500.0);
        let near = root.transform_point(cap.transform_point(prism.point(prism.a0)));
        let far = root.transform_point(cap.transform_point(prism.point(prism.a1)));
        assert!((near.distance(far) - CAP_THICKNESS_MM).abs() < 0.01, "{near} {far}");
        assert!(close((near + far) * 0.5, axis_a.lerp(axis_b, 0.5)));
    }

    fn test_app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Mesh>()
            .insert_resource(crate::blank_view())
            .insert_resource(XRayMaterial(Handle::default()))
            .insert_resource(SectionMaterial(Handle::default()))
            .init_resource::<RoomClip>()
            .add_systems(Update, (apply_room_clip_ops, advance_room_clip).chain());
        app
    }

    /// 装一间房：面板网格直接进 `Assets<Mesh>`（不经文件），面板局部系就是毫米世界系。
    fn activate(app: &mut App, rooms: Vec<Mesh>, carriers: &[RefU64], truncate: bool) {
        activate_with(app, rooms, carriers, &[], truncate);
    }

    fn activate_with(
        app: &mut App,
        rooms: Vec<Mesh>,
        carriers: &[RefU64],
        components: &[RefU64],
        truncate: bool,
    ) {
        let panels: Vec<PanelMesh> = rooms
            .into_iter()
            .map(|mesh| PanelMesh {
                handle: app.world_mut().resource_mut::<Assets<Mesh>>().add(mesh),
                matrix: Mat4::IDENTITY,
            })
            .collect();
        let layer = app
            .world_mut()
            .spawn((Transform::IDENTITY, Visibility::Visible, RoomVolumeLayer))
            .id();
        app.world_mut().resource_mut::<RoomClip>().active = Some(Active::new(
            7,
            layer,
            panels,
            carriers.iter().copied(),
            components.iter().copied(),
            truncate,
            None,
        ));
    }

    fn add_mesh(app: &mut App, mesh: Mesh) -> Handle<Mesh> {
        app.world_mut().resource_mut::<Assets<Mesh>>().add(mesh)
    }

    /// 在场景里放一段直段：模型根 + 一个挂着 `TubeBody` 的网格子实体。交回子实体。
    fn spawn_tube(app: &mut App, refno: RefU64, root: Transform, mesh: Handle<Mesh>, invalid: bool) -> Entity {
        let body = if invalid {
            TubeBody::invalid(Transform::IDENTITY, root, mesh.clone())
        } else {
            TubeBody::solid(Transform::IDENTITY, root)
        };
        spawn_body(app, refno, root, mesh, body)
    }

    fn spawn_body(app: &mut App, refno: RefU64, root: Transform, mesh: Handle<Mesh>, body: TubeBody) -> Entity {
        let line = body.invalid_line.is_some();
        let world = app.world_mut();
        let parent = world
            .spawn((root, Visibility::Visible, ModelRoot { refno, owner: refno }))
            .id();
        world
            .spawn((
                Mesh3d(mesh),
                MeshMaterial3d::<StandardMaterial>(Handle::default()),
                Transform::IDENTITY,
                ModelMesh {
                    refno,
                    owner: refno,
                    colour: [0; 4],
                    line,
                    room_shell: false,
                },
                ModelPickTarget { refno },
                body,
                ChildOf(parent),
            ))
            .id()
    }

    fn extras(app: &mut App) -> (usize, usize) {
        let world = app.world_mut();
        let mut pieces = world.query_filtered::<(), (With<TubeExtra>, With<ModelMesh>)>();
        let pieces = pieces.iter(world).count();
        let mut all = world.query_filtered::<(), With<TubeExtra>>();
        (pieces, all.iter(world).count() - pieces)
    }

    fn status(app: &App) -> RoomClipStatus {
        app.world().resource::<View3d>().room_clip_status().expect("有房间体就有回执")
    }

    fn transform_of(app: &App, entity: Entity) -> Transform {
        *app.world().entity(entity).get::<Transform>().unwrap()
    }

    /// 穿一面墙：只留墙内那一截（子实体本身改写），墙上一片断口；根的可见性不动。整段在内的原样，
    /// 整段在外的藏起来，不是截断候选的不碰。
    #[test]
    fn crossing_tubes_are_cut_at_the_wall_and_outside_ones_hidden() {
        let (bran, other) = (RefU64::from(11), RefU64::from(12));
        let mut app = test_app();
        let cylinder = add_mesh(&mut app, cylinder_v3());
        activate(&mut app, vec![room_box()], &[bran], true);
        let crossing = spawn_tube(
            &mut app,
            bran,
            tube_root(Vec3::new(500.0, 500.0, 500.0), Vec3::new(1500.0, 500.0, 500.0)),
            cylinder.clone(),
            false,
        );
        let inside = spawn_tube(
            &mut app,
            bran,
            tube_root(Vec3::new(100.0, 300.0, 500.0), Vec3::new(900.0, 300.0, 500.0)),
            cylinder.clone(),
            false,
        );
        let outside = spawn_tube(
            &mut app,
            bran,
            tube_root(Vec3::new(1500.0, 500.0, 500.0), Vec3::new(2500.0, 500.0, 500.0)),
            cylinder.clone(),
            false,
        );
        let foreign = spawn_tube(
            &mut app,
            other,
            tube_root(Vec3::new(500.0, 700.0, 500.0), Vec3::new(1500.0, 700.0, 500.0)),
            cylinder,
            false,
        );

        app.update();
        app.update();

        let kept = transform_of(&app, crossing);
        assert!((kept.scale.z - 0.5).abs() < 1e-6, "{kept:?}");
        assert_eq!(extras(&mut app), (0, 1), "一截、一片断口");
        assert_eq!(transform_of(&app, inside), Transform::IDENTITY);
        assert_eq!(
            app.world().entity(outside).get::<Visibility>(),
            Some(&Visibility::Hidden)
        );
        let outside_root = app.world().entity(outside).get::<ChildOf>().unwrap().parent();
        assert_eq!(
            app.world().entity(outside_root).get::<Visibility>(),
            Some(&Visibility::Visible),
            "根不动：eye 与隔离快照照旧"
        );
        assert_eq!(transform_of(&app, foreign), Transform::IDENTITY);
        let status = status(&app);
        assert_eq!(status.phase, RoomClipPhase::Ready);
        assert_eq!(
            (status.cut, status.hidden, status.whole, status.skipped),
            (1, 1, 1, 0)
        );
        assert_eq!(status.generation, 7);
    }

    /// L 形凹房：穿出缺角再穿回来，一段直段得两截——子实体改写一截、补一个兄弟段、两片断口。
    /// 关掉截断，全部复原。
    #[test]
    fn a_concave_room_gives_two_pieces_and_switching_off_restores_everything() {
        let bran = RefU64::from(11);
        let mut app = test_app();
        let cylinder = add_mesh(&mut app, cylinder_v3());
        activate(&mut app, vec![l_room()], &[bran], true);
        let tube = spawn_tube(
            &mut app,
            bran,
            tube_root(Vec3::new(1800.0, 500.0, 500.0), Vec3::new(500.0, 1800.0, 500.0)),
            cylinder,
            false,
        );
        app.update();
        app.update();
        let first = transform_of(&app, tube);
        assert!((first.scale.z - 500.0 / 1300.0).abs() < 1e-4, "{first:?}");
        assert_eq!(extras(&mut app), (1, 2));

        app.world_mut()
            .resource_mut::<View3d>()
            .set_room_truncation(false);
        app.update();
        assert_eq!(transform_of(&app, tube), Transform::IDENTITY);
        assert_eq!(extras(&mut app), (0, 0));
        let status = status(&app);
        assert!(!status.truncate);
        assert_eq!(status.phase, RoomClipPhase::Ready);
        assert_eq!(status.cut, 0);

        // 再拨开：照样截回来。
        app.world_mut()
            .resource_mut::<View3d>()
            .set_room_truncation(true);
        app.update();
        app.update();
        assert_eq!(extras(&mut app), (1, 2));
    }

    /// 截断生效期间才装进来的直段（重画、取回工作）照样截。
    #[test]
    fn tubes_loaded_later_are_cut_too() {
        let bran = RefU64::from(11);
        let mut app = test_app();
        let cylinder = add_mesh(&mut app, cylinder_v3());
        activate(&mut app, vec![room_box()], &[bran], true);
        app.update();
        assert_eq!(status(&app).phase, RoomClipPhase::Ready);
        let late = spawn_tube(
            &mut app,
            bran,
            tube_root(Vec3::new(-500.0, 500.0, 500.0), Vec3::new(500.0, 500.0, 500.0)),
            cylinder,
            false,
        );
        app.update();
        app.update();
        let kept = transform_of(&app, late);
        assert!((kept.scale.z - 0.5).abs() < 1e-6, "{kept:?}");
        assert!((kept.translation.z - 0.5).abs() < 1e-6, "{kept:?}");
        assert_eq!(status(&app).cut, 1);
    }

    /// 无效直段换一份截后长度的虚线网格、不加断口；复原时换回原来那份。
    #[test]
    fn an_invalid_tube_gets_a_shorter_dash_mesh_and_no_cap() {
        let bran = RefU64::from(11);
        let mut app = test_app();
        let line = add_mesh(&mut app, invalid_tubi_fallback_line_mesh(1000.0));
        activate(&mut app, vec![room_box()], &[bran], true);
        let tube = spawn_tube(
            &mut app,
            bran,
            tube_root(Vec3::new(500.0, 500.0, 500.0), Vec3::new(1500.0, 500.0, 500.0)),
            line.clone(),
            true,
        );
        app.update();
        app.update();
        let now = app.world().entity(tube).get::<Mesh3d>().unwrap().0.clone();
        assert_ne!(now, line);
        assert_eq!(extras(&mut app), (0, 0), "虚线没有断口");

        app.world_mut().resource_mut::<View3d>().clear_room_volume();
        app.update();
        assert_eq!(app.world().entity(tube).get::<Mesh3d>().unwrap().0, line);
        assert_eq!(transform_of(&app, tube), Transform::IDENTITY);
        assert_eq!(app.world().resource::<View3d>().room_clip_status(), None);
    }

    /// 网格不是沿局部 Z 的棱柱：不截，原样画，计进「未能截断」。
    #[test]
    fn a_non_prism_tube_is_left_alone_and_counted() {
        let bran = RefU64::from(11);
        let mut app = test_app();
        let mut odd = cylinder_v3();
        if let Some(VertexAttributeValues::Float32x3(positions)) =
            odd.attribute_mut(Mesh::ATTRIBUTE_POSITION)
        {
            positions.push([0.0, 0.0, 0.5]);
        }
        let odd = add_mesh(&mut app, odd);
        activate(&mut app, vec![room_box()], &[bran], true);
        let tube = spawn_tube(
            &mut app,
            bran,
            tube_root(Vec3::new(500.0, 500.0, 500.0), Vec3::new(1500.0, 500.0, 500.0)),
            odd,
            false,
        );
        app.update();
        app.update();
        assert_eq!(transform_of(&app, tube), Transform::IDENTITY);
        assert_eq!(status(&app).skipped, 1);
    }

    /// 直管件（FTUB 那样的实心方盒，管轴在局部 X 上）按自己名下的 refno 认，沿管轴在墙上截断、
    /// 盖一片断口；截面方向（Y、Z）不动。所属分支不在候选里也照截。
    #[test]
    fn straight_components_are_cut_along_their_own_axis() {
        let ftub = RefU64::from(31);
        let mut app = test_app();
        let slab = add_mesh(&mut app, ftub_box(1000.0));
        activate_with(&mut app, vec![room_box()], &[], &[ftub], true);
        // 管轴转到世界 +Y：从 (500, 500) 到 (500, 1500)，墙在 y = 1000，留前一半。
        let root = Transform::from_translation(Vec3::new(500.0, 500.0, 400.0))
            .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2));
        let part = spawn_body(
            &mut app,
            ftub,
            root,
            slab,
            TubeBody::component(Transform::IDENTITY, root),
        );
        app.update();
        app.update();

        let kept = transform_of(&app, part);
        assert!((kept.scale.x - 0.5).abs() < 1e-6, "{kept:?}");
        assert_eq!((kept.scale.y, kept.scale.z), (1.0, 1.0), "{kept:?}");
        assert_eq!(extras(&mut app), (0, 1), "一截、一片断口");
        let status = status(&app);
        assert_eq!(
            (status.cut, status.hidden, status.whole, status.skipped),
            (1, 0, 0, 0)
        );
        assert_eq!((status.carriers, status.components, status.found), (0, 1, 1));
    }

    /// 候选一个都没装进场景（所在的 SITE / ZONE 没打开）：截不到任何一段，回执照实说候选几根、
    /// 场景里找到 0 段，宿主据此说原因。装进来之后照样补截。
    #[test]
    fn the_receipt_counts_candidates_and_what_the_scene_has_of_them() {
        let (bran, ftub) = (RefU64::from(11), RefU64::from(31));
        let mut app = test_app();
        let cylinder = add_mesh(&mut app, cylinder_v3());
        activate_with(&mut app, vec![room_box()], &[bran, bran], &[ftub], true);
        app.update();
        app.update();
        let empty = status(&app);
        assert_eq!(empty.phase, RoomClipPhase::Ready);
        assert_eq!(
            (empty.carriers, empty.components, empty.found, empty.cut),
            (1, 1, 0, 0),
            "候选去重；场景里一段都没有"
        );

        spawn_tube(
            &mut app,
            bran,
            tube_root(Vec3::new(500.0, 500.0, 500.0), Vec3::new(1500.0, 500.0, 500.0)),
            cylinder,
            false,
        );
        app.update();
        app.update();
        let later = status(&app);
        assert_eq!((later.found, later.cut), (1, 1));
    }

    /// 截断关着进房间视图：房间体照样建，直段一段不碰。
    #[test]
    fn with_truncation_off_nothing_is_touched() {
        let bran = RefU64::from(11);
        let mut app = test_app();
        let cylinder = add_mesh(&mut app, cylinder_v3());
        activate(&mut app, vec![room_box()], &[bran], false);
        let tube = spawn_tube(
            &mut app,
            bran,
            tube_root(Vec3::new(500.0, 500.0, 500.0), Vec3::new(1500.0, 500.0, 500.0)),
            cylinder,
            false,
        );
        app.update();
        app.update();
        assert_eq!(transform_of(&app, tube), Transform::IDENTITY);
        let status = status(&app);
        assert_eq!((status.phase, status.truncate), (RoomClipPhase::Ready, false));
    }

    fn panel_record(min: [f32; 3], max: [f32; 3]) -> GeomInstQuery {
        use plant_room::parry3d::bounding_volume::Aabb;
        use plant_room::parry3d::math::Point;
        GeomInstQuery {
            refno: aios_core::RefnoEnum::Refno(RefU64::from(1)),
            old_refno: None,
            owner: aios_core::RefnoEnum::Refno(RefU64::from(1)),
            world_aabb: Aabb::new(
                Point::new(min[0], min[1], min[2]),
                Point::new(max[0], max[1], max[2]),
            ),
            world_trans: Transform::IDENTITY,
            insts: Vec::new(),
            has_neg: false,
            generic: "PANE".into(),
            pts: None,
            date: None,
        }
    }

    /// 取景用的房间体盒子是全部面板盒子的并集；坏盒子（NaN、翻转）跳过。挂上房间体之后，
    /// 「视图适应」对准的就是它（换算与模型包围盒同一条 `scene_bounds_from_mm`）。
    #[test]
    fn panel_bounds_union_every_usable_record() {
        assert_eq!(panel_bounds_mm(&[]), None);
        let records = vec![
            panel_record([0.0, 0.0, 0.0], [1000.0, 500.0, 3000.0]),
            panel_record([900.0, -200.0, 0.0], [2000.0, 400.0, 2500.0]),
            panel_record([f32::NAN; 3], [1.0; 3]),
            panel_record([5.0; 3], [1.0; 3]),
        ];
        let union = ([0.0, -200.0, 0.0], [2000.0, 500.0, 3000.0]);
        assert_eq!(panel_bounds_mm(&records), Some(union));

        let mut view = crate::blank_view();
        view.set_room_volume(RoomVolumeBatch {
            generation: 1,
            panels: records,
            carriers: Vec::new(),
            components: Vec::new(),
            truncate: true,
            unavailable: None,
        });
        assert_eq!(
            view.room_frame_bounds,
            scene_bounds_from_mm(union.0, union.1)
        );
        view.clear_room_volume();
        assert_eq!(view.room_frame_bounds, None);
    }

    /// 房间体图层（计划 M2 验收）：每个面板网格实例一个半透明子实体，点不中（不挂
    /// `ModelPickTarget`）、不进选中与可见性账（不挂 `ModelMesh`）；撤掉时整层连子实体一起带走，
    /// 回执也跟着没了。
    #[test]
    fn the_room_volume_layer_is_unpickable_and_goes_away_whole() {
        let mut app = test_app();
        let mut record = panel_record([0.0; 3], [1000.0; 3]);
        record.insts = ["panel-a", "panel-b"]
            .into_iter()
            .map(|geo_hash| aios_core::ModelHashInst {
                geo_hash: geo_hash.into(),
                ..Default::default()
            })
            .collect();
        app.world_mut()
            .resource_mut::<View3d>()
            .set_room_volume(RoomVolumeBatch {
                generation: 3,
                panels: vec![record],
                carriers: Vec::new(),
                components: Vec::new(),
                truncate: true,
                unavailable: None,
            });
        app.update();

        let world = app.world_mut();
        let mut layers = world.query_filtered::<Entity, With<RoomVolumeLayer>>();
        assert_eq!(layers.iter(world).count(), 1);
        let mut shells = world.query_filtered::<Entity, With<Mesh3d>>();
        let shells: Vec<Entity> = shells.iter(world).collect();
        assert_eq!(shells.len(), 2, "两个网格实例各一个子实体");
        for shell in shells {
            let shell = world.entity(shell);
            assert!(!shell.contains::<ModelPickTarget>(), "房间体点不中");
            assert!(!shell.contains::<ModelMesh>(), "房间体不进选中与可见性账");
            assert!(shell.contains::<NotShadowCaster>());
        }
        assert_eq!(status(&app).generation, 3);

        app.world_mut().resource_mut::<View3d>().clear_room_volume();
        app.update();
        let world = app.world_mut();
        let mut layers = world.query_filtered::<Entity, With<RoomVolumeLayer>>();
        assert_eq!(layers.iter(world).count(), 0);
        let mut shells = world.query_filtered::<Entity, With<Mesh3d>>();
        assert_eq!(shells.iter(world).count(), 0, "子实体随图层一起撤掉");
        assert_eq!(app.world().resource::<View3d>().room_clip_status(), None);
    }

    /// 面板网格有一块没加载成：房间体不全，宁可不截——回执说缺几块，直段一段不碰。
    #[test]
    fn a_missing_panel_mesh_makes_the_room_volume_unavailable() {
        let bran = RefU64::from(11);
        let mut app = test_app();
        let cylinder = add_mesh(&mut app, cylinder_v3());
        let good = add_mesh(&mut app, room_box());
        // 不归 AssetServer 管、`Assets` 里也已经没有：就是一块加载失败的网格。
        let gone = add_mesh(&mut app, room_box());
        app.world_mut().resource_mut::<Assets<Mesh>>().remove(&gone);
        let layer = app
            .world_mut()
            .spawn((Transform::IDENTITY, Visibility::Visible, RoomVolumeLayer))
            .id();
        let panels = [good, gone]
            .into_iter()
            .map(|handle| PanelMesh {
                handle,
                matrix: Mat4::IDENTITY,
            })
            .collect();
        app.world_mut().resource_mut::<RoomClip>().active =
            Some(Active::new(7, layer, panels, [bran], std::iter::empty(), true, None));
        let tube = spawn_tube(
            &mut app,
            bran,
            tube_root(Vec3::new(500.0, 500.0, 500.0), Vec3::new(1500.0, 500.0, 500.0)),
            cylinder,
            false,
        );
        app.update();
        app.update();

        assert_eq!(
            status(&app).phase,
            RoomClipPhase::Unavailable("房间体缺 1 块面板网格".into())
        );
        assert_eq!(transform_of(&app, tube), Transform::IDENTITY);
        assert_eq!(extras(&mut app), (0, 0));
    }

    /// 选中所属分支（计划 M3 验收 9）：保留段与补出来的兄弟段一起变高亮；断口不挂 `ModelMesh`，
    /// 保持剖切色，但挂着 BRAN 的 `ModelPickTarget`——点断口选中的是这根分支。兄弟段出生时不带材质，
    /// 同帧的 `apply_materials` 补上与保留段同一枚。
    #[test]
    fn selecting_the_branch_highlights_every_piece_but_not_the_caps() {
        let bran = RefU64::from(11);
        let mut app = test_app();
        app.init_asset::<StandardMaterial>()
            .init_asset::<crate::e3d_look::material::SglLookMaterial>()
            .init_resource::<crate::look_materials::LookMaterials>()
            .insert_resource(crate::e3d_look::env_cube::SglEnvCube(None))
            .add_systems(Update, crate::apply_materials.after(advance_room_clip));
        // PBR 外观：材质是 StandardMaterial，断口的剖切色也是，一把尺子量到底。
        app.world_mut().resource_mut::<View3d>().look.mode = plant_ui::LookMode::Pbr;
        let section = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(section_material());
        app.insert_resource(SectionMaterial(section.clone()));
        let cylinder = add_mesh(&mut app, cylinder_v3());
        activate(&mut app, vec![l_room()], &[bran], true);
        let root = tube_root(Vec3::new(1800.0, 500.0, 500.0), Vec3::new(500.0, 1800.0, 500.0));
        let parent = app
            .world_mut()
            .spawn((root, Visibility::Visible, ModelRoot { refno: bran, owner: bran }))
            .id();
        let tube = app
            .world_mut()
            .spawn((
                Mesh3d(cylinder),
                Transform::IDENTITY,
                ModelMesh {
                    refno: bran,
                    owner: bran,
                    colour: [250, 250, 30, 255],
                    line: false,
                    room_shell: false,
                },
                ModelPickTarget { refno: bran },
                TubeBody::solid(Transform::IDENTITY, root),
                ChildOf(parent),
            ))
            .id();
        app.update();
        app.update();
        assert_eq!(extras(&mut app), (1, 2), "凹房里一段直段得两截、两片断口");
        let material_of = |world: &World, entity: Entity| {
            world
                .entity(entity)
                .get::<MeshMaterial3d<StandardMaterial>>()
                .unwrap()
                .0
                .clone()
        };
        let base = material_of(app.world(), tube);
        {
            let world = app.world_mut();
            let mut pieces = world.query_filtered::<Entity, (With<TubeExtra>, With<ModelMesh>)>();
            let pieces: Vec<Entity> = pieces.iter(world).collect();
            assert_eq!(pieces.len(), 1);
            assert_eq!(material_of(world, pieces[0]), base, "兄弟段出生就拿到与保留段同一枚常态材质");
        }

        {
            let mut view = app.world_mut().resource_mut::<View3d>();
            view.set_selection(vec![bran], Some(bran));
        }
        app.update();

        let world = app.world_mut();
        let highlight = material_of(world, tube);
        assert_ne!(highlight, base, "选中换成高亮材质");
        let mut pieces = world.query_filtered::<Entity, (With<TubeExtra>, With<ModelMesh>)>();
        let pieces: Vec<Entity> = pieces.iter(world).collect();
        assert_eq!(pieces.len(), 1);
        assert_eq!(material_of(world, pieces[0]), highlight, "兄弟段跟着高亮");
        let mut caps = world.query_filtered::<Entity, (With<TubeExtra>, Without<ModelMesh>)>();
        let caps: Vec<Entity> = caps.iter(world).collect();
        assert_eq!(caps.len(), 2);
        for cap in caps {
            assert_eq!(material_of(world, cap), section, "断口不跟着变色");
            assert_eq!(
                world.entity(cap).get::<ModelPickTarget>().map(|pick| pick.refno),
                Some(bran),
                "点断口选中所属分支"
            );
        }
    }
}
