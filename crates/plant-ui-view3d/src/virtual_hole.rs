//! 虚拟孔洞预览层（ADR-0031 阶段二 / 阶段四）。
//!
//! 两层东西住在一起：
//!
//! - **切削体层**：每条孔洞一根半透明切削体（CIR 圆柱 / RECT 长方体，毫米、PDMS 世界系，
//!   挂在与 `SceneRoot` 平级的图层根上，不参与拾取、不进可见性账）。选中行高亮并挂
//!   「编号 + 尺寸 + 深度」文字标签（复用尺寸标注那一套毫米锚点投影）。
//! - **布尔预览**：对映射目标的每个实例实体，把相交的切削体换算到实例局部系，连同局部
//!   网格一起交给 `plant-csg` 后端（原生端线程跑 manifold-csg，浏览器端 Worker 跑
//!   manifold-3d）；结果网格只换实体的 `Mesh3d` 句柄，原始网格句柄记在 [`TargetPreview`]
//!   里，清掉 / 看原始 / 重导入都能整批恢复（ADR-0028 换-复原同一套路）。**结果只在
//!   内存里，不写回正式模型。**
//!
//! 布尔回执（[`VirtualHoleReceipt`]）攒在 `View3d` 上，宿主每帧取走落进会话；帧号
//! （`epoch`）对不上的回执宿主自己丢。

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use aios_core::RefU64;
use bevy::pbr::{NotShadowCaster, NotShadowReceiver};
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_asset::RenderAssetUsages;
use plant_csg::{
    BooleanJob, BooleanPreviewBackend, BooleanStatus, CutterJob, CutterShape, MeshData,
};
use plant_ui::virtual_hole::{
    NEGATIVE_INFLATE_MM, VirtualHole, VirtualHoleOp, VirtualHoleReceipt, VirtualHoleShape,
    VirtualHoleStatus, circle_segments, dmat4_to_affine4x3, fmt_mm, obb_intersects_aabb,
};
use web_time::{Duration, Instant};

use super::e3d_look::env_cube::SglEnvCube;
use super::e3d_look::material::SglLookMaterial;
use super::look_materials::{LookMaterials, MaterialAssets};
use super::{
    DimensionLabel, MaterialState, ModelMesh, ModelRoot, OrbitCamera, View3d, ViewCamera,
    XRayMaterial, frame_bounds, material_state, room_clip::triangles_of, scene_bounds_from_mm,
    scene_transform,
};

/// 切削体常态色：琥珀半透明，与 X-Ray 淡蓝、选中橙红、剖切洋红和各档类型色都分得开。
const CUTTER_COLOR: Color = Color::srgba_u8(255, 176, 64, 96);
/// 选中切削体：同一档琥珀，不透明度翻倍——高亮靠「更实」，不靠换色。
const CUTTER_SELECTED_COLOR: Color = Color::srgba_u8(255, 200, 96, 176);
/// 目标实例网格没落地时最多等多久；超时按失败回执（模型还在加载是宿主那一头的事，
/// 重试-显示那条链路已经处理；这里只防网格文件彻底丢掉时的死等）。
const MESH_WAIT: Duration = Duration::from_secs(10);

/// 切削体图层根。与 `SceneRoot` 平级，隔离、显示 / 隐藏、整场重装都碰不到它。
#[derive(Component)]
pub(crate) struct VirtualHoleLayer;

/// 一个孔洞的切削体实体。**不挂 `ModelPickTarget`**——孔洞不参与拾取，点到它还是
/// 点的底下那个构件。
#[derive(Component)]
pub(crate) struct CutterSolid {
    hole_id: String,
}

/// 被布尔预览替换过网格的实例实体。标记存在期间，材质由 [`apply_hole_materials`]
/// 接管（房间 X-Ray 与选中高亮照常生效，孔洞 X-Ray 开关叠在上面）。
#[derive(Component)]
pub(crate) struct HolePreviewed {
    target: RefU64,
}

/// 切削体的两枚共享材质。
#[derive(Resource)]
pub(crate) struct CutterMaterials {
    normal: Handle<StandardMaterial>,
    selected: Handle<StandardMaterial>,
}

fn cutter_material(color: Color) -> StandardMaterial {
    StandardMaterial {
        base_color: color,
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        double_sided: true,
        cull_mode: None,
        ..default()
    }
}

pub(crate) fn setup_virtual_hole(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(CutterMaterials {
        normal: materials.add(cutter_material(CUTTER_COLOR)),
        selected: materials.add(cutter_material(CUTTER_SELECTED_COLOR)),
    });
    commands.insert_resource(HolePreview::new());
}

/// 一个实例的布尔作业进展。
enum InstanceState {
    /// 网格还没取到（资产加载中），每帧重试直到作业的 deadline。
    WaitingMesh,
    /// 已提交后端，等回结果。
    Submitted { job_id: u64 },
    /// 已出结果。
    Done {
        status: BooleanStatus,
        mesh: Option<MeshData>,
        error: Option<String>,
    },
}

/// 一个实例在途的那一单。
struct PendingInstance {
    entity: Entity,
    /// 实体上此刻的网格句柄（原始网格；无论加载没加载，句柄先拿到手）。
    original: Handle<Mesh>,
    /// 实例局部 → 毫米世界：`world_trans × inst.transform`（PDMS 世界系毫米）。
    world: Mat4,
    /// 目标网格内容键；提交时算，`Clear` 时拿它清后端缓存。
    mesh_key: u64,
    /// `NeedMesh` 重提次数上限，防后端抽风时死循环。
    resubmits: u8,
    state: InstanceState,
}

/// 一个映射目标的一趟布尔预览：目标下的全部实例各一单。
struct PendingJob {
    epoch: u64,
    target: RefU64,
    /// 参与这趟预览的孔洞 id（回执原样带回给宿主落状态）。
    hole_ids: Vec<String>,
    /// 这趟作业里目标下的全部孔洞（每个实例各自做相交粗筛）。
    holes: Vec<VirtualHole>,
    /// 网格等待截止；过了还没取到网格的实例按失败落账。
    deadline: Instant,
    instances: Vec<PendingInstance>,
}

/// 一个实例被替换过的网格：原始 / 预览两枚句柄，「看原始」开关来回切。
struct SwappedMesh {
    original: Handle<Mesh>,
    preview: Handle<Mesh>,
}

/// 一个目标已落地的预览：记账（恢复 / 撤销 / X-Ray 都要它）。
struct TargetPreview {
    epoch: u64,
    /// 被替换的实例 -> 原始 / 预览网格。
    swapped: HashMap<Entity, SwappedMesh>,
    hole_ids: Vec<String>,
    status: VirtualHoleStatus,
    /// 参与过的网格内容键（卸载 / 清掉时拿它们清后端缓存）。
    mesh_keys: Vec<u64>,
}

/// 虚拟孔洞层的全部状态。布尔后端也活在这里（线程 / Worker 都不能挂在实体上）。
#[derive(Resource)]
pub(crate) struct HolePreview {
    /// 切削体图层根，`None` = 层没挂。
    layer: Option<Entity>,
    /// 当前层上的孔洞（与宿主 `SetLayer` 同序）。
    holes: Vec<VirtualHole>,
    selected: Option<String>,
    backend: Box<dyn BooleanPreviewBackend>,
    /// 进行中的布尔作业。
    pending: Vec<PendingJob>,
    /// 已落地的预览，按目标归堆。
    previews: HashMap<RefU64, TargetPreview>,
    /// 「看原始网格」开关（只对已出结果的实体有意义）。
    show_original: bool,
    /// 孔洞预览 X-Ray 开关（被开孔的实体半透明）。
    xray: bool,
    /// 下一个作业号。
    next_job: u64,
}

impl HolePreview {
    fn new() -> Self {
        Self {
            layer: None,
            holes: Vec::new(),
            selected: None,
            backend: plant_csg::backend(),
            pending: Vec::new(),
            previews: HashMap::new(),
            show_original: false,
            xray: false,
            next_job: 1,
        }
    }
}

impl Default for HolePreview {
    fn default() -> Self {
        Self::new()
    }
}

impl View3d {
    /// 交一条虚拟孔洞图层指令（按序执行，见 [`apply_virtual_hole_ops`]）。
    pub fn virtual_hole_op(&mut self, op: VirtualHoleOp) {
        self.virtual_hole_ops.push_back(op);
    }

    /// 取走视口攒下的布尔回执（每帧一次；帧号对不上的宿主自己丢）。
    pub fn take_virtual_hole_receipts(&mut self) -> Vec<VirtualHoleReceipt> {
        std::mem::take(&mut self.virtual_hole_receipts)
    }

    /// 本帧落在画内的孔洞标签：纹理 UV（文字中心）+ 文字。与尺寸标注标签同一套投影。
    pub fn visible_hole_labels(&self) -> impl Iterator<Item = ([f32; 2], &str)> + '_ {
        self.hole_labels
            .iter()
            .zip(&self.hole_label_uvs)
            .filter_map(|(label, uv)| uv.map(|uv| (uv, label.text.as_str())))
    }
}

/// 选中孔洞的标签：编号 + 尺寸 + 深度。没选中时整层清空，投影下一拍重算。
fn update_hole_labels(view: &mut View3d, holes: &HolePreview) {
    view.hole_labels.clear();
    if let Some(selected) = &holes.selected
        && let Some(hole) = holes.holes.iter().find(|hole| &hole.id == selected)
    {
        view.hole_labels.push(DimensionLabel {
            anchor: hole.center_mm,
            text: format!(
                "{} {} 深{}",
                hole.id,
                hole.size_label(),
                fmt_mm(hole.depth_mm)
            ),
        });
    }
    view.hole_label_uvs.clear();
}

/// 圆孔切削体网格（毫米，居中，轴沿局部 Z，平盖）。段数与布尔切削体共用
/// [`circle_segments`] 口径——两端画的和切的必须是同一个圆。
fn circle_cutter_mesh(diameter_mm: f32, depth_mm: f32) -> Mesh {
    let n = circle_segments(diameter_mm).max(3) as usize;
    let r = diameter_mm * 0.5;
    let half = depth_mm * 0.5;
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(2 * n + 2);
    let mut indices: Vec<u32> = Vec::with_capacity(6 * n);
    let ring = |z: f32, out: &mut Vec<[f32; 3]>| {
        for i in 0..n {
            let a = i as f32 / n as f32 * std::f32::consts::TAU;
            let (s, c) = a.sin_cos();
            out.push([c * r, s * r, z]);
        }
    };
    ring(-half, &mut positions);
    let bottom = 0..n as u32;
    ring(half, &mut positions);
    let top = n as u32..2 * n as u32;
    for i in 0..n as u32 {
        let j = (i + 1) % n as u32;
        let (b0, b1) = (bottom.start + i, bottom.start + j);
        let (t0, t1) = (top.start + i, top.start + j);
        indices.extend([b0, b1, t1, b0, t1, t0]);
    }
    let bottom_center = positions.len() as u32;
    positions.push([0.0, 0.0, -half]);
    for i in 0..n as u32 {
        let j = (i + 1) % n as u32;
        indices.extend([bottom_center, bottom.start + j, bottom.start + i]);
    }
    let top_center = positions.len() as u32;
    positions.push([0.0, 0.0, half]);
    for i in 0..n as u32 {
        let j = (i + 1) % n as u32;
        indices.extend([top_center, top.start + i, top.start + j]);
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// 一条孔洞的切削体网格 + 摆放变换（毫米、PDMS 世界系）。尺寸非法的孔洞不画——
/// 导入器已经拦过，这里是兜底。
fn cutter_solid(hole: &VirtualHole) -> Option<(Mesh, Transform)> {
    let spec = hole.cutter().ok()?;
    let mesh = match hole.shape {
        VirtualHoleShape::Circle => {
            let diameter = hole.diameter_mm?;
            circle_cutter_mesh(diameter, hole.depth_mm)
        }
        VirtualHoleShape::Rect => {
            let width = hole.width_mm?;
            let height = hole.height_mm?;
            Mesh::from(Cuboid::new(width, height, hole.depth_mm))
        }
    };
    let rotation = Quat::from_mat3(&spec.orientation);
    Some((
        mesh,
        Transform::from_translation(spec.center_mm).with_rotation(rotation),
    ))
}

/// 网格局部顶点在世界矩阵下的世界包围盒（毫米）。粗筛用：宁可误报、不可漏报。
fn mesh_world_aabb(vertices: &[Vec3], world: &Mat4) -> ([f32; 3], [f32; 3]) {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for vertex in vertices {
        let point = world.transform_point3(*vertex);
        for axis in 0..3 {
            min[axis] = min[axis].min(point[axis]);
            max[axis] = max[axis].max(point[axis]);
        }
    }
    (min, max)
}

/// 把一条孔洞的切削体从世界毫米换算到实例局部毫米，打包成后端吃的 [`CutterJob`]。
/// 与实例网格不相交的孔洞返回 `None`（粗筛，布尔前先挡掉）。
fn cutter_job(hole: &VirtualHole, world: &Mat4, world_aabb: ([f32; 3], [f32; 3])) -> Option<CutterJob> {
    let obb = hole.obb().ok()?;
    if !obb_intersects_aabb(&obb, world_aabb.0, world_aabb.1) {
        return None;
    }
    let spec = hole.cutter().ok()?;
    let world_to_local = world.inverse().as_dmat4();
    let local = world_to_local * hole.cutter_mat4().ok()?;
    let inflate = 2.0 * NEGATIVE_INFLATE_MM;
    let size = spec.size_mm.as_dvec3() + bevy::math::DVec3::splat(inflate);
    Some(CutterJob {
        shape: match hole.shape {
            VirtualHoleShape::Circle => CutterShape::Circle,
            VirtualHoleShape::Rect => CutterShape::Rect,
        },
        size_mm: [size.x, size.y, size.z],
        affine: dmat4_to_affine4x3(local),
        segments: match hole.shape {
            VirtualHoleShape::Circle => circle_segments(hole.diameter_mm.unwrap_or(0.0)),
            VirtualHoleShape::Rect => 0,
        },
    })
}

/// 布尔结果网格（局部毫米）→ 渲染网格：展平三角 + 平面法线（预览网格不需要光顺法线）。
fn preview_render_mesh(data: &MeshData) -> Mesh {
    let (positions, normals) = plant_csg::triangle_list(data);
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh
}

/// 恢复一个目标的全部替换：网格句柄换回原始，记账清掉，后端缓存按内容键清。
fn restore_preview(
    commands: &mut Commands,
    backend: &mut dyn BooleanPreviewBackend,
    preview: &mut TargetPreview,
    mesh3ds: &mut Query<&mut Mesh3d>,
) {
    for (entity, swapped) in preview.swapped.drain() {
        if let Ok(mut mesh) = mesh3ds.get_mut(entity) {
            mesh.0 = swapped.original;
        }
        if let Ok(mut entity_commands) = commands.get_entity(entity) {
            entity_commands.remove::<HolePreviewed>();
        }
    }
    for key in preview.mesh_keys.drain(..) {
        backend.forget(key);
    }
}

/// 执行宿主交来的虚拟孔洞指令（整层替换 / 选中 / 预览 / 取景 / 开关 / 撤掉）。
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_virtual_hole_ops(
    mut commands: Commands,
    mut view: ResMut<View3d>,
    mut holes: ResMut<HolePreview>,
    mut meshes: ResMut<Assets<Mesh>>,
    cutters: Res<CutterMaterials>,
    mut orbit: ResMut<OrbitCamera>,
    layer_roots: Query<Entity, With<VirtualHoleLayer>>,
    mut cutter_solids: Query<(Entity, &CutterSolid, &mut MeshMaterial3d<StandardMaterial>)>,
    // 相机（写 `Transform`）与实例 / 父根（读 `Transform`）访问冲突，一并塞进 ParamSet；
    // 需要交叉访问时先整批收了再问（见 Preview 臂）。
    mut mesh_params: ParamSet<(
        Query<(Entity, &ModelMesh, &Mesh3d, &Transform, &ChildOf)>,
        Query<&mut Mesh3d>,
        Query<(&mut Transform, &mut Projection), With<ViewCamera>>,
        Query<&Transform, With<ModelRoot>>,
    )>,
) {
    if view.virtual_hole_ops.is_empty() {
        return;
    }
    let ops: Vec<VirtualHoleOp> = view.virtual_hole_ops.drain(..).collect();
    for op in ops {
        match op {
            VirtualHoleOp::SetLayer { holes: layer_holes } => {
                for entity in &layer_roots {
                    if let Ok(mut entity_commands) = commands.get_entity(entity) {
                        entity_commands.despawn();
                    }
                }
                holes.layer = None;
                holes.holes = layer_holes;
                if let Some(selected) = &holes.selected
                    && !holes.holes.iter().any(|hole| &hole.id == selected)
                {
                    holes.selected = None;
                }
                let layer = commands
                    .spawn((scene_transform(), Visibility::Visible, VirtualHoleLayer))
                    .id();
                holes.layer = Some(layer);
                for hole in &holes.holes {
                    let Some((mesh, transform)) = cutter_solid(hole) else {
                        continue;
                    };
                    let handle = meshes.add(mesh);
                    let material = if holes.selected.as_deref() == Some(hole.id.as_str()) {
                        cutters.selected.clone()
                    } else {
                        cutters.normal.clone()
                    };
                    commands.spawn((
                        Mesh3d(handle),
                        MeshMaterial3d(material),
                        transform,
                        CutterSolid {
                            hole_id: hole.id.clone(),
                        },
                        NotShadowCaster,
                        NotShadowReceiver,
                        ChildOf(layer),
                    ));
                }
                update_hole_labels(&mut view, &holes);
            }
            VirtualHoleOp::SelectHole(selected) => {
                holes.selected = selected
                    .filter(|id| holes.holes.iter().any(|hole| &hole.id == id));
                for (_, solid, mut material) in &mut cutter_solids {
                    material.0 = if holes.selected.as_deref() == Some(solid.hole_id.as_str()) {
                        cutters.selected.clone()
                    } else {
                        cutters.normal.clone()
                    };
                }
                update_hole_labels(&mut view, &holes);
            }
            VirtualHoleOp::Preview {
                epoch,
                target,
                holes: preview_holes,
            } => {
                // 重发同一目标：旧的预览先整批恢复，在途作业丢掉（回执靠帧号作废）。
                if let Some(mut old) = holes.previews.remove(&target) {
                    restore_preview(
                        &mut commands,
                        &mut *holes.backend,
                        &mut old,
                        &mut mesh_params.p1(),
                    );
                }
                let (keep, removed): (Vec<PendingJob>, Vec<PendingJob>) =
                    holes.pending.drain(..).partition(|job| job.target != target);
                for job in &removed {
                    for instance in &job.instances {
                        if instance.mesh_key != 0 {
                            holes.backend.forget(instance.mesh_key);
                        }
                    }
                }
                holes.pending = keep;
                let hole_ids: Vec<String> =
                    preview_holes.iter().map(|hole| hole.id.clone()).collect();
                // 目标在场景里的实例：refno 或 owner 命中都算（与材质 / 可见性同一口径）。
                // 实例查询与父根查询都读 `Transform`，同属一个 ParamSet，得先收再问。
                let mut found = Vec::new();
                {
                    let instances = mesh_params.p0();
                    for (entity, mesh, mesh3d, transform, child) in instances.iter() {
                        if mesh.refno != target && mesh.owner != target {
                            continue;
                        }
                        found.push((entity, mesh3d.0.clone(), *transform, child.parent()));
                    }
                }
                let mut instances = Vec::new();
                let roots = mesh_params.p3();
                for (entity, original, local, parent) in found {
                    let Ok(root) = roots.get(parent) else {
                        continue;
                    };
                    let world = root.compute_matrix() * local.compute_matrix();
                    instances.push(PendingInstance {
                        entity,
                        original,
                        world,
                        mesh_key: 0,
                        resubmits: 0,
                        state: InstanceState::WaitingMesh,
                    });
                }
                if instances.is_empty() {
                    // 一个实例都没有：回执请宿主先显示模型，落地了再重试。
                    view.virtual_hole_receipts.push(VirtualHoleReceipt {
                        epoch,
                        refno: target,
                        hole_ids,
                        status: VirtualHoleStatus::Ready,
                        no_instances: true,
                        dropped: false,
                        error: None,
                        elapsed_ms: 0,
                    });
                    continue;
                }
                holes.pending.push(PendingJob {
                    epoch,
                    target,
                    hole_ids,
                    holes: preview_holes,
                    deadline: Instant::now() + MESH_WAIT,
                    instances,
                });
            }
            VirtualHoleOp::FocusHole(id) => {
                let Some(hole) = holes.holes.iter().find(|hole| hole.id == id) else {
                    continue;
                };
                let Ok((min_mm, max_mm)) = hole.world_aabb_mm() else {
                    continue;
                };
                let Some((min, max)) = scene_bounds_from_mm(min_mm, max_mm) else {
                    continue;
                };
                let mut camera = mesh_params.p2();
                if let Ok((mut camera_transform, mut projection)) = camera.single_mut() {
                    orbit.anim = None;
                    frame_bounds(min, max, &mut orbit, &mut camera_transform, &mut projection);
                }
            }
            VirtualHoleOp::SetShowOriginal(on) => {
                holes.show_original = on;
                for preview in holes.previews.values() {
                    for (entity, swapped) in &preview.swapped {
                        if let Ok(mut mesh) = mesh_params.p1().get_mut(*entity) {
                            mesh.0 = if on {
                                swapped.original.clone()
                            } else {
                                swapped.preview.clone()
                            };
                        }
                    }
                }
            }
            VirtualHoleOp::SetXRay(on) => {
                // 材质由 `apply_hole_materials` 每帧按开关落。
                holes.xray = on;
            }
            VirtualHoleOp::Clear => {
                let previews: Vec<TargetPreview> =
                    holes.previews.drain().map(|(_, preview)| preview).collect();
                for mut preview in previews {
                    restore_preview(
                        &mut commands,
                        &mut *holes.backend,
                        &mut preview,
                        &mut mesh_params.p1(),
                    );
                }
                for job in std::mem::take(&mut holes.pending) {
                    for instance in &job.instances {
                        if instance.mesh_key != 0 {
                            holes.backend.forget(instance.mesh_key);
                        }
                    }
                }
                if let Some(layer) = holes.layer.take() {
                    if let Ok(mut entity_commands) = commands.get_entity(layer) {
                        entity_commands.despawn();
                    }
                }
                holes.holes.clear();
                holes.selected = None;
                view.hole_labels.clear();
                view.hole_label_uvs.clear();
            }
        }
    }
}

/// 每帧推进：等网格 / 提交作业 / 收回结果 / 拼回执；实体没了的作业与预览按卸载作废。
pub(crate) fn advance_virtual_hole(
    mut commands: Commands,
    mut view: ResMut<View3d>,
    mut holes: ResMut<HolePreview>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mesh3ds: Query<&mut Mesh3d>,
) {
    // 字段级拆借：`pending` 与 `backend`/`next_job` 要在同一循环里交叉访问，
    // 不能对 `ResMut` 各拿各的（DerefMut 拿走的是整个资源）。
    let HolePreview {
        pending,
        next_job,
        backend,
        previews,
        ..
    } = &mut *holes;
    // 1. 等网格的实例：拿到网格就做粗筛、提交作业；超时按失败落账。
    for job in pending.iter_mut() {
        for instance in &mut job.instances {
            if !matches!(instance.state, InstanceState::WaitingMesh) {
                continue;
            }
            let Ok(mesh3d) = mesh3ds.get(instance.entity) else {
                instance.state = InstanceState::Done {
                    status: BooleanStatus::Failed,
                    mesh: None,
                    error: Some("实例已卸载".into()),
                };
                continue;
            };
            let Some(mesh) = meshes.get(&mesh3d.0) else {
                if Instant::now() >= job.deadline {
                    instance.state = InstanceState::Done {
                        status: BooleanStatus::Failed,
                        mesh: None,
                        error: Some("网格加载超时".into()),
                    };
                }
                continue;
            };
            let Some((vertices, indices)) = triangles_of(mesh) else {
                instance.state = InstanceState::Done {
                    status: BooleanStatus::Failed,
                    mesh: None,
                    error: Some("网格不是三角形列表".into()),
                };
                continue;
            };
            let mesh_data = MeshData {
                positions: vertices.iter().map(|v| v.to_array()).collect(),
                indices,
            };
            let mesh_key = plant_csg::mesh_key(&mesh_data);
            instance.mesh_key = mesh_key;
            let world_aabb = mesh_world_aabb(&vertices, &instance.world);
            let cutters: Vec<CutterJob> = job
                .holes
                .iter()
                .filter_map(|hole| cutter_job(hole, &instance.world, world_aabb))
                .collect();
            if cutters.is_empty() {
                instance.state = InstanceState::Done {
                    status: BooleanStatus::NoIntersection,
                    mesh: None,
                    error: None,
                };
                continue;
            }
            let job_id = *next_job;
            *next_job += 1;
            backend.submit(BooleanJob {
                job_id,
                epoch: job.epoch,
                mesh_key,
                mesh: Some(Arc::new(mesh_data)),
                cutters,
            });
            instance.state = InstanceState::Submitted { job_id };
        }
    }

    // 2. 收后端结果。
    let outcomes = backend.poll();
    for outcome in outcomes {
        let Some(job) = pending.iter_mut().find(|job| {
            job.instances.iter().any(|instance| {
                matches!(instance.state, InstanceState::Submitted { job_id } if job_id == outcome.job_id)
            })
        }) else {
            // 作业已撤（清预览 / 重发同一目标），结果不要了。
            continue;
        };
        let Some(instance) = job.instances.iter_mut().find(|instance| {
            matches!(instance.state, InstanceState::Submitted { job_id } if job_id == outcome.job_id)
        }) else {
            continue;
        };
        instance.state = match outcome.status {
            BooleanStatus::NeedMesh if instance.resubmits < 2 => {
                // 后端丢了缓存：带网格重提一次（回到等网格那一步，网格还在资产里）。
                instance.resubmits += 1;
                InstanceState::WaitingMesh
            }
            BooleanStatus::NeedMesh => InstanceState::Done {
                status: BooleanStatus::Failed,
                mesh: None,
                error: Some("布尔后端反复索要网格".into()),
            },
            status => InstanceState::Done {
                status,
                mesh: outcome.mesh,
                error: outcome.error,
            },
        };
    }

    // 3. 聚齐的作业拼回执：有失败即失败；全无相交即无相交；否则已切割并换网格。
    let mut finished = Vec::new();
    for (index, job) in pending.iter().enumerate() {
        if job.instances
            .iter()
            .all(|instance| matches!(instance.state, InstanceState::Done { .. }))
        {
            finished.push(index);
        }
    }
    for index in finished.into_iter().rev() {
        let job = pending.remove(index);
        let mut error = None;
        let mut all_no_intersection = true;
        let mut swapped: HashMap<Entity, SwappedMesh> = HashMap::new();
        let mut mesh_keys = Vec::new();
        for instance in &job.instances {
            let InstanceState::Done {
                status,
                mesh,
                error: instance_error,
            } = &instance.state
            else {
                continue;
            };
            match status {
                BooleanStatus::Failed => {
                    error = Some(
                        instance_error
                            .clone()
                            .unwrap_or_else(|| "布尔内核失败".into()),
                    );
                }
                BooleanStatus::NoIntersection => {}
                BooleanStatus::Changed => {
                    all_no_intersection = false;
                    let Some(data) = mesh else { continue };
                    let preview_handle = meshes.add(preview_render_mesh(&data));
                    if let Ok(mut mesh3d) = mesh3ds.get_mut(instance.entity) {
                        mesh3d.0 = preview_handle.clone();
                        commands.entity(instance.entity).insert(HolePreviewed {
                            target: job.target,
                        });
                        if !mesh_keys.contains(&instance.mesh_key) {
                            mesh_keys.push(instance.mesh_key);
                        }
                        swapped.insert(
                            instance.entity,
                            SwappedMesh {
                                original: instance.original.clone(),
                                preview: preview_handle,
                            },
                        );
                    }
                }
                // NeedMesh 在上面已经折回 WaitingMesh 或 Failed，到不了这里。
                BooleanStatus::NeedMesh => {}
            }
        }
        let status = if error.is_some() {
            VirtualHoleStatus::Failed
        } else if all_no_intersection {
            VirtualHoleStatus::NoIntersection
        } else if !swapped.is_empty() {
            VirtualHoleStatus::Changed
        } else {
            // 没失败、没无相交、也没换成任何网格：实例在出结果前被卸掉了。
            VirtualHoleStatus::Failed
        };
        if status == VirtualHoleStatus::Changed {
            previews.insert(
                job.target,
                TargetPreview {
                    epoch: job.epoch,
                    swapped,
                    hole_ids: job.hole_ids.clone(),
                    status,
                    mesh_keys,
                },
            );
        } else {
            // 失败的目标不留半截预览：已经换过的网格当场恢复。
            for (entity, swapped_mesh) in swapped {
                if let Ok(mut mesh3d) = mesh3ds.get_mut(entity) {
                    mesh3d.0 = swapped_mesh.original;
                }
                if let Ok(mut entity_commands) = commands.get_entity(entity) {
                    entity_commands.remove::<HolePreviewed>();
                }
            }
        }
        let dropped = status == VirtualHoleStatus::Failed && error.is_none();
        view.virtual_hole_receipts.push(VirtualHoleReceipt {
            epoch: job.epoch,
            refno: job.target,
            hole_ids: job.hole_ids,
            status,
            no_instances: false,
            dropped,
            error: error.or(if dropped {
                Some("目标已卸载，预览作废".into())
            } else {
                None
            }),
            elapsed_ms: 0,
        });
    }

    // 4. 进行中的作业：实例实体一个都没了（卸载 / 重装）——按作废回执。
    let mut dropped = Vec::new();
    for (index, job) in pending.iter().enumerate() {
        if job.instances
            .iter()
            .all(|instance| mesh3ds.get(instance.entity).is_err())
        {
            dropped.push(index);
        }
    }
    for index in dropped.into_iter().rev() {
        let job = pending.remove(index);
        for instance in &job.instances {
            if instance.mesh_key != 0 {
                backend.forget(instance.mesh_key);
            }
        }
        view.virtual_hole_receipts.push(VirtualHoleReceipt {
            epoch: job.epoch,
            refno: job.target,
            hole_ids: job.hole_ids,
            status: VirtualHoleStatus::Failed,
            no_instances: false,
            dropped: true,
            error: Some("目标已卸载，预览作废".into()),
            elapsed_ms: 0,
        });
    }

    // 5. 已落地的预览：被替换的实体一个都没了（重装 / 卸载）——预览失效，回执叫宿主落账。
    let mut gone = Vec::new();
    for (target, preview) in previews.iter() {
        if preview.status == VirtualHoleStatus::Changed
            && preview
                .swapped
                .keys()
                .all(|entity| mesh3ds.get(*entity).is_err())
        {
            gone.push(*target);
        }
    }
    for target in gone {
        if let Some(mut preview) = previews.remove(&target) {
            for key in preview.mesh_keys.drain(..) {
                backend.forget(key);
            }
            view.virtual_hole_receipts.push(VirtualHoleReceipt {
                epoch: preview.epoch,
                refno: target,
                hole_ids: std::mem::take(&mut preview.hole_ids),
                status: VirtualHoleStatus::Failed,
                no_instances: false,
                dropped: true,
                error: Some("目标已卸载，预览已失效".into()),
                elapsed_ms: 0,
            });
        }
    }
}

/// `HolePreviewed` 实体的材质维护：房间 X-Ray / 选中高亮照常（与 `apply_materials` 同一套裁决、
/// 同一张 `LookMaterials` 表），孔洞 X-Ray 开关叠在最上。跑在 `apply_materials` 之后，赢最后一笔。
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_hole_materials(
    mut commands: Commands,
    view: Res<View3d>,
    mut look_materials: ResMut<LookMaterials>,
    mut standard: ResMut<Assets<StandardMaterial>>,
    mut sgl: ResMut<Assets<SglLookMaterial>>,
    env: Res<SglEnvCube>,
    xray: Res<XRayMaterial>,
    holes: Res<HolePreview>,
    mut previewed: Query<(
        Entity,
        &ModelMesh,
        &HolePreviewed,
        Option<&mut MeshMaterial3d<StandardMaterial>>,
        Option<&mut MeshMaterial3d<SglLookMaterial>>,
    )>,
) {
    let hole_xray: HashSet<RefU64> = if holes.xray {
        holes.previews.keys().copied().collect()
    } else {
        HashSet::new()
    };
    let mut assets = MaterialAssets {
        standard: &mut standard,
        sgl: &mut sgl,
        env: env.0.clone(),
        xray: xray.0.clone(),
    };
    for (entity, mesh, previewed, pbr, sgl_material) in &mut previewed {
        let state = if hole_xray.contains(&previewed.target) {
            MaterialState::XRay
        } else {
            material_state(
                &view.selected,
                view.primary,
                &view.xray,
                mesh.refno,
                mesh.owner,
            )
        };
        look_materials
            .resolve(view.look, mesh, state, &mut assets)
            .attach(entity, pbr, sgl_material, &mut commands);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ModelPickTarget, blank_view};
    use bevy::asset::AssetPlugin;
    use plant_csg::BooleanOutcome;
    use plant_ui::virtual_hole::parse_rely_refno;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    /// 可控的假后端：提交攒着，结果从共享队列喂，测试想什么时候回就什么时候回。
    struct FakeBackend {
        submitted: Vec<BooleanJob>,
        outbox: Arc<Mutex<VecDeque<BooleanOutcome>>>,
    }

    impl BooleanPreviewBackend for FakeBackend {
        fn submit(&mut self, job: BooleanJob) {
            self.submitted.push(job);
        }
        fn poll(&mut self) -> Vec<BooleanOutcome> {
            self.outbox.lock().unwrap().drain(..).collect()
        }
        fn forget(&mut self, _mesh_key: u64) {}
    }

    fn test_app() -> (App, Arc<Mutex<VecDeque<BooleanOutcome>>>) {
        let outbox = Arc::new(Mutex::new(VecDeque::new()));
        let backend = FakeBackend {
            submitted: Vec::new(),
            outbox: outbox.clone(),
        };
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Mesh>()
            .init_asset::<StandardMaterial>()
            .init_asset::<SglLookMaterial>()
            .init_resource::<LookMaterials>()
            .insert_resource(SglEnvCube(None))
            .insert_resource(blank_view())
            .insert_resource(OrbitCamera::default())
            .insert_resource(XRayMaterial(Handle::default()))
            .insert_resource(CutterMaterials {
                normal: Handle::default(),
                selected: Handle::default(),
            })
            .insert_resource(HolePreview::new())
            .add_systems(
                Update,
                (
                    apply_virtual_hole_ops,
                    advance_virtual_hole,
                    apply_hole_materials,
                )
                    .chain(),
            );
        // 把真后端换成假后端。
        app.world_mut().resource_mut::<HolePreview>().backend = Box::new(backend);
        (app, outbox)
    }

    fn hole(id: &str) -> VirtualHole {
        VirtualHole {
            id: id.to_string(),
            shape: VirtualHoleShape::Circle,
            center_mm: [0.0, 0.0, 0.0],
            orientation: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            diameter_mm: Some(300.0),
            width_mm: None,
            height_mm: None,
            depth_mm: 800.0,
            rely_item_ref: "24381_145018".into(),
            rely_refno: parse_rely_refno("24381_145018"),
            item_ref: String::new(),
            source_row: 2,
            hole_work: String::new(),
            h_type: String::new(),
            subs_material: String::new(),
            raw_shape: "CIR".into(),
            raw_orientation: String::new(),
        }
    }

    fn target() -> RefU64 {
        parse_rely_refno("24381_145018").unwrap()
    }

    /// 1000³ 的箱子，原点为角点（三角形列表）。
    fn cube_mesh() -> Mesh {
        let p: [[f32; 3]; 8] = [
            [0.0, 0.0, 0.0],
            [1000.0, 0.0, 0.0],
            [1000.0, 1000.0, 0.0],
            [0.0, 1000.0, 0.0],
            [0.0, 0.0, 1000.0],
            [1000.0, 0.0, 1000.0],
            [1000.0, 1000.0, 1000.0],
            [0.0, 1000.0, 1000.0],
        ];
        let faces = [
            [0, 1, 2, 3],
            [4, 6, 5, 7],
            [0, 4, 5, 1],
            [2, 6, 7, 3],
            [0, 3, 7, 4],
            [1, 5, 6, 2],
        ];
        let mut positions = Vec::new();
        let mut indices = Vec::new();
        for face in faces {
            let base = positions.len() as u32;
            positions.extend(face.map(|i| p[i]));
            indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_indices(Indices::U32(indices));
        mesh
    }

    fn add_mesh(app: &mut App, mesh: Mesh) -> Handle<Mesh> {
        app.world_mut().resource_mut::<Assets<Mesh>>().add(mesh)
    }

    /// 场景里放一个目标实例：模型根 + 一个网格子实体。交回子实体。
    fn spawn_instance(app: &mut App, refno: RefU64, mesh: Handle<Mesh>) -> Entity {
        let world = app.world_mut();
        let parent = world
            .spawn((
                Transform::IDENTITY,
                Visibility::Visible,
                ModelRoot {
                    refno,
                    owner: refno,
                },
            ))
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
                    line: false,
                },
                ModelPickTarget { refno },
                ChildOf(parent),
            ))
            .id()
    }

    fn op(app: &mut App, op: VirtualHoleOp) {
        app.world_mut()
            .resource_mut::<View3d>()
            .virtual_hole_op(op);
    }

    fn receipts(app: &mut App) -> Vec<VirtualHoleReceipt> {
        app.world_mut()
            .resource_mut::<View3d>()
            .take_virtual_hole_receipts()
    }

    fn mesh_of(app: &App, entity: Entity) -> Handle<Mesh> {
        app.world().get::<Mesh3d>(entity).unwrap().0.clone()
    }

    /// 切削体层：挂出来、不参与拾取、选中高亮换材质。
    #[test]
    fn set_layer_spawns_unpickable_cutters_and_highlights_selection() {
        let (mut app, _outbox) = test_app();
        let normal = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(cutter_material(CUTTER_COLOR));
        let selected = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(cutter_material(CUTTER_SELECTED_COLOR));
        app.world_mut().resource_mut::<CutterMaterials>().normal = normal.clone();
        app.world_mut().resource_mut::<CutterMaterials>().selected = selected.clone();
        op(&mut app, VirtualHoleOp::SetLayer {
            holes: vec![hole("H1"), hole("H2")],
        });
        app.update();
        // 选中要等切削体落地（commands 是延后的），下一帧再发。
        op(&mut app, VirtualHoleOp::SelectHole(Some("H2".into())));
        app.update();

        let world = app.world_mut();
        let mut cutters = world.query_filtered::<(Entity, &CutterSolid, &MeshMaterial3d<StandardMaterial>), With<CutterSolid>>();
        let mut rows: Vec<(String, bool, bool)> = cutters
            .iter(world)
            .map(|(entity, solid, material)| {
                let pickable = world.get::<ModelPickTarget>(entity).is_some();
                (solid.hole_id.clone(), material.0 == selected, pickable)
            })
            .collect();
        rows.sort();
        assert_eq!(
            rows,
            vec![
                ("H1".to_string(), false, false),
                ("H2".to_string(), true, false)
            ]
        );
        // 选中行的标签挂上了（锚点就是孔洞中心）。
        let view = world.resource::<View3d>();
        assert_eq!(view.hole_labels.len(), 1);
        assert_eq!(view.hole_labels[0].anchor, [0.0, 0.0, 0.0]);
        assert!(view.hole_labels[0].text.contains("H2"));
        assert!(view.hole_labels[0].text.contains("Φ300"));
    }

    /// 圆切削体网格：两圈侧边各 n 点 + 两个盖心，侧面 6n 三角 + 两个盖各 3n 三角，
    /// 段数与布尔口径同源。
    #[test]
    fn circle_cutter_mesh_matches_boolean_segment_rule() {
        let mesh = circle_cutter_mesh(325.0, 800.0);
        let n = circle_segments(325.0) as usize;
        let Some(bevy::render::mesh::VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("cutter mesh needs positions");
        };
        assert_eq!(positions.len(), 2 * n + 2);
        let Some(Indices::U32(indices)) = mesh.indices() else {
            panic!("cutter mesh needs u32 indices");
        };
        assert_eq!(indices.len(), 12 * n);
    }

    /// 布尔完成：网格替换、实体挂标记、回执 Changed。
    #[test]
    fn changed_outcome_swaps_mesh_and_reports_receipt() {
        let (mut app, outbox) = test_app();
        let original = add_mesh(&mut app, cube_mesh());
        let instance = spawn_instance(&mut app, target(), original.clone());
        op(&mut app, VirtualHoleOp::SetLayer {
            holes: vec![hole("H1")],
        });
        op(&mut app, VirtualHoleOp::Preview {
            epoch: 7,
            target: target(),
            holes: vec![hole("H1")],
        });
        app.update();
        // 作业已提交（粗筛命中：孔洞就在箱子里）。
        {
            let holes = app.world().resource::<HolePreview>();
            assert_eq!(holes.pending.len(), 1);
            assert_eq!(holes.pending[0].instances.len(), 1);
        }
        // 后端回结果：换一张网格。
        let result_mesh = MeshData {
            positions: vec![[0.0, 0.0, 0.0], [500.0, 0.0, 0.0], [0.0, 500.0, 0.0]],
            indices: vec![0, 1, 2],
        };
        outbox.lock().unwrap().push_back(BooleanOutcome {
            job_id: 1,
            epoch: 7,
            status: BooleanStatus::Changed,
            mesh: Some(result_mesh),
            volume_before: 1.0,
            volume_after: 0.5,
            tri_count: 1,
            elapsed_ms: 3,
            error: None,
        });
        app.update();
        assert_ne!(mesh_of(&app, instance), original);
        assert!(app.world().get::<HolePreviewed>(instance).is_some());
        let receipts = receipts(&mut app);
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0].status, VirtualHoleStatus::Changed);
        assert_eq!(receipts[0].epoch, 7);
        assert_eq!(receipts[0].refno, target());
        assert_eq!(receipts[0].hole_ids, vec!["H1".to_string()]);
        assert!(!receipts[0].no_instances);
        assert!(!receipts[0].dropped);
    }

    /// 清掉预览：原始网格整批恢复，切削体层撤掉，标记摘掉。
    #[test]
    fn clear_restores_original_meshes_and_despawns_layer() {
        let (mut app, outbox) = test_app();
        let original = add_mesh(&mut app, cube_mesh());
        let instance = spawn_instance(&mut app, target(), original.clone());
        op(&mut app, VirtualHoleOp::SetLayer {
            holes: vec![hole("H1")],
        });
        op(&mut app, VirtualHoleOp::Preview {
            epoch: 1,
            target: target(),
            holes: vec![hole("H1")],
        });
        app.update();
        outbox.lock().unwrap().push_back(BooleanOutcome {
            job_id: 1,
            epoch: 1,
            status: BooleanStatus::Changed,
            mesh: Some(MeshData {
                positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                indices: vec![0, 1, 2],
            }),
            volume_before: 1.0,
            volume_after: 0.5,
            tri_count: 1,
            elapsed_ms: 1,
            error: None,
        });
        app.update();
        assert_ne!(mesh_of(&app, instance), original);
        op(&mut app, VirtualHoleOp::Clear);
        app.update();
        assert_eq!(mesh_of(&app, instance), original);
        assert!(app.world().get::<HolePreviewed>(instance).is_none());
        let mut layers = app
            .world_mut()
            .query_filtered::<Entity, With<VirtualHoleLayer>>();
        assert_eq!(layers.iter(app.world()).count(), 0);
        let holes = app.world().resource::<HolePreview>();
        assert!(holes.previews.is_empty());
        assert!(holes.pending.is_empty());
        assert!(holes.holes.is_empty());
    }

    /// 「看原始」开关：在原始 / 预览两枚网格之间来回切。
    #[test]
    fn show_original_toggles_between_original_and_preview() {
        let (mut app, outbox) = test_app();
        let original = add_mesh(&mut app, cube_mesh());
        let instance = spawn_instance(&mut app, target(), original.clone());
        op(&mut app, VirtualHoleOp::Preview {
            epoch: 1,
            target: target(),
            holes: vec![hole("H1")],
        });
        app.update();
        outbox.lock().unwrap().push_back(BooleanOutcome {
            job_id: 1,
            epoch: 1,
            status: BooleanStatus::Changed,
            mesh: Some(MeshData {
                positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                indices: vec![0, 1, 2],
            }),
            volume_before: 1.0,
            volume_after: 0.5,
            tri_count: 1,
            elapsed_ms: 1,
            error: None,
        });
        app.update();
        let preview = mesh_of(&app, instance);
        assert_ne!(preview, original);
        op(&mut app, VirtualHoleOp::SetShowOriginal(true));
        app.update();
        assert_eq!(mesh_of(&app, instance), original);
        op(&mut app, VirtualHoleOp::SetShowOriginal(false));
        app.update();
        assert_eq!(mesh_of(&app, instance), preview);
    }

    /// 目标一个实例都没有：立刻回执 no_instances，请宿主先显示模型。
    #[test]
    fn preview_without_instances_reports_no_instances() {
        let (mut app, _outbox) = test_app();
        op(&mut app, VirtualHoleOp::Preview {
            epoch: 3,
            target: target(),
            holes: vec![hole("H1")],
        });
        app.update();
        let receipts = receipts(&mut app);
        assert_eq!(receipts.len(), 1);
        assert!(receipts[0].no_instances);
        assert_eq!(receipts[0].epoch, 3);
        let holes = app.world().resource::<HolePreview>();
        assert!(holes.pending.is_empty());
    }

    /// 作业在途时目标被卸掉：回执 dropped，宿主拿它把行状态落成失败。
    #[test]
    fn unload_during_flight_reports_dropped() {
        let (mut app, _outbox) = test_app();
        let original = add_mesh(&mut app, cube_mesh());
        spawn_instance(&mut app, target(), original);
        op(&mut app, VirtualHoleOp::Preview {
            epoch: 5,
            target: target(),
            holes: vec![hole("H1")],
        });
        app.update();
        assert_eq!(app.world().resource::<HolePreview>().pending.len(), 1);
        // 卸掉目标（实体整棵没了）。
        let mut roots = app
            .world_mut()
            .query_filtered::<Entity, With<ModelRoot>>();
        let root = roots.iter(app.world()).next().unwrap();
        app.world_mut().entity_mut(root).despawn();
        app.update();
        let receipts = receipts(&mut app);
        assert_eq!(receipts.len(), 1);
        assert!(receipts[0].dropped);
        assert_eq!(receipts[0].status, VirtualHoleStatus::Failed);
        assert_eq!(receipts[0].epoch, 5);
        assert!(app.world().resource::<HolePreview>().pending.is_empty());
    }

    /// 已落地的预览被整棵卸掉：同样回执 dropped（预览失效，不是无声消失）。
    #[test]
    fn unload_after_preview_reports_dropped() {
        let (mut app, outbox) = test_app();
        let original = add_mesh(&mut app, cube_mesh());
        spawn_instance(&mut app, target(), original);
        op(&mut app, VirtualHoleOp::Preview {
            epoch: 2,
            target: target(),
            holes: vec![hole("H1")],
        });
        app.update();
        outbox.lock().unwrap().push_back(BooleanOutcome {
            job_id: 1,
            epoch: 2,
            status: BooleanStatus::Changed,
            mesh: Some(MeshData {
                positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                indices: vec![0, 1, 2],
            }),
            volume_before: 1.0,
            volume_after: 0.5,
            tri_count: 1,
            elapsed_ms: 1,
            error: None,
        });
        app.update();
        assert_eq!(receipts(&mut app).len(), 1);
        // 卸掉目标。
        let mut roots = app
            .world_mut()
            .query_filtered::<Entity, With<ModelRoot>>();
        let root = roots.iter(app.world()).next().unwrap();
        app.world_mut().entity_mut(root).despawn();
        app.update();
        let receipts = receipts(&mut app);
        assert_eq!(receipts.len(), 1);
        assert!(receipts[0].dropped);
        assert_eq!(receipts[0].hole_ids, vec!["H1".to_string()]);
    }

    /// 与实例不相交的孔洞：不提交作业，整目标回执 NoIntersection。
    #[test]
    fn far_away_hole_reports_no_intersection_without_submitting() {
        let (mut app, _outbox) = test_app();
        let original = add_mesh(&mut app, cube_mesh());
        spawn_instance(&mut app, target(), original);
        let mut far = hole("H1");
        far.center_mm = [1_000_000.0, 0.0, 0.0];
        op(&mut app, VirtualHoleOp::Preview {
            epoch: 1,
            target: target(),
            holes: vec![far],
        });
        app.update();
        let receipts = receipts(&mut app);
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0].status, VirtualHoleStatus::NoIntersection);
        let holes = app.world().resource::<HolePreview>();
        assert!(holes.pending.is_empty());
        assert!(holes.previews.is_empty());
    }

    /// 孔洞 X-Ray 开关：预览目标的实体换成半透明材质（PBR 外观下就是房间 X-Ray 那枚）。
    #[test]
    fn hole_xray_swaps_previewed_material() {
        let (mut app, outbox) = test_app();
        app.world_mut().resource_mut::<View3d>().look.mode = plant_ui::LookMode::Pbr;
        let xray = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(crate::xray_material());
        app.world_mut().resource_mut::<XRayMaterial>().0 = xray.clone();
        let original = add_mesh(&mut app, cube_mesh());
        let instance = spawn_instance(&mut app, target(), original);
        op(&mut app, VirtualHoleOp::Preview {
            epoch: 1,
            target: target(),
            holes: vec![hole("H1")],
        });
        app.update();
        outbox.lock().unwrap().push_back(BooleanOutcome {
            job_id: 1,
            epoch: 1,
            status: BooleanStatus::Changed,
            mesh: Some(MeshData {
                positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                indices: vec![0, 1, 2],
            }),
            volume_before: 1.0,
            volume_after: 0.5,
            tri_count: 1,
            elapsed_ms: 1,
            error: None,
        });
        app.update();
        op(&mut app, VirtualHoleOp::SetXRay(true));
        app.update();
        let material = app
            .world()
            .get::<MeshMaterial3d<StandardMaterial>>(instance)
            .unwrap();
        assert_eq!(material.0, xray);
        op(&mut app, VirtualHoleOp::SetXRay(false));
        app.update();
        let material = app
            .world()
            .get::<MeshMaterial3d<StandardMaterial>>(instance)
            .unwrap();
        assert_ne!(material.0, xray);
    }
}
