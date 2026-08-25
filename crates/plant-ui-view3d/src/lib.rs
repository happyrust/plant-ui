//! 独立三维视口模块：模型装卸、离屏渲染、相机控制与拾取，以及视口显示基建
//! ——主题渐变背景、地面网格与原点三色轴、ViewCube 视角跳转动画。
//!
//! 立方体本体不在这儿：它是 egui 侧的正交小部件（`plant_ui::workbench::viewcube`），
//! 这一侧只负责把相机姿态逐帧发布出去、并执行它发回的 `SnapView`。曾有一版
//! 第二正交相机 + RenderLayer 的场景内立方体走到一半，被换掉的原因写在
//! viewcube 模块头上：中文面标、悬停高亮、26 区命中在 egui 里是二十行投影，
//! 在场景里是字体栅格化加射线拾取一整套。

use std::collections::{HashMap, HashSet, VecDeque};
use std::f32::consts::FRAC_PI_2;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use web_time::{Duration, Instant};

use std::str::FromStr;

use aios_core::pdms_types::PdmsGenericType;
use aios_core::shape::pdms_shape::PlantMesh;
use aios_core::{GeomInstQuery, RefU64};
use bevy::asset::io::Reader;
use bevy::asset::{
    AssetLoader, AssetServer, Assets, LoadContext, LoadState, load_internal_asset, weak_handle,
};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey, NotShadowCaster, NotShadowReceiver};
use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::render::camera::{ClearColorConfig, RenderTarget, ScalingMode};
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::mesh::{
    Indices, MeshVertexAttribute, MeshVertexBufferLayoutRef, PrimitiveTopology,
};
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, RenderPipelineDescriptor, Shader, ShaderRef,
    SpecializedMeshPipelineError, TextureDescriptor, TextureDimension, TextureFormat,
    TextureUsages, VertexFormat,
};
use bevy::render::view::RenderLayers;
use bevy::render::{Render, RenderApp, RenderSystems, renderer::render_system};
use bevy::transform::TransformSystems;
use bevy_egui35::{EguiUserTextures, egui};
use plant_ui::{CameraGesture, CameraMotion, ModelAction};

pub mod mesh_source;

const INITIAL_SIZE: UVec2 = UVec2::new(1200, 700);
const RESIZE_DEBOUNCE: Duration = Duration::from_millis(120);
/// 模型单位（PDMS 毫米）到世界单位：1 mm = 0.01 世界单位，也就是
/// **1 世界单位 = 100 mm**。它只挂在 `SceneRoot` 上，所以场景里任何表示
/// 「多长」的数都要过这一层才有物理意义。
const MODEL_SCALE: f32 = 0.01;
/// 上一行的倒数。网格与三轴按真实长度反推世界尺寸走它，两个方向同源，
/// 改比例只动 `MODEL_SCALE` 一处。
const MM_PER_WORLD: f32 = 1.0 / MODEL_SCALE;
const HEADLIGHT_LIFT: f32 = 0.6;
const HEADLIGHT_SIDE: f32 = 0.35;

/// ViewCube 视角跳转的插值时长。Autodesk 规范里是 0.5s，S1-C 定 0.3s——
/// orbit 焦点不动，短一点更跟手。
const SNAP_SECS: f32 = 0.3;
/// 地面网格每侧线数与主线间隔：40×40 格、每 5 条一根主线（S1-C 规格）。
const GRID_HALF_LINES: i32 = 20;
const GRID_MAJOR_EVERY: i32 = 5;
/// 主 / 次网格线的线心不透明度；顶点色让线端渐隐到 0，充当「随距离淡出」。
const GRID_MAJOR_ALPHA: f32 = 0.5;
const GRID_MINOR_ALPHA: f32 = 0.22;
/// 视野里横着大致铺多少格；格距按它从相机距离反推。
const GRID_CELLS_ACROSS: f32 = 12.0;
/// 格距的上下限，用**真实长度**（毫米）表述：下界是模型自身的精度量级，
/// 上界够装下整个厂区。两端都取在档位上（1×10⁰ / 1×10⁶），所以夹过之后
/// 再落档不会又被推出界。
const GRID_MIN_MM: f32 = 1.0;
const GRID_MAX_MM: f32 = 1_000_000.0;
/// 开机档位：一格 1 世界单位。首帧 `update_grid` 就按相机距离改写它，这个值
/// 只决定「相机第一帧就位之前」那一瞬间的网格。
const INITIAL_GRID_LEVEL: f32 = 1.0;
/// 原点三轴长（单位：格）。整根轴挂在网格档位上等比缩放，取一格意味着它在
/// 屏幕上恒定占 `1 / GRID_CELLS_ACROSS` 的宽度，不随远近变大；单臂长度也正好
/// 等于 HUD 报的那句「一格 N m」，三轴顺带当比例尺。
const AXIS_CELLS: f32 = 1.0;
/// 轴端箭头高（同为格数）。箭头底面要严丝合缝接在杆末端、轴标签又要落在箭尖
/// 之外，两处位置都从它推，别再各写各的常数。
const AXIS_TIP_HEIGHT: f32 = 0.18;
/// X / Y / Z 轴的世界方向：PDMS 的 X、Y、Z 过了装载旋转就是这三条。
const AXIS_DIRS: [Vec3; 3] = [Vec3::X, Vec3::NEG_Z, Vec3::Y];
/// X 红 / Y 绿 / Z 蓝（S1-B 稿取值）。egui 侧轴标签用同一组色，两处要一致。
const AXIS_COLORS: [Color; 3] = [
    Color::srgb(0.851, 0.329, 0.302),
    Color::srgb(0.298, 0.686, 0.431),
    Color::srgb(0.294, 0.482, 0.898),
];
/// 背景面片的专用渲染层：背景相机只看它，主相机看不见它。
const BACKGROUND_LAYER: usize = 7;
/// 模型 PBR 参数：管道厂里的构件多是漆过的金属与混凝土，低金属度、偏粗糙，
/// 高光收敛一点才不至于每根管子都糊成一片镜面。金属度全交给贴图时代之前，
/// 这一档定死是从 rs-plant3-d 的观感里对出来的。
const MODEL_METALLIC: f32 = 0.1;
const MODEL_ROUGHNESS: f32 = 0.7;
/// 选中高亮色：橙红描不上边框（无 outline 依赖），直接换 base_color 最稳。
/// 与 rs-plant3-d outline 的 ORANGE_RED 同源。
const SELECT_COLOR: Color = Color::srgb(1.0, 0.27, 0.0);
/// 房间面板 X-Ray：淡蓝、约 25% 不透明度。
const XRAY_COLOR: Color = Color::srgba_u8(89, 169, 255, 64);
/// 无效 TUBI 诊断带的屏幕参数。宽度与节距全部使用物理像素，不随模型或镜头缩放。
const INVALID_TUBI_LINE_WIDTH_PX: f32 = 1.5;
const INVALID_TUBI_SELECTED_WIDTH_PX: f32 = 2.0;
const INVALID_TUBI_DASH_PX: f32 = 5.0;
const INVALID_TUBI_GAP_PX: f32 = 4.0;
const INVALID_TUBI_AA_PX: f32 = 0.75;
/// GPU 屏幕带在离屏视口上不可用时采用的真实长度虚线节距（模型数据单位为毫米）。
const INVALID_TUBI_DASH_MM: f32 = 100.0;
const INVALID_TUBI_GAP_MM: f32 = 80.0;
/// 开机默认配色（深色主题的 viewport tokens：#232F3A / #0E1318 / #46586A）。
/// 首帧 App 就会按当前主题发 `SetViewportBackground` 盖掉，这里只求
/// 「主题命令到达前别闪白」。
const DEFAULT_TOP: Color = Color::srgb(0.137, 0.184, 0.227);
const DEFAULT_BOTTOM: Color = Color::srgb(0.055, 0.075, 0.094);
const DEFAULT_GRID: Color = Color::srgb(0.275, 0.345, 0.416);

pub struct View3dPlugin;

const INVALID_TUBI_LINE_SHADER: Handle<Shader> =
    weak_handle!("fd65ad9b-1e39-4c83-8f44-d03b65fb443d");

/// 高编号避免与 Bevy 内置顶点属性碰撞；值为屏幕法线方向上的 `-1/+1`。
const INVALID_TUBI_LINE_SIDE: MeshVertexAttribute =
    MeshVertexAttribute::new("InvalidTubiLineSide", 1_734_510_291, VertexFormat::Float32);

impl Plugin for View3dPlugin {
    fn build(&self, app: &mut bevy::prelude::App) {
        load_internal_asset!(
            app,
            INVALID_TUBI_LINE_SHADER,
            "invalid_tubi_line.wgsl",
            Shader::from_wgsl
        );
        app.init_resource::<ViewportResizeSync>()
            .add_plugins(MaterialPlugin::<InvalidTubiLineMaterial> {
                prepass_enabled: false,
                shadows_enabled: false,
                ..default()
            })
            .add_plugins(ExtractResourcePlugin::<ViewportResizeSync>::default())
            .init_asset_loader::<MeshLoader>()
            .add_systems(Startup, setup)
            .add_systems(
                Update,
                (
                    load_models,
                    update_mesh_progress,
                    retry_meshes,
                    apply_resize,
                    apply_commands,
                    apply_selection,
                    animate_snap,
                    update_grid,
                )
                    .chain(),
            )
            .add_systems(
                PostUpdate,
                (
                    aim_headlight.before(TransformSystems::Propagate),
                    // 发布的是本帧真正用于渲染的姿态，所以要等传播完。
                    publish_camera.after(TransformSystems::Propagate),
                ),
            );
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app.add_systems(
                Render,
                acknowledge_viewport_render
                    .after(render_system)
                    .in_set(RenderSystems::Render),
            );
        }
    }
}

/// 屏幕空间无效 TUBI 虚线材质。`params = (line_width, dash, gap, aa)`，均为物理像素。
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
struct InvalidTubiLineMaterial {
    #[uniform(0)]
    color: LinearRgba,
    #[uniform(1)]
    params: Vec4,
}

impl InvalidTubiLineMaterial {
    fn new(color: Color, line_width_px: f32) -> Self {
        Self {
            color: color.to_linear(),
            params: Vec4::new(
                line_width_px,
                INVALID_TUBI_DASH_PX,
                INVALID_TUBI_GAP_PX,
                INVALID_TUBI_AA_PX,
            ),
        }
    }
}

impl Material for InvalidTubiLineMaterial {
    fn vertex_shader() -> ShaderRef {
        INVALID_TUBI_LINE_SHADER.into()
    }

    fn fragment_shader() -> ShaderRef {
        INVALID_TUBI_LINE_SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        // 抗锯齿覆盖率与 X-Ray 透明度都由片元 alpha 表达；仍沿用 Bevy 透明管线的
        // 深度比较，所以实体几何会照常遮挡虚线。
        AlphaMode::Blend
    }

    fn specialize(
        _pipeline: &MaterialPipeline<Self>,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.vertex.buffers = vec![layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            INVALID_TUBI_LINE_SIDE.at_shader_location(1),
        ])?];
        // 屏幕法线扩张后的绕序会随投影方向翻转，不能剔除任一面。
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// 主世界把待切换代次提取到 Render world；Render world 只有在整张 render graph
/// 跑完之后才确认该代次。两个原子量由两边共享，不把“过了一次 Update”误当成
/// “离屏相机真的画完了一帧”。
#[derive(Resource, Clone, ExtractResource)]
struct ViewportResizeSync {
    requested_generation: u64,
    rendered_generation: Arc<AtomicU64>,
    rendered_frames: Arc<AtomicU64>,
}

impl Default for ViewportResizeSync {
    fn default() -> Self {
        Self {
            requested_generation: 0,
            rendered_generation: Arc::new(AtomicU64::new(0)),
            rendered_frames: Arc::new(AtomicU64::new(0)),
        }
    }
}

impl ViewportResizeSync {
    fn acknowledge_rendered(&self) {
        self.rendered_generation
            .store(self.requested_generation, Ordering::Release);
        self.rendered_frames.fetch_add(1, Ordering::Release);
    }

    fn generation_is_rendered(&self, generation: u64) -> bool {
        self.rendered_generation.load(Ordering::Acquire) >= generation
    }

    fn rendered_frames(&self) -> u64 {
        self.rendered_frames.load(Ordering::Acquire)
    }
}

fn acknowledge_viewport_render(sync: Res<ViewportResizeSync>) {
    sync.acknowledge_rendered();
}

#[derive(Resource)]
pub struct View3d {
    pub texture: egui::TextureId,
    pub size: UVec2,
    /// 相机世界旋转的三列：right / up / back。egui 侧 ViewCube 的全部输入。
    pub camera_rot: [[f32; 3]; 3],
    /// X / Y / Z 轴端标签在渲染纹理上的归一化 UV；出画或在相机身后为 None。
    pub axis_labels: [Option<[f32; 2]>; 3],
    /// 当前地面网格的格距，**真实长度（毫米）**。HUD 的比例读数用它。
    /// 读数与网格必须是同一个数：另算一遍迟早与眼睛看见的格子对不上。
    pub grid_cell_mm: f32,
    image: Handle<Image>,
    commands: VecDeque<ViewCommand>,
    pending_models: VecDeque<ModelBatch>,
    loading_meshes: Vec<LoadingMesh>,
    /// 加载失败的网格，**一直留到重试成功为止**。
    ///
    /// 与 `loading_meshes` 那份「装完即清」的账不同：换过网格目录之后要重来的正是
    /// 这些，清掉就无从下手了。
    failed_meshes: Vec<LoadingMesh>,
    /// 已经重新发出、还没有结果的那些。
    retrying_meshes: Vec<RetryingMesh>,
    retry_requested: bool,
    mesh_progress: Option<MeshLoadProgress>,
    desired_visibility: HashMap<RefU64, bool>,
    /// 每个模型在场景里的**实际**渲染结果。与 `desired_visibility` 分开记：
    /// 那一份是收到的指令，这一份是指令真的落到实体和网格文件上之后的样子。
    render_states: HashMap<RefU64, RenderState>,
    /// 实际状态变过、还没被宿主取走的模型。
    render_dirty: HashSet<RefU64>,
    pending_resize: Option<(UVec2, Instant)>,
    /// 已经改给离屏相机、但还没交给 egui 的新目标。至少让相机完整画过一帧再切，
    /// 否则窗口相机会采到刚创建、内容尚空的 GPU 纹理。
    prepared_resize: Option<PreparedResize>,
    /// 已不再展示/写入、但上一帧的 egui paint job 或 render world 仍可能引用的目标。
    /// 再等一个确认完成的渲染帧才同时注销 TextureId 与 Image asset。
    retired_resize: VecDeque<RetiredResize>,
    picked: Option<RefU64>,
    selected: HashSet<RefU64>,
    selection_dirty: bool,
    /// 当前完整的 X-Ray 目标集；同时匹配模型 refno 与 owner。
    xray: HashSet<RefU64>,
    material_dirty: bool,
    bounds: HashMap<RefU64, (Vec3, Vec3)>,
    /// 隔离前的可见性快照（refno -> 是否可见）。`Some` = 正处于隔离中。
    /// 只记第一次：连续隔离退出时回到隔离前的世界，而不是上一间房。
    isolate_restore: Option<HashMap<RefU64, bool>>,
}

impl View3d {
    pub fn load(&mut self, models: Vec<GeomInstQuery>) {
        self.pending_models.clear();
        self.desired_visibility.clear();
        self.pending_models.push_back(ModelBatch::Replace(models));
    }

    /// 在当前场景上只追加新模型；调用方负责按 refno 去重。
    pub fn append(&mut self, models: Vec<GeomInstQuery>) {
        if models.is_empty() {
            return;
        }
        if let Some(ModelBatch::Append(pending)) = self.pending_models.back_mut() {
            pending.extend(models);
        } else {
            self.pending_models.push_back(ModelBatch::Append(models));
        }
    }

    pub fn resize(&mut self, size: [u32; 2]) {
        let wanted = UVec2::new(size[0].max(16), size[1].max(16));
        let prepared = self.prepared_resize.as_ref().map(|resize| resize.size);
        if (wanted == self.size && prepared.is_none()) || prepared == Some(wanted) {
            self.pending_resize = None;
        } else if self.pending_resize.map(|(size, _)| size) != Some(wanted) {
            self.pending_resize = Some((wanted, Instant::now()));
        }
    }

    pub fn camera(&mut self, gesture: CameraGesture) {
        self.commands.push_back(ViewCommand::Camera(gesture));
    }

    pub fn model(&mut self, action: ModelAction) {
        if let ModelAction::Unload { refnos } = &action {
            let removed: HashSet<_> = refnos.iter().copied().collect();
            for batch in &mut self.pending_models {
                batch.retain(|model| {
                    !removed.contains(&model.refno.refno())
                        && !removed.contains(&model.owner.refno())
                });
            }
        }
        record_model_visibility(&action, &mut self.desired_visibility);
        self.commands.push_back(ViewCommand::Model(action));
    }

    pub fn pick(&mut self, uv: [f32; 2]) {
        // TODO(诊断): 拾取排查完删除。
        diag(format!("[诊断] UI 发出拾取 uv={uv:?}"));
        self.commands.push_back(ViewCommand::Pick(uv));
    }

    /// 下发当前选择集。传空集即取消全部高亮。宿主每次选择变更都发一遍整集，
    /// 视口无状态地照它重刷——不发「加了谁减了谁」，跟树侧 `SetSelection` 同理。
    pub fn set_selection(&mut self, refnos: Vec<RefU64>) {
        let selected: HashSet<_> = refnos.into_iter().collect();
        if self.selected != selected {
            self.selected = selected;
            self.selection_dirty = true;
        }
    }

    /// 主题下发的视口配色：渐变上下两色进背景面片，网格色进网格重建。
    pub fn set_background(
        &mut self,
        top: egui::Color32,
        bottom: egui::Color32,
        grid: egui::Color32,
    ) {
        self.commands.push_back(ViewCommand::Background {
            top: srgb(top),
            bottom: srgb(bottom),
            grid: srgb(grid),
        });
    }

    /// ViewCube 的视角跳转。方向已是世界系（换算在 egui 侧的立方体模块）。
    pub fn snap(&mut self, forward: [f32; 3], up: [f32; 3], fit: bool) {
        self.commands.push_back(ViewCommand::Snap {
            forward: Vec3::from(forward).normalize_or(Vec3::NEG_Z),
            up: Vec3::from(up).normalize_or(Vec3::Y),
            fit,
        });
    }

    pub fn take_picked(&mut self) -> Option<RefU64> {
        self.picked.take()
    }

    /// 加载失败、还没重试成功的网格数。
    pub fn failed_mesh_count(&self) -> usize {
        self.failed_meshes.len()
    }

    /// 重来一遍那些失败的网格。换过网格目录之后由宿主发起。
    ///
    /// 只碰失败的那些：网格文件名是内容哈希，已经加载成功的换个目录取到的还是同一份
    /// 内容，重来一遍只是白花时间。返回这一轮重发了几个。
    pub fn retry_failed_meshes(&mut self) -> usize {
        if self.failed_meshes.is_empty() {
            return 0;
        }
        self.retry_requested = true;
        self.failed_meshes.len()
    }

    /// 取一拍 mesh 文件加载进度；终态只交付一次。
    pub fn take_mesh_progress(&mut self) -> Option<MeshLoadProgress> {
        let progress = self.mesh_progress.clone();
        if progress.as_ref().is_some_and(MeshLoadProgress::finished) {
            self.mesh_progress = None;
        }
        progress
    }

    /// 取走自上次以来实际渲染状态变过的模型。
    ///
    /// 装载途中的模型不在其中：它的网格还没画出来，这一刻把它算成「已显示」
    /// 就是抢在画面前面说话。它留在待回执里，等 mesh 全部有了结果再交付一次。
    pub fn take_render_states(&mut self) -> Vec<ModelRenderState> {
        if self.render_dirty.is_empty() {
            return Vec::new();
        }
        let Self {
            render_states,
            render_dirty,
            ..
        } = self;
        let mut settled = Vec::with_capacity(render_dirty.len());
        render_dirty.retain(|refno| {
            let Some(state) = render_states.get(refno) else {
                // 模型已经被整场替换掉了，这一条没有对应的实体，丢掉。
                return false;
            };
            if !state.settled() {
                return true;
            }
            settled.push(state.publish(*refno));
            false
        });
        settled
    }
}

struct PreparedResize {
    size: UVec2,
    image: Handle<Image>,
    texture: egui::TextureId,
    generation: u64,
}

struct RetiredResize {
    image: Handle<Image>,
    remove_after_rendered_frame: u64,
}

/// 一个模型此刻在三维场景里的**实际**渲染结果。
///
/// 与「下过的指令」相对：`visible` 是实体真的可见，两个 mesh 计数是网格文件
/// 真的加载成功 / 失败了几个。模型树的 eye 照它画，不照指令画。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelRenderState {
    pub refno: RefU64,
    pub visible: bool,
    /// 这个模型一共要加载几个网格文件。
    pub mesh_total: usize,
    pub mesh_loaded: usize,
    pub mesh_failed: usize,
}

/// 模型的实际渲染结果（视口内部账）。
struct RenderState {
    visible: bool,
    mesh_total: usize,
    /// 当场绑定共享内存网格的数量（无效 TUBI 的屏幕空间虚线带）。它们不走 `AssetServer`，
    /// 没有加载事件可等，所以要单独记一笔：`update_mesh_progress` 每帧从
    /// `loading_meshes` 重算的只是外部文件那一半，得把这一笔加回去才是全部。
    mesh_immediate: usize,
    mesh_loaded: usize,
    mesh_failed: usize,
}

impl RenderState {
    /// 每个网格文件都有了结果（成功或失败）。回执只在这之后发。
    fn settled(&self) -> bool {
        self.mesh_loaded + self.mesh_failed >= self.mesh_total
    }

    fn publish(&self, refno: RefU64) -> ModelRenderState {
        ModelRenderState {
            refno,
            visible: self.visible,
            mesh_total: self.mesh_total,
            mesh_loaded: self.mesh_loaded,
            mesh_failed: self.mesh_failed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeshLoadProgress {
    pub done: usize,
    pub total: usize,
    pub errors: Vec<String>,
}

impl MeshLoadProgress {
    pub fn finished(&self) -> bool {
        self.done == self.total
    }
}

struct LoadingMesh {
    /// 这个网格属于哪个模型。逐模型的成功 / 失败计数靠它归堆。
    refno: RefU64,
    path: String,
    handle: Handle<Mesh>,
}

/// 重发之后等结果的一个网格。
struct RetryingMesh {
    mesh: LoadingMesh,
    asked_at: Instant,
    /// 有没有亲眼见过它离开失败态。
    started: bool,
}

/// 重发之后多久，还没见它动过就认账。
///
/// `AssetServer::reload` 不给回执，只能等状态自己翻。没有这道兜底的话，一个压根
/// 没被重发的网格会永远挂在重试队列里，界面上那个红点也就永远不落地。
const RETRY_GRACE: Duration = Duration::from_secs(3);

impl RetryingMesh {
    /// 这一刻的失败态算不算数。
    fn settling(&self) -> bool {
        self.started || self.asked_at.elapsed() >= RETRY_GRACE
    }
}

enum ModelBatch {
    Replace(Vec<GeomInstQuery>),
    Append(Vec<GeomInstQuery>),
}

impl ModelBatch {
    fn clears_scene(&self) -> bool {
        matches!(self, Self::Replace(_))
    }

    fn into_models(self) -> Vec<GeomInstQuery> {
        match self {
            Self::Replace(models) | Self::Append(models) => models,
        }
    }

    fn retain(&mut self, keep: impl FnMut(&GeomInstQuery) -> bool) {
        match self {
            Self::Replace(models) | Self::Append(models) => models.retain(keep),
        }
    }
}

fn record_model_visibility(action: &ModelAction, desired: &mut HashMap<RefU64, bool>) {
    match action {
        ModelAction::SetVisible { refnos, visible } => {
            desired.extend(refnos.iter().copied().map(|refno| (refno, *visible)));
        }
        ModelAction::Unload { refnos } => {
            for refno in refnos {
                desired.remove(refno);
            }
        }
        ModelAction::HideAll => desired.values_mut().for_each(|visible| *visible = false),
        ModelAction::ShowAll => desired.values_mut().for_each(|visible| *visible = true),
        _ => {}
    }
}

/// 把一条刚刚**落到实体上**的可见性写进实际状态账，并给已经装完的模型排回执。
///
/// 还在装网格的模型只更新账不排回执：它的 eye 要等 mesh 有了结果才第一次动，
/// 那一刻交付的就是这里记下的最后一次方向——装载途中反向点击照样以最后一次为准。
fn record_applied_visibility(view: &mut View3d, applied: Vec<RefU64>, visible: bool) {
    for refno in applied {
        let Some(state) = view.render_states.get_mut(&refno) else {
            continue;
        };
        state.visible = visible;
        if state.settled() {
            view.render_dirty.insert(refno);
        }
    }
}

/// 只有落进空场景的那一批需要自动取景：首个模型进场时相机多半还停在出厂
/// 位置，不取景就是一屏空白。整场替换不再取景——取回工作的约定是回到取回
/// 前的样子，相机也算在内（docs/plans/get-work-clear-and-reload.md 决定 5）。
fn should_frame_batch(scene_empty: bool) -> bool {
    scene_empty
}

fn srgb(c: egui::Color32) -> Color {
    Color::srgba_u8(c.r(), c.g(), c.b(), c.a())
}

// TODO(诊断): 拾取排查完删除。
fn diag(msg: String) {
    #[cfg(target_arch = "wasm32")]
    web_sys::console::warn_1(&msg.as_str().into());
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("{msg}");
}

enum ViewCommand {
    Camera(CameraGesture),
    Model(ModelAction),
    Pick([f32; 2]),
    Background {
        top: Color,
        bottom: Color,
        grid: Color,
    },
    Snap {
        forward: Vec3,
        up: Vec3,
        fit: bool,
    },
}

#[derive(Component)]
struct ViewCamera;

#[derive(Component)]
struct Headlight;

#[derive(Component)]
struct SceneRoot;

#[derive(Component, Clone, Copy)]
struct ModelRoot {
    refno: RefU64,
    owner: RefU64,
}

#[derive(Component, Clone)]
struct ModelMesh {
    refno: RefU64,
    owner: RefU64,
    /// 该网格按类型算出的常态材质。选中高亮换成 [`ModelMesh::highlight`]，
    /// 取消选中时靠它还原——不存的话就得重算颜色，还得重新走一遍类型解析。
    base: Handle<StandardMaterial>,
    /// 该网格选中时该换成哪枚材质。
    highlight: Handle<StandardMaterial>,
}

/// 拾取只依赖模型身份，不依赖它使用 StandardMaterial 还是专用虚线材质。
#[derive(Component, Clone, Copy)]
struct ModelPickTarget {
    refno: RefU64,
}

/// 无效 TUBI 使用专用屏幕空间材质；身份与常态/选中句柄独立于实体网格保存。
#[derive(Component, Clone)]
struct InvalidTubiLine {
    refno: RefU64,
    owner: RefU64,
    base: Handle<InvalidTubiLineMaterial>,
    highlight: Handle<InvalidTubiLineMaterial>,
}

/// 选中高亮共用的一枚材质句柄；换选择集只改各网格指向哪枚材质，不新建材质。
#[derive(Resource)]
struct HighlightMaterial(Handle<StandardMaterial>);

/// 无效 TUBI 的 `LineList` 高亮不参与光照，避免随视角变暗消失。
#[derive(Resource)]
struct LineHighlightMaterial(Handle<StandardMaterial>);

/// 全部无效 TUBI 共用的一枚四顶点单位带；长度完全来自实例的局部 Z 缩放。
#[derive(Resource)]
struct InvalidTubiLineMesh(Handle<Mesh>);

impl InvalidTubiLineMesh {
    fn handle(&self) -> Handle<Mesh> {
        self.0.clone()
    }
}

/// 无效 TUBI 的全局状态材质。Base 继续按类型色缓存，只有 Selected / X-Ray 共用。
#[derive(Resource)]
struct InvalidTubiLineStateMaterials {
    highlight: Handle<InvalidTubiLineMaterial>,
    xray: Handle<InvalidTubiLineMaterial>,
}

/// 房间面板共用的一枚半透明材质；换房间只替换目标集合。
#[derive(Resource)]
struct XRayMaterial(Handle<StandardMaterial>);

/// 原点三轴的挂点；网格换级时改它的 scale，轴跟着格距伸缩。
#[derive(Component)]
struct AxesRoot;

/// 渐变背景面片的网格句柄；换主题改它的顶点色。
#[derive(Resource)]
struct BackgroundMesh(Handle<Mesh>);

#[derive(Resource)]
struct GridMesh(Handle<Mesh>);

/// 网格现状：当前格距、线色，以及「配色变了、同级也要重建」的脏标记。
#[derive(Resource)]
struct GridState {
    level: f32,
    color: Color,
    rebuild: bool,
}

/// 一次进行中的视角跳转。`t` 走 [0,1]，smoothstep 缓动。
struct SnapAnim {
    from_pos: Vec3,
    from_rot: Quat,
    to_pos: Vec3,
    to_rot: Quat,
    t: f32,
}

#[derive(Resource, Default)]
struct OrbitCamera {
    focus: Vec3,
    motion: Option<CameraMotion>,
    /// 进行中的 Snap 动画。任何手势或即时取景立刻打断——人比动画大。
    anim: Option<SnapAnim>,
}

#[derive(Default)]
struct MeshLoader;

impl AssetLoader for MeshLoader {
    type Asset = Mesh;
    type Settings = ();
    type Error = anyhow::Error;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &Self::Settings,
        _load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        Ok(PlantMesh::des_from_bytes(&bytes)?.gen_bevy_mesh())
    }

    fn extensions(&self) -> &[&str] {
        &["mesh"]
    }
}

fn viewport_image(size: UVec2) -> Image {
    let extent = Extent3d {
        width: size.x,
        height: size.y,
        ..default()
    };
    let mut image = Image {
        texture_descriptor: TextureDescriptor {
            label: Some("plant-ui 3d viewport"),
            size: extent,
            dimension: TextureDimension::D2,
            format: TextureFormat::Bgra8UnormSrgb,
            mip_level_count: 1,
            sample_count: 1,
            usage: TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_DST
                | TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        },
        ..default()
    };
    image.resize(extent);
    image
}

/// 所有写入三维离屏目标的相机。换目标必须一起换，否则背景与模型会分写到两张图。
#[derive(Component)]
struct ViewportRenderCamera;

fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut egui_textures: ResMut<EguiUserTextures>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut line_materials: ResMut<Assets<InvalidTubiLineMaterial>>,
) {
    let image = images.add(viewport_image(INITIAL_SIZE));
    let texture = egui_textures.add_image(image.clone());
    commands.insert_resource(View3d {
        texture,
        size: INITIAL_SIZE,
        camera_rot: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        axis_labels: [None; 3],
        grid_cell_mm: INITIAL_GRID_LEVEL * MM_PER_WORLD,
        image: image.clone(),
        commands: VecDeque::new(),
        pending_models: VecDeque::new(),
        loading_meshes: Vec::new(),
        failed_meshes: Vec::new(),
        retrying_meshes: Vec::new(),
        retry_requested: false,
        mesh_progress: None,
        desired_visibility: HashMap::new(),
        render_states: HashMap::new(),
        render_dirty: HashSet::new(),
        pending_resize: None,
        prepared_resize: None,
        retired_resize: VecDeque::new(),
        picked: None,
        selected: HashSet::new(),
        selection_dirty: false,
        xray: HashSet::new(),
        material_dirty: false,
        bounds: HashMap::new(),
        isolate_restore: None,
    });
    commands.insert_resource(OrbitCamera::default());

    // 背景：独立相机先画一张全屏渐变面片（拷问定案第 2 题，用户点名 Bevy 内画），
    // 主相机不清色、按序叠在上面。渐变色是顶点色，换主题只动四枚顶点。
    let bg_mesh = meshes.add(gradient_mesh(DEFAULT_TOP, DEFAULT_BOTTOM));
    commands.insert_resource(BackgroundMesh(bg_mesh.clone()));
    commands.spawn((
        Mesh3d(bg_mesh),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::WHITE,
            unlit: true,
            // 背景相机与主相机共用离屏目标；背景只写颜色，不能留下挡住模型的深度。
            alpha_mode: AlphaMode::Blend,
            ..default()
        })),
        Transform::IDENTITY,
        RenderLayers::layer(BACKGROUND_LAYER),
    ));
    commands.spawn((
        Camera3d::default(),
        Camera {
            order: -1,
            is_active: false,
            target: RenderTarget::Image(image.clone().into()),
            clear_color: ClearColorConfig::Custom(DEFAULT_BOTTOM),
            ..default()
        },
        // 定宽定高的正交投影：面片就是 2×2，视口怎么变形都恰好铺满。
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::Fixed {
                width: 2.0,
                height: 2.0,
            },
            near: 0.1,
            far: 10.0,
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_xyz(0.0, 0.0, 1.0).looking_at(Vec3::ZERO, Vec3::Y),
        Tonemapping::None,
        RenderLayers::layer(BACKGROUND_LAYER),
        ViewportRenderCamera,
    ));

    commands.spawn((
        Camera3d::default(),
        Camera {
            target: RenderTarget::Image(image.into()),
            clear_color: ClearColorConfig::Custom(Color::NONE),
            ..default()
        },
        Transform::from_xyz(8.0, 6.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
        Tonemapping::None,
        ViewCamera,
        ViewportRenderCamera,
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 1_600.0,
            shadows_enabled: false,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.8, -0.7, 0.0)),
        Headlight,
    ));
    // 环境光：PBR 下背光面若全黑，模型会看着像剪影。给一档柔和环境把暗部提起来，
    // 之前 unlit 时不需要它，这次开 PBR 才补上。
    commands.insert_resource(AmbientLight {
        color: Color::WHITE,
        brightness: 220.0,
        affects_lightmapped_meshes: false,
    });
    commands.insert_resource(HighlightMaterial(
        materials.add(model_material(SELECT_COLOR)),
    ));
    commands.insert_resource(LineHighlightMaterial(
        materials.add(fallback_line_material(SELECT_COLOR)),
    ));
    commands.insert_resource(XRayMaterial(materials.add(xray_material())));
    commands.insert_resource(InvalidTubiLineStateMaterials {
        highlight: line_materials.add(InvalidTubiLineMaterial::new(
            SELECT_COLOR,
            INVALID_TUBI_SELECTED_WIDTH_PX,
        )),
        xray: line_materials.add(InvalidTubiLineMaterial::new(
            XRAY_COLOR,
            INVALID_TUBI_LINE_WIDTH_PX,
        )),
    });
    commands.insert_resource(InvalidTubiLineMesh(meshes.add(invalid_tubi_line_mesh())));

    // 地面网格（PDMS Z=0，过装载旋转即世界 y=0 面）与原点三色轴。
    let grid_mesh = meshes.add(build_grid_mesh(INITIAL_GRID_LEVEL, DEFAULT_GRID));
    commands.insert_resource(GridMesh(grid_mesh.clone()));
    commands.insert_resource(GridState {
        level: INITIAL_GRID_LEVEL,
        color: DEFAULT_GRID,
        rebuild: false,
    });
    commands.spawn((
        Mesh3d(grid_mesh),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::WHITE,
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        })),
        Transform::IDENTITY,
    ));
    // 杆径不跟着臂长等比走：臂长收到一格后按比例算出来的杆会细进亚像素，
    // 这里单独定在屏幕上约 3px 的粗细上。
    let rod_mesh = meshes.add(Cylinder::new(0.015, AXIS_CELLS));
    let tip_mesh = meshes.add(Cone {
        radius: 0.05,
        height: AXIS_TIP_HEIGHT,
    });
    // 把「+Y 朝向」的圆柱掰到各轴的世界方向上：X 红、Y(PDMS) 绿即 -Z、Z(PDMS) 蓝即 +Y。
    let axis_rotations = [
        Quat::from_rotation_z(-FRAC_PI_2),
        Quat::from_rotation_x(-FRAC_PI_2),
        Quat::IDENTITY,
    ];
    commands
        .spawn((Transform::IDENTITY, Visibility::Visible, AxesRoot))
        .with_children(|root| {
            for (axis, rotation) in axis_rotations.into_iter().enumerate() {
                let material = materials.add(StandardMaterial {
                    base_color: AXIS_COLORS[axis],
                    unlit: true,
                    ..default()
                });
                root.spawn((Transform::from_rotation(rotation), Visibility::Visible))
                    .with_children(|arm| {
                        arm.spawn((
                            Mesh3d(rod_mesh.clone()),
                            MeshMaterial3d(material.clone()),
                            Transform::from_xyz(0.0, AXIS_CELLS / 2.0, 0.0),
                        ));
                        arm.spawn((
                            Mesh3d(tip_mesh.clone()),
                            MeshMaterial3d(material),
                            Transform::from_xyz(0.0, AXIS_CELLS + AXIS_TIP_HEIGHT / 2.0, 0.0),
                        ));
                    });
            }
        });
}

/// 全屏渐变面片：2×2 的四边形，顶点色下底上顶。配正交 Fixed{2,2} 相机恰好铺满。
fn gradient_mesh(top: Color, bottom: Color) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![
            [-1.0, -1.0, 0.0],
            [1.0, -1.0, 0.0],
            [1.0, 1.0, 0.0],
            [-1.0, 1.0, 0.0],
        ],
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 4]);
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, gradient_colors(top, bottom));
    mesh.insert_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]));
    mesh
}

fn gradient_colors(top: Color, bottom: Color) -> Vec<[f32; 4]> {
    let t = top.to_linear().to_f32_array();
    let b = bottom.to_linear().to_f32_array();
    vec![b, b, t, t]
}

/// 地面网格线网：LineList，每条线两段、线心实两端透——一份顶点色同时管
/// 主次线分级与「随距离淡出」，不烧任何着色器。
fn build_grid_mesh(cell: f32, color: Color) -> Mesh {
    let base = color.to_linear().to_f32_array();
    let extent = GRID_HALF_LINES as f32 * cell;
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut push_line = |a: Vec3, mid: Vec3, b: Vec3, alpha: f32| {
        let solid = [base[0], base[1], base[2], alpha];
        let clear = [base[0], base[1], base[2], 0.0];
        positions.extend([a, mid, mid, b].map(|v| v.to_array()));
        colors.extend([clear, solid, solid, clear]);
    };
    for k in -GRID_HALF_LINES..=GRID_HALF_LINES {
        let off = k as f32 * cell;
        let alpha = if k % GRID_MAJOR_EVERY == 0 {
            GRID_MAJOR_ALPHA
        } else {
            GRID_MINOR_ALPHA
        };
        push_line(
            Vec3::new(-extent, 0.0, off),
            Vec3::new(0.0, 0.0, off),
            Vec3::new(extent, 0.0, off),
            alpha,
        );
        push_line(
            Vec3::new(off, 0.0, -extent),
            Vec3::new(off, 0.0, 0.0),
            Vec3::new(off, 0.0, extent),
            alpha,
        );
    }
    let mut mesh = Mesh::new(PrimitiveTopology::LineList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh
}

/// 相机离焦点多远，格距取多少**真实长度**（毫米）。
///
/// 换档判断全在毫米上做。在世界单位上取 10 的整幂得出的是「一格 100 mm」
/// 这种没人报得出口的数，而界面上又没有地方说明一格多长；落到
/// 1/2/5×10ⁿ 这套工程与地图通用的档位上，每一档都是能直接念出来的长度，
/// 相邻档之间也只差 2～2.5 倍，不像纯 10 次幂那样一跳就是十倍。
fn grid_cell_mm(dist: f32) -> f32 {
    // 先夹再落档：夹的是真实长度，且两端本身就在档位上，落档不会再推出界。
    // 夹在前面还顺带挡掉 `dist = 0` 时的退化对数。
    let wanted =
        ((dist.max(0.0) / GRID_CELLS_ACROSS) * MM_PER_WORLD).clamp(GRID_MIN_MM, GRID_MAX_MM);
    snap_grid_step(wanted)
}

/// 同上，换算成网格与三轴直接使用的世界单位。
fn grid_level(dist: f32) -> f32 {
    grid_cell_mm(dist) * MODEL_SCALE
}

/// 把一个真实长度落到最近的 1/2/5×10ⁿ 档。
///
/// 档位边界取相邻两档的**几何**中点：拉远与拉近的换档时机因此对称。用算术
/// 中点的话同一个格距在放大时和缩小时会在不同的距离上跳档，来回滚一次滚轮
/// 就能看出网格在同一位置上闪。
fn snap_grid_step(mm: f32) -> f32 {
    let decade = 10f32.powi(mm.max(f32::MIN_POSITIVE).log10().floor() as i32);
    match mm / decade {
        mantissa if mantissa < std::f32::consts::SQRT_2 => decade,
        // √10 与 √50：1↔2、2↔5、5↔10 三处交界。
        mantissa if mantissa < 3.162_277_7 => decade * 2.0,
        mantissa if mantissa < 7.071_068 => decade * 5.0,
        _ => decade * 10.0,
    }
}

fn headlight_aim(camera: &Transform) -> Option<Vec3> {
    (*camera.forward() + *camera.right() * HEADLIGHT_SIDE - Vec3::Y * HEADLIGHT_LIFT)
        .try_normalize()
}

fn aim_headlight(
    camera: Query<&Transform, (With<ViewCamera>, Without<Headlight>)>,
    mut lights: Query<&mut Transform, With<Headlight>>,
) {
    let Ok(camera) = camera.single() else {
        return;
    };
    let Some(aim) = headlight_aim(camera) else {
        return;
    };
    for mut light in &mut lights {
        light.look_to(aim, Vec3::Y);
    }
}

/// 把相机姿态与轴端标签位置发布给绘制层。ViewCube 与轴标签都长在 egui 里，
/// 它们要的「相机现在朝哪」只有这一侧有真值。
fn publish_camera(
    mut view: ResMut<View3d>,
    grid: Res<GridState>,
    camera: Query<(&Camera, &GlobalTransform), With<ViewCamera>>,
) {
    let Ok((camera, transform)) = camera.single() else {
        return;
    };
    let m = Mat3::from_quat(transform.rotation());
    view.camera_rot = [
        m.x_axis.to_array(),
        m.y_axis.to_array(),
        m.z_axis.to_array(),
    ];
    view.grid_cell_mm = grid.level * MM_PER_WORLD;
    // 标签落在箭尖再往外一个箭头高的位置，跟轴端留出一点空隙。
    let tip = (AXIS_CELLS + AXIS_TIP_HEIGHT * 2.0) * grid.level;
    for (axis, dir) in AXIS_DIRS.into_iter().enumerate() {
        view.axis_labels[axis] = camera
            .world_to_ndc(transform, dir * tip)
            .filter(|ndc| ndc.z > 0.0 && ndc.z < 1.0)
            .map(|ndc| [ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5]);
    }
}

/// 构件类型 -> 基色。原样搬 rs-plant3-d 的 `default_color_rules`：
/// 管道亮黄、结构天蓝、设备金黄、墙灰、房间/楼板米色……CE（当前元素）本是
/// 半透明蓝。查不到的类型落到 UNKOWN 的米色。
fn type_color(generic: &str) -> Color {
    let ty = PdmsGenericType::from_str(generic).unwrap_or(PdmsGenericType::UNKOWN);
    match ty {
        PdmsGenericType::PIPE => Color::srgb_u8(250, 250, 30),
        PdmsGenericType::STRU => Color::srgb_u8(25, 184, 241),
        PdmsGenericType::EQUI => Color::srgb_u8(255, 190, 0),
        PdmsGenericType::CE => Color::srgba_u8(44, 84, 220, 200),
        PdmsGenericType::SCTN | PdmsGenericType::GENSEC => Color::srgb_u8(188, 141, 125),
        PdmsGenericType::HANG => Color::srgb_u8(255, 126, 0),
        PdmsGenericType::WALL | PdmsGenericType::STWALL | PdmsGenericType::GWALL => {
            Color::srgb_u8(150, 150, 150)
        }
        // ROOM / PANE / FLOOR / HVAC 及其余一律米色（rs-plant3-d 的 UNKOWN 取值）。
        _ => Color::srgb_u8(213, 191, 169),
    }
}

/// 一个类型的 PBR 材质。双面 + 关背面剔除保留原壳
/// 「管壁两面都画」的行为——PDMS 网格法线不保证朝外，剔一面会漏。
fn model_material(color: Color) -> StandardMaterial {
    StandardMaterial {
        base_color: color,
        alpha_mode: if color.alpha() < 1.0 {
            AlphaMode::Blend
        } else {
            AlphaMode::Opaque
        },
        metallic: MODEL_METALLIC,
        perceptual_roughness: MODEL_ROUGHNESS,
        double_sided: true,
        cull_mode: None,
        ..default()
    }
}

fn fallback_line_material(color: Color) -> StandardMaterial {
    StandardMaterial {
        unlit: true,
        ..model_material(color)
    }
}

/// 在局部 `+Z` 的 `[0, 1]` 范围内生成真实长度虚线。两端都落在线段端点，实例原点
/// 仍是连接起点；短于一个节距的错误管段退化为一条完整诊断线，不会彻底消失。
fn invalid_tubi_fallback_line_mesh(axis_length_mm: f32) -> Mesh {
    let length_mm = axis_length_mm.abs();
    let mut positions = Vec::new();
    if !length_mm.is_finite() || length_mm <= INVALID_TUBI_DASH_MM + INVALID_TUBI_GAP_MM {
        positions.extend([[0.0, 0.0, 0.0], [0.0, 0.0, 1.0]]);
    } else {
        let dash_count = ((length_mm + INVALID_TUBI_GAP_MM)
            / (INVALID_TUBI_DASH_MM + INVALID_TUBI_GAP_MM))
            .ceil()
            .max(2.0) as usize;
        let dash_mm =
            (length_mm - INVALID_TUBI_GAP_MM * (dash_count - 1) as f32) / dash_count as f32;
        for index in 0..dash_count {
            let start_mm = index as f32 * (dash_mm + INVALID_TUBI_GAP_MM);
            let end_mm = start_mm + dash_mm;
            positions.extend([
                [0.0, 0.0, start_mm / length_mm],
                [0.0, 0.0, end_mm / length_mm],
            ]);
        }
    }
    let normals = vec![[0.0, 1.0, 0.0]; positions.len()];
    let mut mesh = Mesh::new(PrimitiveTopology::LineList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh
}

/// 为全部无效 TUBI 创建共享的局部 +Z 单位带。局部端点严格为 `[0, 1]`：实例原点
/// 就是起始连接点，既不以中心对称，也不会向 `Z < 0` 反向延长。
fn invalid_tubi_line_mesh() -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, 1.0],
        ],
    );
    mesh.insert_attribute(INVALID_TUBI_LINE_SIDE, vec![-1.0, 1.0, -1.0, 1.0]);
    mesh.insert_indices(Indices::U32(vec![0, 1, 2, 2, 1, 3]));
    mesh
}

fn xray_material() -> StandardMaterial {
    StandardMaterial {
        base_color: XRAY_COLOR,
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        double_sided: true,
        cull_mode: None,
        ..default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MaterialState {
    Base,
    Selected,
    XRay,
}

impl MaterialState {
    fn choose<M: Asset>(
        self,
        base: &Handle<M>,
        highlight: &Handle<M>,
        xray: &Handle<M>,
    ) -> Handle<M> {
        match self {
            Self::Base => base.clone(),
            Self::Selected => highlight.clone(),
            Self::XRay => xray.clone(),
        }
    }
}

/// 普通实体与无效 TUBI 共用的唯一状态裁决：X-Ray > Selected > Base，且 refno / owner
/// 任一命中都生效。
fn material_state(
    selected: &HashSet<RefU64>,
    xray: &HashSet<RefU64>,
    refno: RefU64,
    owner: RefU64,
) -> MaterialState {
    if xray.contains(&refno) || xray.contains(&owner) {
        MaterialState::XRay
    } else if selected.contains(&refno) || selected.contains(&owner) {
        MaterialState::Selected
    } else {
        MaterialState::Base
    }
}

fn forget_model_state(view: &mut View3d, removed: &HashSet<RefU64>) {
    view.desired_visibility
        .retain(|refno, _| !removed.contains(refno));
    view.render_states
        .retain(|refno, _| !removed.contains(refno));
    view.render_dirty.retain(|refno| !removed.contains(refno));
    view.bounds.retain(|refno, _| !removed.contains(refno));
    view.loading_meshes
        .retain(|mesh| !removed.contains(&mesh.refno));
    view.failed_meshes
        .retain(|mesh| !removed.contains(&mesh.refno));
    view.retrying_meshes
        .retain(|mesh| !removed.contains(&mesh.mesh.refno));
    view.selected.retain(|refno| !removed.contains(refno));
    view.xray.retain(|refno| !removed.contains(refno));
    if let Some(snapshot) = &mut view.isolate_restore {
        snapshot.retain(|refno, _| !removed.contains(refno));
    }
    view.selection_dirty = true;
    view.material_dirty = true;
}

#[allow(clippy::too_many_arguments)]
fn load_models(
    mut commands: Commands,
    mut view: ResMut<View3d>,
    mut orbit: ResMut<OrbitCamera>,
    mut camera: Query<(&mut Transform, &mut Projection), With<ViewCamera>>,
    roots: Query<Entity, With<SceneRoot>>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut line_materials: ResMut<Assets<InvalidTubiLineMaterial>>,
    highlight: Res<HighlightMaterial>,
    line_highlight: Res<LineHighlightMaterial>,
    xray_material: Res<XRayMaterial>,
    invalid_line_mesh: Res<InvalidTubiLineMesh>,
    invalid_line_state: Res<InvalidTubiLineStateMaterials>,
) {
    let Some(batch) = view.pending_models.pop_front() else {
        return;
    };
    let replace = batch.clears_scene();
    let frame = should_frame_batch(roots.is_empty());
    let models = batch.into_models();
    if replace {
        view.bounds.clear();
        view.loading_meshes.clear();
        // 整场换掉，上一场的失败连同它指向的实体一起作废。
        view.failed_meshes.clear();
        view.retrying_meshes.clear();
        view.mesh_progress = None;
        view.render_states.clear();
        view.render_dirty.clear();
    }
    let scene_transform = Transform::from_scale(Vec3::splat(MODEL_SCALE))
        * Transform::from_rotation(Quat::from_rotation_x(-FRAC_PI_2));
    let scene_matrix = scene_transform.compute_matrix();
    for model in &models {
        let center = model.world_aabb.center();
        let center = Vec3::new(center.x, center.y, center.z);
        let half = model.world_aabb.half_extents();
        let half = Vec3::new(half.x, half.y, half.z);
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for x in [-half.x, half.x] {
            for y in [-half.y, half.y] {
                for z in [-half.z, half.z] {
                    let point = scene_matrix.transform_point3(center + Vec3::new(x, y, z));
                    min = min.min(point);
                    max = max.max(point);
                }
            }
        }
        extend_bounds(&mut view.bounds, model.refno.refno(), min, max);
        extend_bounds(&mut view.bounds, model.owner.refno(), min, max);
    }
    if frame {
        let mut all_min = Vec3::splat(f32::INFINITY);
        let mut all_max = Vec3::splat(f32::NEG_INFINITY);
        for &(min, max) in view.bounds.values() {
            all_min = all_min.min(min);
            all_max = all_max.max(max);
        }
        if all_min.is_finite()
            && all_max.is_finite()
            && let Ok((mut camera, mut projection)) = camera.single_mut()
        {
            frame_bounds(all_min, all_max, &mut orbit, &mut camera, &mut projection);
        }
    }
    if replace {
        for entity in &roots {
            commands.entity(entity).despawn();
        }
    }
    let scene = if replace {
        commands
            .spawn((scene_transform, Visibility::Visible, SceneRoot))
            .id()
    } else {
        roots.iter().next().unwrap_or_else(|| {
            commands
                .spawn((scene_transform, Visibility::Visible, SceneRoot))
                .id()
        })
    };
    // 一类一枚材质：同类构件成千上万，逐网格建材质既费显存也断批。按基色缓存，
    // 同色（含 UNKOWN 兜底）只落一枚。
    let mut material_cache: HashMap<[u8; 4], Handle<StandardMaterial>> = HashMap::new();
    let mut fallback_line_material_cache: HashMap<[u8; 4], Handle<StandardMaterial>> =
        HashMap::new();
    let mut fallback_line_mesh_cache: HashMap<u32, Handle<Mesh>> = HashMap::new();
    // 无效 TUBI 的屏幕空间材质同样按类型色缓存；Mesh 则全场只用资源里的那一枚。
    let mut line_material_cache: HashMap<[u8; 4], Handle<InvalidTubiLineMaterial>> = HashMap::new();
    let selected = view.selected.clone();
    let xray = view.xray.clone();
    let desired_visibility = view.desired_visibility.clone();
    let mut loading_meshes = Vec::new();
    let mut spawned: Vec<(RefU64, bool, usize, usize)> = Vec::new();
    commands.entity(scene).with_children(|scene| {
        for model in models {
            let refno = model.refno.refno();
            let owner = model.owner.refno();
            let color = type_color(&model.generic);
            let key = color.to_srgba().to_u8_array();
            let material = material_cache
                .entry(key)
                .or_insert_with(|| materials.add(model_material(color)))
                .clone();
            let state = material_state(&selected, &xray, refno, owner);
            let visible = desired_visibility
                .get(&refno)
                .or_else(|| desired_visibility.get(&owner))
                .copied()
                .unwrap_or(true);
            let visibility = if visible {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
            let mesh_total = model.insts.len();
            let mesh_immediate = model
                .insts
                .iter()
                .filter(|inst| inst.is_invalid_tubi)
                .count();
            spawned.push((refno, visible, mesh_total, mesh_immediate));
            // 只有真挂着无效 TUBI 的模型才建线材质。
            let line_material = (mesh_immediate > 0).then(|| {
                line_material_cache
                    .entry(key)
                    .or_insert_with(|| {
                        line_materials.add(InvalidTubiLineMaterial::new(
                            color,
                            INVALID_TUBI_LINE_WIDTH_PX,
                        ))
                    })
                    .clone()
            });
            scene
                .spawn((model.world_trans, visibility, ModelRoot { refno, owner }))
                .with_children(|element| {
                    for inst in model.insts {
                        if inst.is_invalid_tubi {
                            // 这里使用 Bevy 原生 LineList，而不是自定义屏幕带。后者的材质
                            // bind group / 离屏管线一旦失配会整段不画；LineList 已在同一实库
                            // 场景验证过，且仍保留正确的起点、方向、深度遮挡和选择状态。
                            let base = fallback_line_material_cache
                                .entry(key)
                                .or_insert_with(|| materials.add(fallback_line_material(color)))
                                .clone();
                            let shown = state.choose(&base, &line_highlight.0, &xray_material.0);
                            let axis_length_mm =
                                (model.world_trans.scale.z * inst.transform.scale.z).abs();
                            let mesh = fallback_line_mesh_cache
                                .entry(axis_length_mm.to_bits())
                                .or_insert_with(|| {
                                    meshes.add(invalid_tubi_fallback_line_mesh(axis_length_mm))
                                })
                                .clone();
                            element.spawn((
                                Mesh3d(mesh),
                                MeshMaterial3d(shown),
                                inst.transform,
                                ModelMesh {
                                    refno,
                                    owner,
                                    base,
                                    highlight: line_highlight.0.clone(),
                                },
                                ModelPickTarget { refno },
                                NotShadowCaster,
                                NotShadowReceiver,
                            ));
                        } else {
                            let path = mesh_source::asset_path(&inst.geo_hash);
                            let handle = assets.load(path.clone());
                            loading_meshes.push(LoadingMesh {
                                refno,
                                path,
                                handle: handle.clone(),
                            });
                            let shown = state.choose(&material, &highlight.0, &xray_material.0);
                            element.spawn((
                                Mesh3d(handle),
                                MeshMaterial3d(shown),
                                inst.transform,
                                ModelMesh {
                                    refno,
                                    owner,
                                    base: material.clone(),
                                    highlight: highlight.0.clone(),
                                },
                                ModelPickTarget { refno },
                            ));
                        }
                    }
                });
        }
    });
    // 新模型先记账再等 mesh：`settled()` 在这一刻还是 false，所以宿主取不到它，
    // eye 停在原来的样子直到网格真的有了结果。零网格的那些当场就是终态。
    for (refno, visible, mesh_total, mesh_immediate) in spawned {
        let state = view.render_states.entry(refno).or_insert(RenderState {
            visible,
            mesh_total: 0,
            mesh_immediate: 0,
            mesh_loaded: 0,
            mesh_failed: 0,
        });
        state.visible = visible;
        state.mesh_total += mesh_total;
        state.mesh_immediate += mesh_immediate;
        state.mesh_loaded += mesh_immediate;
        view.render_dirty.insert(refno);
    }
    view.loading_meshes.extend(loading_meshes);
    // Even an empty replacement is observable only after this system has cleared the old
    // scene. Reporting a terminal 0/0 batch here lets the host publish its refresh generation
    // on the following frame instead of doing so before `View3d::load` is consumed.
    view.mesh_progress = Some(begin_mesh_progress(view.loading_meshes.len()));
}

fn begin_mesh_progress(total: usize) -> MeshLoadProgress {
    MeshLoadProgress {
        done: 0,
        total,
        errors: Vec::new(),
    }
}

fn update_mesh_progress(mut view: ResMut<View3d>, assets: Res<AssetServer>) {
    if view.loading_meshes.is_empty() {
        return;
    }
    let mut done = 0;
    let mut errors = Vec::new();
    let mut failed_paths = HashSet::new();
    // 逐模型的成功 / 失败数是**重算**出来的而不是累加的：这张表在整批装完之前
    // 一直留着，每帧从头数一遍才不会因为多跑几帧就把同一个网格记两次。
    let mut counts: HashMap<RefU64, (usize, usize)> = HashMap::new();
    for mesh in &view.loading_meshes {
        match assets.get_load_state(&mesh.handle) {
            Some(LoadState::Loaded) => {
                done += 1;
                counts.entry(mesh.refno).or_default().0 += 1;
            }
            Some(LoadState::Failed(error)) => {
                done += 1;
                counts.entry(mesh.refno).or_default().1 += 1;
                if failed_paths.insert(&mesh.path) {
                    errors.push(format!("{}: {error}", mesh.path));
                }
            }
            _ => {}
        }
    }
    let mut settled_now = Vec::new();
    for (refno, (loaded, failed)) in counts {
        let Some(state) = view.render_states.get_mut(&refno) else {
            continue;
        };
        let was_settled = state.settled();
        // 重算的只是外部文件那一半；内存里的共享虚线带没有加载事件，
        // 直接赋值会把它们抹掉，那个模型就再也等不到终态。
        state.mesh_loaded = state.mesh_immediate + loaded;
        state.mesh_failed = failed;
        if !was_settled && state.settled() {
            settled_now.push(refno);
        }
    }
    view.render_dirty.extend(settled_now);
    let total = view.loading_meshes.len();
    view.mesh_progress = Some(MeshLoadProgress {
        done,
        total,
        errors,
    });
    if done == total {
        // 整批有了结果：失败的那些挑出来留着。换过网格目录之后要重来的就是它们，
        // 跟着 `loading_meshes` 一起清掉就无从下手了。
        let failed: Vec<LoadingMesh> = view
            .loading_meshes
            .drain(..)
            .filter(|mesh| {
                matches!(
                    assets.get_load_state(&mesh.handle),
                    Some(LoadState::Failed(_))
                )
            })
            .collect();
        view.failed_meshes.extend(failed);
    }
}

/// 重发上一轮失败的网格，并推进正在重试的那些。
///
/// 与 [`update_mesh_progress`] 分开走：那一份每帧从 `loading_meshes` 从头重算整批
/// 计数，而重试只补个别网格——混进同一份账里会把同一个模型早先成功的那些数抹掉。
fn retry_meshes(mut view: ResMut<View3d>, assets: Res<AssetServer>) {
    if view.retry_requested {
        view.retry_requested = false;
        let asked_at = Instant::now();
        let retrying: Vec<_> = view
            .failed_meshes
            .drain(..)
            .map(|mesh| RetryingMesh {
                mesh,
                asked_at,
                started: false,
            })
            .collect();
        let mut reloaded = HashSet::new();
        for entry in &retrying {
            // 一个网格文件可以被许多实例共用，重发一次就够。
            if reloaded.insert(entry.mesh.path.clone()) {
                assets.reload(entry.mesh.path.as_str());
            }
        }
        view.retrying_meshes.extend(retrying);
    }
    if view.retrying_meshes.is_empty() {
        return;
    }
    let mut settled_now = Vec::new();
    let mut still_failed = Vec::new();
    let mut in_flight = Vec::new();
    for mut entry in std::mem::take(&mut view.retrying_meshes) {
        match assets.get_load_state(&entry.mesh.handle) {
            Some(LoadState::Loaded) => {
                if let Some(state) = view.render_states.get_mut(&entry.mesh.refno) {
                    state.mesh_loaded += 1;
                    state.mesh_failed = state.mesh_failed.saturating_sub(1);
                }
                settled_now.push(entry.mesh.refno);
            }
            // 还是失败态，但重发是丢进 IO 任务池的，状态要过几帧才翻——在亲眼见到
            // 它离开失败态之前，这个 `Failed` 说的还是上一轮的事，不是这次的结论。
            Some(LoadState::Failed(_)) if entry.settling() => still_failed.push(entry.mesh),
            Some(LoadState::Failed(_)) => in_flight.push(entry),
            _ => {
                entry.started = true;
                in_flight.push(entry);
            }
        }
    }
    view.retrying_meshes = in_flight;
    view.failed_meshes.extend(still_failed);
    view.render_dirty.extend(settled_now);
}

fn apply_resize(
    mut view: ResMut<View3d>,
    mut sync: ResMut<ViewportResizeSync>,
    mut images: ResMut<Assets<Image>>,
    mut egui_textures: ResMut<EguiUserTextures>,
    mut cameras: Query<&mut Camera, With<ViewportRenderCamera>>,
) {
    let rendered_frame = sync.rendered_frames();
    while view
        .retired_resize
        .front()
        .is_some_and(|retired| retired.remove_after_rendered_frame <= rendered_frame)
    {
        let retired = view.retired_resize.pop_front().expect("刚检查过队首");
        egui_textures.remove_image(&retired.image);
        images.remove(&retired.image);
    }

    if let Some(prepared) = view.prepared_resize.take() {
        // 在新目标预热这一帧里，dock 可能又被拖回当前尺寸。此时丢掉预热目标，
        // 相机重新指回仍完整保留的展示纹理，避免下一帧无意义地来回切换。
        if view.pending_resize.map(|(size, _)| size) == Some(view.size) {
            for mut camera in &mut cameras {
                camera.target = RenderTarget::Image(view.image.clone().into());
            }
            view.retired_resize.push_back(RetiredResize {
                image: prepared.image,
                remove_after_rendered_frame: rendered_frame + 1,
            });
            view.pending_resize = None;
            return;
        }

        if !sync.generation_is_rendered(prepared.generation) {
            view.prepared_resize = Some(prepared);
            return;
        }

        // Render world 已在完整 render graph 之后确认过这个代次。现在发布它不会露出
        // 未初始化/透明的一帧；旧目标再留一个渲染帧，等上一份 egui paint job 排空。
        let old_image = std::mem::replace(&mut view.image, prepared.image);
        view.texture = prepared.texture;
        view.size = prepared.size;
        view.retired_resize.push_back(RetiredResize {
            image: old_image,
            remove_after_rendered_frame: rendered_frame + 1,
        });
        if view.pending_resize.map(|(size, _)| size) == Some(view.size) {
            view.pending_resize = None;
        }
    }

    let Some((wanted, asked_at)) = view.pending_resize else {
        return;
    };
    if asked_at.elapsed() < RESIZE_DEBOUNCE {
        return;
    }
    view.pending_resize = None;
    let image = images.add(viewport_image(wanted));
    let texture = egui_textures.add_image(image.clone());
    sync.requested_generation += 1;
    for mut camera in &mut cameras {
        camera.target = RenderTarget::Image(image.clone().into());
    }
    view.prepared_resize = Some(PreparedResize {
        size: wanted,
        image,
        texture,
        generation: sync.requested_generation,
    });
}

#[allow(clippy::too_many_arguments)]
fn apply_commands(
    mut commands: Commands,
    mut view: ResMut<View3d>,
    mut orbit: ResMut<OrbitCamera>,
    mut camera: Query<
        (&Camera, &GlobalTransform, &mut Transform, &mut Projection),
        With<ViewCamera>,
    >,
    mut roots: Query<(Entity, &ModelRoot, &mut Visibility)>,
    meshes: Query<&ModelPickTarget>,
    mut mesh_params: ParamSet<(MeshRayCast, ResMut<Assets<Mesh>>)>,
    background: Res<BackgroundMesh>,
    mut grid: ResMut<GridState>,
) {
    let Ok((camera_component, camera_global, mut camera_transform, mut projection)) =
        camera.single_mut()
    else {
        return;
    };
    while let Some(command) = view.commands.pop_front() {
        match command {
            ViewCommand::Pick(uv) => {
                let hit = surface_hit(
                    uv,
                    camera_component,
                    camera_global,
                    &meshes,
                    &mut mesh_params.p0(),
                );
                // TODO(诊断): 拾取排查完删除。
                diag(format!(
                    "[诊断] 拾取结果 uv={uv:?} hit={:?}",
                    hit.as_ref().map(|(refno, point)| (refno.0, *point))
                ));
                view.picked = hit.map(|(refno, _)| refno);
            }
            ViewCommand::Background {
                top,
                bottom,
                grid: grid_color,
            } => {
                if let Some(mesh) = mesh_params.p1().get_mut(&background.0) {
                    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, gradient_colors(top, bottom));
                }
                if grid.color != grid_color {
                    grid.color = grid_color;
                    grid.rebuild = true;
                }
            }
            ViewCommand::Snap { forward, up, fit } => {
                let mut focus = orbit.focus;
                let mut dist = camera_transform.translation.distance(focus).max(0.1);
                if fit {
                    // Home 键的「拉回全景」：与 FitAll 同一套可见包围盒。
                    let mut min = Vec3::splat(f32::INFINITY);
                    let mut max = Vec3::splat(f32::NEG_INFINITY);
                    for (_, root, visibility) in &roots {
                        if *visibility == Visibility::Hidden {
                            continue;
                        }
                        if let Some(&(model_min, model_max)) = view.bounds.get(&root.refno) {
                            min = min.min(model_min);
                            max = max.max(model_max);
                        }
                    }
                    if min.is_finite() && max.is_finite() {
                        focus = (min + max) * 0.5;
                        dist = (max - min).length().max(0.1) * 0.5 * 2.8;
                    }
                }
                orbit.focus = focus;
                orbit.anim = Some(snap_anim(&camera_transform, focus, forward, up, dist));
            }
            ViewCommand::Model(action) => match action {
                ModelAction::SetXRay { refnos } => {
                    let xray: HashSet<_> = refnos.into_iter().collect();
                    if view.xray != xray {
                        view.xray = xray;
                        view.material_dirty = true;
                    }
                }
                ModelAction::Unload { refnos } => {
                    let targets: HashSet<_> = refnos.into_iter().collect();
                    let mut removed = HashSet::new();
                    let mut entities = Vec::new();
                    for (entity, root, _) in &mut roots {
                        if targets.contains(&root.refno) || targets.contains(&root.owner) {
                            removed.insert(root.refno);
                            entities.push(entity);
                        }
                    }
                    for entity in entities {
                        commands.entity(entity).despawn();
                    }
                    removed.extend(targets);
                    forget_model_state(&mut view, &removed);
                }
                ModelAction::SetVisible { refnos, visible } => {
                    let mut applied = Vec::new();
                    for (_, root, mut visibility) in &mut roots {
                        if refnos.contains(&root.refno) || refnos.contains(&root.owner) {
                            *visibility = if visible {
                                Visibility::Visible
                            } else {
                                Visibility::Hidden
                            };
                            applied.push(root.refno);
                        }
                    }
                    record_applied_visibility(&mut view, applied, visible);
                }
                ModelAction::HideAll => {
                    let mut applied = Vec::new();
                    for (_, root, mut visibility) in &mut roots {
                        *visibility = Visibility::Hidden;
                        applied.push(root.refno);
                    }
                    record_applied_visibility(&mut view, applied, false);
                }
                ModelAction::ShowAll => {
                    let mut applied = Vec::new();
                    for (_, root, mut visibility) in &mut roots {
                        *visibility = Visibility::Visible;
                        applied.push(root.refno);
                    }
                    record_applied_visibility(&mut view, applied, true);
                }
                ModelAction::Focus(refno) => {
                    if let Some(&(min, max)) = view.bounds.get(&refno) {
                        orbit.anim = None;
                        frame_bounds(min, max, &mut orbit, &mut camera_transform, &mut projection);
                    }
                }
                ModelAction::FocusGroup { refnos } => {
                    let mut min = Vec3::splat(f32::INFINITY);
                    let mut max = Vec3::splat(f32::NEG_INFINITY);
                    for refno in &refnos {
                        if let Some(&(group_min, group_max)) = view.bounds.get(refno) {
                            min = min.min(group_min);
                            max = max.max(group_max);
                        }
                    }
                    // 一个都没加载就不动相机：飞到原点比不动更糟。
                    if min.is_finite() && max.is_finite() {
                        orbit.anim = None;
                        frame_bounds(min, max, &mut orbit, &mut camera_transform, &mut projection);
                    }
                }
                ModelAction::Isolate { refnos } => {
                    if view.isolate_restore.is_none() {
                        view.isolate_restore = Some(
                            roots
                                .iter()
                                .map(|(_, root, visibility)| {
                                    (root.refno, *visibility != Visibility::Hidden)
                                })
                                .collect(),
                        );
                    }
                    let mut shown = Vec::new();
                    let mut hidden = Vec::new();
                    for (_, root, mut visibility) in &mut roots {
                        let keep = refnos.contains(&root.refno) || refnos.contains(&root.owner);
                        *visibility = if keep {
                            Visibility::Visible
                        } else {
                            Visibility::Hidden
                        };
                        if keep {
                            shown.push(root.refno);
                        } else {
                            hidden.push(root.refno);
                        }
                    }
                    record_applied_visibility(&mut view, shown, true);
                    record_applied_visibility(&mut view, hidden, false);
                }
                ModelAction::ExitIsolate => {
                    if let Some(snapshot) = view.isolate_restore.take() {
                        let mut shown = Vec::new();
                        let mut hidden = Vec::new();
                        for (_, root, mut visibility) in &mut roots {
                            // 快照之后才加载进来的模型不在账上，按可见处理。
                            let visible = snapshot.get(&root.refno).copied().unwrap_or(true);
                            *visibility = if visible {
                                Visibility::Visible
                            } else {
                                Visibility::Hidden
                            };
                            if visible {
                                shown.push(root.refno);
                            } else {
                                hidden.push(root.refno);
                            }
                        }
                        record_applied_visibility(&mut view, shown, true);
                        record_applied_visibility(&mut view, hidden, false);
                    }
                }
                ModelAction::FitAll => {
                    let mut min = Vec3::splat(f32::INFINITY);
                    let mut max = Vec3::splat(f32::NEG_INFINITY);
                    for (_, root, visibility) in &roots {
                        if *visibility == Visibility::Hidden {
                            continue;
                        }
                        if let Some(&(model_min, model_max)) = view.bounds.get(&root.refno) {
                            min = min.min(model_min);
                            max = max.max(model_max);
                        }
                    }
                    if min.is_finite() && max.is_finite() {
                        orbit.anim = None;
                        frame_bounds(min, max, &mut orbit, &mut camera_transform, &mut projection);
                    }
                }
                ModelAction::ToggleMeasure => {}
            },
            ViewCommand::Camera(gesture) => {
                // 手一动，动画立刻让位；不半路抢方向盘。
                if !matches!(gesture, CameraGesture::End) {
                    orbit.anim = None;
                }
                match gesture {
                    CameraGesture::Begin { motion, anchor } => {
                        orbit.motion = Some(motion);
                        if let Some((_, point)) = surface_hit(
                            anchor,
                            camera_component,
                            camera_global,
                            &meshes,
                            &mut mesh_params.p0(),
                        ) {
                            orbit.focus = point;
                        }
                    }
                    CameraGesture::Drag([x, y]) => match orbit.motion {
                        Some(CameraMotion::Orbit) => {
                            orbit_camera(&mut camera_transform, orbit.focus, x, y);
                        }
                        Some(CameraMotion::Pan) => {
                            let distance = camera_transform.translation.distance(orbit.focus);
                            let delta = (camera_transform.rotation * Vec3::X * -x
                                + camera_transform.rotation * Vec3::Y * y)
                                * distance
                                * 0.0015;
                            camera_transform.translation += delta;
                            orbit.focus += delta;
                        }
                        None => {}
                    },
                    CameraGesture::Zoom { amount, anchor } => {
                        if let Some((_, point)) = surface_hit(
                            anchor,
                            camera_component,
                            camera_global,
                            &meshes,
                            &mut mesh_params.p0(),
                        ) {
                            orbit.focus = point;
                        }
                        zoom_camera(&mut camera_transform, orbit.focus, amount);
                    }
                    CameraGesture::End => orbit.motion = None,
                }
            }
        }
    }
}

fn zoom_camera(camera: &mut Transform, focus: Vec3, amount: f32) {
    let offset = camera.translation - focus;
    camera.translation = focus + offset * (amount * -0.002).exp().clamp(0.2, 5.0);
}

fn orbit_camera(camera: &mut Transform, focus: Vec3, x: f32, y: f32) {
    // 视图采用“拖动场景”的 CAD 手感：右键向右拖时，相机沿 -X 绕焦点运动。
    let yaw = Quat::from_rotation_y(-x * 0.005);
    let right = camera.rotation * Vec3::X;
    let pitch = Quat::from_axis_angle(right, -y * 0.005);
    camera.rotate_around(focus, yaw * pitch);
}

fn apply_selection(
    mut view: ResMut<View3d>,
    xray: Res<XRayMaterial>,
    invalid_line_state: Res<InvalidTubiLineStateMaterials>,
    mut meshes: Query<(&ModelMesh, &mut MeshMaterial3d<StandardMaterial>)>,
    mut invalid_lines: Query<(
        &InvalidTubiLine,
        &mut MeshMaterial3d<InvalidTubiLineMaterial>,
    )>,
) {
    if !view.selection_dirty && !view.material_dirty {
        return;
    }
    view.selection_dirty = false;
    view.material_dirty = false;
    for (mesh, mut material) in &mut meshes {
        material.0 = material_state(&view.selected, &view.xray, mesh.refno, mesh.owner).choose(
            &mesh.base,
            &mesh.highlight,
            &xray.0,
        );
    }
    for (line, mut material) in &mut invalid_lines {
        material.0 = material_state(&view.selected, &view.xray, line.refno, line.owner).choose(
            &line.base,
            &line.highlight,
            &invalid_line_state.xray,
        );
    }
}

/// 从当前相机出发、绕住焦点、终点朝向 `forward` 的一次插值动画。
fn snap_anim(camera: &Transform, focus: Vec3, forward: Vec3, up: Vec3, dist: f32) -> SnapAnim {
    let to_pos = focus - forward * dist;
    let to_rot = Transform::from_translation(to_pos)
        .looking_at(focus, up)
        .rotation;
    SnapAnim {
        from_pos: camera.translation,
        from_rot: camera.rotation,
        to_pos,
        to_rot,
        t: 0.0,
    }
}

/// 推进 Snap 动画：位置 lerp、姿态 slerp，smoothstep 缓动，SNAP_SECS 内到位。
fn animate_snap(
    time: Res<Time>,
    mut orbit: ResMut<OrbitCamera>,
    mut camera: Query<&mut Transform, With<ViewCamera>>,
) {
    let Some(anim) = orbit.anim.as_mut() else {
        return;
    };
    let Ok(mut transform) = camera.single_mut() else {
        return;
    };
    anim.t = (anim.t + time.delta_secs() / SNAP_SECS).min(1.0);
    let s = anim.t * anim.t * (3.0 - 2.0 * anim.t);
    transform.translation = anim.from_pos.lerp(anim.to_pos, s);
    transform.rotation = anim.from_rot.slerp(anim.to_rot, s);
    if anim.t >= 1.0 {
        orbit.anim = None;
    }
}

/// 相机拉远拉近时让网格换档（10 的整幂），三轴跟着格距伸缩。
/// 换主题时（rebuild 脏标记）同级也重建一次，把新线色写进顶点。
fn update_grid(
    camera: Query<&Transform, With<ViewCamera>>,
    orbit: Res<OrbitCamera>,
    mut grid: ResMut<GridState>,
    grid_mesh: Res<GridMesh>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut axes: Query<&mut Transform, (With<AxesRoot>, Without<ViewCamera>)>,
) {
    let Ok(camera) = camera.single() else {
        return;
    };
    let level = grid_level(camera.translation.distance(orbit.focus));
    if level == grid.level && !grid.rebuild {
        return;
    }
    grid.level = level;
    grid.rebuild = false;
    if let Some(mesh) = meshes.get_mut(&grid_mesh.0) {
        *mesh = build_grid_mesh(level, grid.color);
    }
    for mut transform in &mut axes {
        transform.scale = Vec3::splat(level);
    }
}

fn extend_bounds(bounds: &mut HashMap<RefU64, (Vec3, Vec3)>, refno: RefU64, min: Vec3, max: Vec3) {
    bounds
        .entry(refno)
        .and_modify(|bounds| {
            bounds.0 = bounds.0.min(min);
            bounds.1 = bounds.1.max(max);
        })
        .or_insert((min, max));
}

fn frame_bounds(
    min: Vec3,
    max: Vec3,
    orbit: &mut OrbitCamera,
    camera: &mut Transform,
    projection: &mut Projection,
) {
    orbit.focus = (min + max) * 0.5;
    let radius = (max - min).length().max(0.1) * 0.5;
    if let Projection::Perspective(projection) = projection {
        projection.far = projection.far.max(radius * 4.0);
    }
    let direction = (camera.translation - orbit.focus).normalize_or(Vec3::new(0.6, 0.4, 0.7));
    camera.translation = orbit.focus + direction * radius * 2.8;
    camera.look_at(orbit.focus, Vec3::Y);
}

fn surface_hit(
    uv: [f32; 2],
    camera: &Camera,
    camera_transform: &GlobalTransform,
    meshes: &Query<&ModelPickTarget>,
    ray_cast: &mut MeshRayCast,
) -> Option<(RefU64, Vec3)> {
    // TODO(诊断): 拾取排查完把日志删掉、恢复 `?` 链。
    let Some(size) = camera.logical_viewport_size() else {
        diag("[诊断] 拾取失败：logical_viewport_size = None".into());
        return None;
    };
    let ray = match camera
        .viewport_to_world(camera_transform, Vec2::new(uv[0] * size.x, uv[1] * size.y))
    {
        Ok(ray) => ray,
        Err(error) => {
            diag(format!(
                "[诊断] 拾取失败：viewport_to_world {error:?}（size={size:?} uv={uv:?}）"
            ));
            return None;
        }
    };
    let is_model = |entity| meshes.contains(entity);
    let settings = MeshRayCastSettings::default()
        .with_visibility(RayCastVisibility::Visible)
        .with_filter(&is_model);
    let hits = ray_cast.cast_ray(ray, &settings);
    diag(format!(
        "[诊断] 射线 origin={:?} dir={:?} size={size:?} 模型网格实体数={} 命中数={}",
        ray.origin,
        ray.direction,
        meshes.iter().count(),
        hits.len()
    ));
    let (entity, hit) = hits.first()?;
    Some((meshes.get(*entity).ok()?.refno, hit.point))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::asset::AssetPlugin;
    use bevy::render::mesh::VertexAttributeValues;

    #[test]
    fn invalid_tubi_line_starts_at_the_connection_origin() {
        let mesh = invalid_tubi_line_mesh();

        assert_eq!(mesh.primitive_topology(), PrimitiveTopology::TriangleList);
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("invalid TUBI line mesh must contain Float32x3 positions");
        };
        assert_eq!(positions.len(), 4);
        assert_eq!(positions[0][2], 0.0);
        assert_eq!(positions[1][2], 0.0);
        assert_eq!(positions[2][2], 1.0);
        assert_eq!(positions[3][2], 1.0);
        assert!(
            positions
                .iter()
                .all(|position| position[0] == 0.0 && position[1] == 0.0)
        );
        let Some(VertexAttributeValues::Float32(sides)) = mesh.attribute(INVALID_TUBI_LINE_SIDE)
        else {
            panic!("invalid TUBI line mesh must contain Float32 side values");
        };
        assert_eq!(sides, &vec![-1.0, 1.0, -1.0, 1.0]);
        match mesh.indices().expect("unit ribbon must be indexed") {
            Indices::U32(indices) => assert_eq!(indices, &vec![0, 1, 2, 2, 1, 3]),
            Indices::U16(_) => panic!("unit ribbon indices must stay U32"),
        }
    }

    #[test]
    fn invalid_tubi_shader_uses_bevys_material_bind_group() {
        let shader = include_str!("invalid_tubi_line.wgsl");
        assert_eq!(shader.matches("@group(#{MATERIAL_BIND_GROUP})").count(), 2);
        assert!(
            !shader.contains("@group(2)"),
            "group 2 belongs to Bevy's mesh bindings; material uniforms must use the injected group"
        );
    }

    #[test]
    fn invalid_tubi_fallback_line_is_dashed_from_connection_to_endpoint() {
        let mesh = invalid_tubi_fallback_line_mesh(1_000.0);
        assert_eq!(mesh.primitive_topology(), PrimitiveTopology::LineList);
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("fallback line must contain Float32x3 positions");
        };
        assert!(positions.len() > 2);
        assert_eq!(positions.first().unwrap(), &[0.0, 0.0, 0.0]);
        assert!((positions.last().unwrap()[2] - 1.0).abs() < 1.0e-6);
        for pair in positions.chunks_exact(2).collect::<Vec<_>>().windows(2) {
            assert!(pair[1][0][2] > pair[0][1][2], "dashes need a visible gap");
        }
    }

    #[test]
    fn long_and_short_invalid_tubi_share_the_same_unit_ribbon() {
        let mut meshes = Assets::<Mesh>::default();
        let shared = InvalidTubiLineMesh(meshes.add(invalid_tubi_line_mesh()));
        let long_segment = shared.handle();
        let short_segment = shared.handle();

        assert_eq!(long_segment, short_segment);
        assert_eq!(
            meshes.len(),
            1,
            "segment length must not create another mesh"
        );
    }

    #[test]
    fn invalid_tubi_material_state_is_xray_then_selected_then_base_for_refno_or_owner() {
        let refno = RefU64::from(42);
        let owner = RefU64::from(7);
        let mut view = test_view();

        assert_eq!(
            material_state(&view.selected, &view.xray, refno, owner),
            MaterialState::Base
        );
        view.selected.insert(owner);
        assert_eq!(
            material_state(&view.selected, &view.xray, refno, owner),
            MaterialState::Selected
        );
        view.xray.insert(refno);
        assert_eq!(
            material_state(&view.selected, &view.xray, refno, owner),
            MaterialState::XRay
        );
        view.xray.clear();
        view.selected.clear();
        view.selected.insert(refno);
        assert_eq!(
            material_state(&view.selected, &view.xray, refno, owner),
            MaterialState::Selected
        );
        view.xray.insert(owner);
        assert_eq!(
            material_state(&view.selected, &view.xray, refno, owner),
            MaterialState::XRay
        );
    }

    #[test]
    fn horizontal_orbit_matches_cad_drag_direction() {
        let focus = Vec3::ZERO;
        let mut camera = Transform::from_xyz(0.0, 0.0, 10.0);

        orbit_camera(&mut camera, focus, 20.0, 0.0);

        assert!(
            camera.translation.x < 0.0,
            "dragging right should orbit the camera toward -X, got {:?}",
            camera.translation
        );
    }

    fn test_view() -> View3d {
        View3d {
            texture: egui::TextureId::Managed(0),
            size: UVec2::new(16, 16),
            camera_rot: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            axis_labels: [None; 3],
            grid_cell_mm: INITIAL_GRID_LEVEL * MM_PER_WORLD,
            image: Handle::default(),
            commands: VecDeque::new(),
            pending_models: VecDeque::new(),
            loading_meshes: Vec::new(),
            failed_meshes: Vec::new(),
            retrying_meshes: Vec::new(),
            retry_requested: false,
            mesh_progress: None,
            desired_visibility: HashMap::new(),
            render_states: HashMap::new(),
            render_dirty: HashSet::new(),
            pending_resize: None,
            prepared_resize: None,
            retired_resize: VecDeque::new(),
            picked: None,
            selected: HashSet::new(),
            selection_dirty: false,
            xray: HashSet::new(),
            material_dirty: false,
            bounds: HashMap::new(),
            isolate_restore: None,
        }
    }

    /// egui 正在采样的离屏纹理不能原地改尺寸：`Image::resize` 会让 wgpu 重建 GPU
    /// 纹理并丢掉旧内容，而窗口相机与离屏相机没有显式的渲染图依赖，窗口可能先把
    /// 这张尚未重画的空纹理采样出来，表现就是 dock 拖动时闪一下底色。
    #[test]
    fn resize_keeps_the_displayed_texture_alive_until_replacement_is_rendered() {
        let old_size = UVec2::new(320, 180);
        let wanted = UVec2::new(640, 360);
        let mut app = App::new();
        app.init_resource::<Assets<Image>>()
            .init_resource::<EguiUserTextures>()
            .init_resource::<ViewportResizeSync>();

        let old_image = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(viewport_image(old_size));
        let old_texture = app
            .world_mut()
            .resource_mut::<EguiUserTextures>()
            .add_image(old_image.clone());
        let mut view = test_view();
        view.image = old_image.clone();
        view.texture = old_texture;
        view.size = old_size;
        view.pending_resize = Some((wanted, Instant::now() - RESIZE_DEBOUNCE));
        app.insert_resource(view).add_systems(Update, apply_resize);
        for _ in 0..2 {
            app.world_mut().spawn((
                Camera {
                    target: RenderTarget::Image(old_image.clone().into()),
                    ..default()
                },
                ViewportRenderCamera,
            ));
        }

        app.update();

        let view = app.world().resource::<View3d>();
        assert_eq!(view.image, old_image, "展示中的纹理句柄被提前换掉了");
        assert_eq!(view.texture, old_texture, "egui 提前采样了新纹理");
        assert_eq!(view.size, old_size, "展示尺寸在替代纹理就绪前就发布了");
        assert_eq!(
            app.world()
                .resource::<Assets<Image>>()
                .get(&old_image)
                .expect("展示纹理仍应存在")
                .size(),
            old_size,
            "展示中的纹理被原地 resize，旧画面已经丢失"
        );
        let (prepared_image, prepared_texture) = {
            let prepared = view
                .prepared_resize
                .as_ref()
                .expect("新目标应该已经开始预热");
            assert_eq!(prepared.size, wanted);
            (prepared.image.clone(), prepared.texture)
        };
        let camera_targets = {
            let world = app.world_mut();
            let mut cameras = world.query_filtered::<&Camera, With<ViewportRenderCamera>>();
            cameras
                .iter(world)
                .map(|camera| match &camera.target {
                    RenderTarget::Image(target) => target.handle.clone(),
                    other => panic!("离屏相机被改到了非 Image 目标：{other:?}"),
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(camera_targets, vec![prepared_image.clone(); 2]);

        // 只过 Update 没有任何资格冒充“已经渲染”：显式回执到达前一直展示旧目标。
        app.update();
        assert_eq!(app.world().resource::<View3d>().image, old_image);
        app.world()
            .resource::<ViewportResizeSync>()
            .acknowledge_rendered();

        // 收到 render_system 后的代次回执才切换；旧目标仍多活一个渲染帧，避免上一份
        // egui paint job 或 render world 还引用它时 TextureId 被复用。
        app.update();
        let view = app.world().resource::<View3d>();
        assert_eq!(view.image, prepared_image);
        assert_eq!(view.texture, prepared_texture);
        assert_eq!(view.size, wanted);
        assert!(
            app.world()
                .resource::<Assets<Image>>()
                .get(&old_image)
                .is_some(),
            "刚发布新目标就回收了旧目标"
        );

        app.world()
            .resource::<ViewportResizeSync>()
            .acknowledge_rendered();
        app.update();
        assert!(
            app.world()
                .resource::<Assets<Image>>()
                .get(&old_image)
                .is_none(),
            "下一帧已经排空，旧目标仍未回收"
        );
    }

    #[test]
    fn resize_reversal_retargets_cameras_before_retiring_the_prepared_image() {
        let old_size = UVec2::new(320, 180);
        let mut app = App::new();
        app.init_resource::<Assets<Image>>()
            .init_resource::<EguiUserTextures>()
            .init_resource::<ViewportResizeSync>();
        let old_image = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(viewport_image(old_size));
        let old_texture = app
            .world_mut()
            .resource_mut::<EguiUserTextures>()
            .add_image(old_image.clone());
        let mut view = test_view();
        view.image = old_image.clone();
        view.texture = old_texture;
        view.size = old_size;
        view.pending_resize = Some((UVec2::new(640, 360), Instant::now() - RESIZE_DEBOUNCE));
        app.insert_resource(view).add_systems(Update, apply_resize);
        let camera = app
            .world_mut()
            .spawn((
                Camera {
                    target: RenderTarget::Image(old_image.clone().into()),
                    ..default()
                },
                ViewportRenderCamera,
            ))
            .id();

        app.update();
        let prepared_image = app
            .world()
            .resource::<View3d>()
            .prepared_resize
            .as_ref()
            .expect("应该已经开始预热")
            .image
            .clone();
        app.world_mut()
            .resource_mut::<View3d>()
            .resize(old_size.to_array());
        app.update();

        let view = app.world().resource::<View3d>();
        assert_eq!(view.image, old_image);
        assert!(view.prepared_resize.is_none());
        let camera = app.world().entity(camera).get::<Camera>().expect("相机");
        assert!(
            matches!(&camera.target, RenderTarget::Image(target) if target.handle == old_image),
            "取消预热后相机没有回到当前展示目标"
        );
        assert!(
            app.world()
                .resource::<Assets<Image>>()
                .get(&prepared_image)
                .is_some(),
            "取消当帧就删除了 render world 仍可能引用的预热目标"
        );

        app.world()
            .resource::<ViewportResizeSync>()
            .acknowledge_rendered();
        app.update();
        assert!(
            app.world()
                .resource::<Assets<Image>>()
                .get(&prepared_image)
                .is_none(),
            "相机回切并完成一帧后，取消的预热目标仍未回收"
        );
    }

    /// 档位表：1/2/5×10ⁿ 毫米，夹在格距上下限之内。
    fn ladder_mm() -> Vec<f32> {
        (0..=6)
            .flat_map(|exp| {
                let decade = 10f32.powi(exp);
                [1.0, 2.0, 5.0].map(move |step| step * decade)
            })
            .filter(|mm| (GRID_MIN_MM..=GRID_MAX_MM).contains(mm))
            .collect()
    }

    #[test]
    fn every_grid_cell_is_a_round_real_length() {
        let ladder = ladder_mm();
        for step in 0..=400 {
            let dist = 10f32.powf(-2.0 + step as f32 * 0.03);
            let mm = grid_cell_mm(dist);
            assert!(
                ladder.iter().any(|rung| (rung - mm).abs() <= rung * 1e-4),
                "相机距离 {dist} 处格距 {mm} mm 不在 1/2/5×10ⁿ 档上"
            );
        }
    }

    #[test]
    fn the_grid_stays_between_a_millimetre_and_a_kilometre() {
        // 相机贴到构件表面，以及 `dist` 退化成 0：都落到下界，不会算出 0 或负数
        // 格距——那会让 `build_grid_mesh` 画出一堆重合线，也会把三轴缩成一点。
        assert_eq!(grid_cell_mm(0.0), GRID_MIN_MM);
        assert_eq!(grid_cell_mm(1e-6), GRID_MIN_MM);
        assert_eq!(grid_cell_mm(f32::MAX), GRID_MAX_MM);
    }

    #[test]
    fn pulling_the_camera_back_never_shrinks_the_grid() {
        let mut previous = 0.0;
        for step in 0..=300 {
            let dist = 10f32.powf(-1.0 + step as f32 * 0.04);
            let mm = grid_cell_mm(dist);
            assert!(mm >= previous, "距离 {dist} 处格距回缩：{previous} -> {mm}");
            previous = mm;
        }
    }

    /// 世界单位与毫米之间只有 `MODEL_SCALE` 一处换算。谁把网格或三轴那一边
    /// 改成另写的常数，这条就红——两个尺度各自漂移是本次要根治的病。
    #[test]
    fn world_units_and_millimetres_meet_at_one_conversion() {
        assert_eq!(MM_PER_WORLD * MODEL_SCALE, 1.0);
        for dist in [0.5f32, 5.0, 50.0, 500.0, 5_000.0] {
            assert_eq!(grid_level(dist), grid_cell_mm(dist) * MODEL_SCALE);
        }
    }

    /// 换到 1/2/5 阶梯不能把密度带跑偏。落档最多偏到相邻两档的几何中点，
    /// 视野里的格数因此稳定在十来格——这是「格距跟着相机走」的全部意义。
    #[test]
    fn the_cell_keeps_about_a_dozen_cells_across_the_view() {
        for step in 0..=285 {
            let dist = 10f32.powf(-0.7 + step as f32 * 0.02);
            let cells = dist / grid_level(dist);
            assert!(
                (GRID_CELLS_ACROSS / 2.0..=GRID_CELLS_ACROSS * 2.0).contains(&cells),
                "距离 {dist} 处视野里 {cells} 格，离 {GRID_CELLS_ACROSS} 太远"
            );
        }
    }

    #[test]
    fn framing_centers_and_keeps_the_camera_outside_the_model() {
        let mut orbit = OrbitCamera::default();
        let mut camera = Transform::from_xyz(4.0, 3.0, 5.0);
        let mut projection = Projection::Perspective(PerspectiveProjection::default());
        frame_bounds(
            Vec3::new(-2_000.0, -1_000.0, -3_000.0),
            Vec3::new(2_000.0, 1_000.0, 3_000.0),
            &mut orbit,
            &mut camera,
            &mut projection,
        );
        assert_eq!(orbit.focus, Vec3::ZERO);
        let radius = Vec3::new(4_000.0, 2_000.0, 6_000.0).length() * 0.5;
        assert!(camera.translation.length() > radius);
        let Projection::Perspective(projection) = projection else {
            unreachable!()
        };
        assert!(projection.far >= radius * 4.0);
        assert!(
            camera
                .forward()
                .dot((orbit.focus - camera.translation).normalize())
                > 0.999
        );
    }

    #[test]
    fn owner_bounds_cover_all_child_models() {
        let owner = RefU64::from(42);
        let mut bounds = HashMap::new();
        extend_bounds(
            &mut bounds,
            owner,
            Vec3::new(1.0, 2.0, 3.0),
            Vec3::new(4.0, 5.0, 6.0),
        );
        extend_bounds(
            &mut bounds,
            owner,
            Vec3::new(-2.0, 3.0, 2.0),
            Vec3::new(3.0, 8.0, 9.0),
        );
        assert_eq!(
            bounds[&owner],
            (Vec3::new(-2.0, 2.0, 2.0), Vec3::new(4.0, 8.0, 9.0))
        );
    }

    #[test]
    fn incremental_batches_keep_the_existing_scene_and_last_visibility_wins() {
        assert!(ModelBatch::Replace(Vec::new()).clears_scene());
        assert!(!ModelBatch::Append(Vec::new()).clears_scene());

        let target = RefU64::from(42);
        let mut desired = HashMap::new();
        record_model_visibility(
            &ModelAction::SetVisible {
                refnos: vec![target],
                visible: true,
            },
            &mut desired,
        );
        record_model_visibility(
            &ModelAction::SetVisible {
                refnos: vec![target],
                visible: false,
            },
            &mut desired,
        );
        assert_eq!(desired.get(&target), Some(&false));
    }

    #[test]
    fn unloading_a_model_forgets_its_render_and_interaction_state() {
        let removed = RefU64::from(42);
        let kept = RefU64::from(43);
        let mut view = test_view();
        for refno in [removed, kept] {
            view.desired_visibility.insert(refno, true);
            view.render_states.insert(
                refno,
                RenderState {
                    visible: true,
                    mesh_total: 1,
                    mesh_immediate: 0,
                    mesh_loaded: 1,
                    mesh_failed: 0,
                },
            );
            view.render_dirty.insert(refno);
            view.bounds.insert(refno, (Vec3::ZERO, Vec3::ONE));
            view.selected.insert(refno);
            view.xray.insert(refno);
        }
        view.isolate_restore = Some(HashMap::from([(removed, true), (kept, false)]));

        record_model_visibility(
            &ModelAction::Unload {
                refnos: vec![removed],
            },
            &mut view.desired_visibility,
        );
        forget_model_state(&mut view, &HashSet::from([removed]));

        assert!(!view.desired_visibility.contains_key(&removed));
        assert!(!view.render_states.contains_key(&removed));
        assert!(!view.render_dirty.contains(&removed));
        assert!(!view.bounds.contains_key(&removed));
        assert!(!view.selected.contains(&removed));
        assert!(!view.xray.contains(&removed));
        assert!(
            !view
                .isolate_restore
                .as_ref()
                .unwrap()
                .contains_key(&removed)
        );
        assert!(view.render_states.contains_key(&kept));
        assert!(view.selected.contains(&kept));
        assert!(view.selection_dirty);
        assert!(view.material_dirty);
    }

    #[test]
    fn only_a_batch_into_an_empty_scene_frames_the_camera() {
        assert!(should_frame_batch(true));
        // 整场替换不取景：取回工作清场重装要回到取回前的样子，相机也算在内。
        assert!(!should_frame_batch(false));
    }

    #[test]
    fn incremental_load_system_keeps_the_existing_scene_root() {
        let mut view = test_view();
        view.pending_models
            .push_back(ModelBatch::Append(Vec::new()));
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Mesh>()
            .insert_resource(view)
            .insert_resource(OrbitCamera::default())
            .insert_resource(HighlightMaterial(Handle::default()))
            .insert_resource(LineHighlightMaterial(Handle::default()))
            .insert_resource(XRayMaterial(Handle::default()))
            .insert_resource(InvalidTubiLineMesh(Handle::default()))
            .insert_resource(test_invalid_line_state())
            .insert_resource(Assets::<StandardMaterial>::default())
            .insert_resource(Assets::<InvalidTubiLineMaterial>::default())
            .add_systems(Update, load_models);
        app.world_mut().spawn((
            Transform::default(),
            Projection::Perspective(PerspectiveProjection::default()),
            ViewCamera,
        ));
        let existing = app
            .world_mut()
            .spawn((Transform::default(), Visibility::Visible, SceneRoot))
            .id();

        app.update();

        assert!(app.world().entity(existing).contains::<SceneRoot>());
    }

    #[test]
    fn terminal_mesh_progress_is_delivered_once() {
        let mut view = test_view();
        view.mesh_progress = Some(MeshLoadProgress {
            done: 2,
            total: 2,
            errors: vec!["meshes/missing.mesh: not found".into()],
        });

        assert_eq!(view.take_mesh_progress().unwrap().errors.len(), 1);
        assert!(view.take_mesh_progress().is_none());
    }

    #[test]
    fn an_empty_replacement_reports_terminal_progress_after_scene_consumption() {
        let progress = begin_mesh_progress(0);
        assert!(progress.finished());
        assert!(progress.errors.is_empty());
    }

    #[test]
    fn missing_mesh_file_finishes_with_a_visible_error() {
        let path = "meshes/__plant_ui_missing_test__.mesh";
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Mesh>()
            .init_asset_loader::<MeshLoader>()
            .insert_resource(test_view())
            .add_systems(Update, update_mesh_progress);
        let handle = app.world().resource::<AssetServer>().load(path);
        app.world_mut()
            .resource_mut::<View3d>()
            .loading_meshes
            .push(LoadingMesh {
                refno: RefU64::from(1),
                path: path.into(),
                handle,
            });

        for _ in 0..100 {
            app.update();
            if app
                .world()
                .resource::<View3d>()
                .mesh_progress
                .as_ref()
                .is_some_and(MeshLoadProgress::finished)
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        let view = app.world().resource::<View3d>();
        let progress = view.mesh_progress.as_ref().unwrap();
        assert_eq!((progress.done, progress.total), (1, 1));
        assert_eq!(progress.errors.len(), 1);
        assert!(progress.errors[0].contains(path));
        // 装完的那一批清掉了，但失败的这个要留着——换过网格目录之后要重来的就是它。
        assert!(view.loading_meshes.is_empty());
        assert_eq!(view.failed_mesh_count(), 1);
    }

    /// 同一个模型同时挂着内存虚线带和外部网格文件。外部那一半是每帧从头重算的，
    /// 重算时要把当场绑定的虚线带加回去——否则这个模型的账永远凑不齐，终态不来。
    #[test]
    fn an_invalid_tubi_line_mesh_survives_the_external_mesh_recount() {
        let refno = RefU64::from(11);
        let path = "meshes/__plant_ui_missing_mixed_test__.mesh";
        let mut view = test_view();
        view.render_states.insert(
            refno,
            RenderState {
                visible: true,
                mesh_total: 2,
                mesh_immediate: 1,
                mesh_loaded: 1,
                mesh_failed: 0,
            },
        );
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Mesh>()
            .init_asset_loader::<MeshLoader>()
            .insert_resource(view)
            .add_systems(Update, update_mesh_progress);
        let handle = app.world().resource::<AssetServer>().load(path);
        app.world_mut()
            .resource_mut::<View3d>()
            .loading_meshes
            .push(LoadingMesh {
                refno,
                path: path.into(),
                handle,
            });

        for _ in 0..100 {
            app.update();
            if app
                .world()
                .resource::<View3d>()
                .mesh_progress
                .as_ref()
                .is_some_and(MeshLoadProgress::finished)
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        let mut view = app.world_mut().resource_mut::<View3d>();
        let reported = view.take_render_states();
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].refno, refno);
        assert_eq!((reported[0].mesh_loaded, reported[0].mesh_failed), (1, 1));
    }

    /// 重发是丢进 IO 任务池的，状态要过几帧才翻。在亲眼见到它离开失败态之前，
    /// 那一刻的 `Failed` 说的还是上一轮的事——照单全收的话重试会当场自我否定。
    #[test]
    fn a_stale_failure_does_not_settle_a_fresh_retry() {
        let mut entry = RetryingMesh {
            mesh: LoadingMesh {
                refno: RefU64::from(1),
                path: "meshes://a.mesh".into(),
                handle: Handle::default(),
            },
            asked_at: Instant::now(),
            started: false,
        };
        assert!(!entry.settling());
        entry.started = true;
        assert!(entry.settling());

        // 兜底：压根没被重发的那些不能永远挂着，宽限期一过就认账。
        let stale = RetryingMesh {
            asked_at: Instant::now() - RETRY_GRACE - Duration::from_millis(1),
            started: false,
            ..entry
        };
        assert!(stale.settling());
    }

    #[test]
    fn retrying_needs_something_to_retry() {
        let mut view = test_view();
        assert_eq!(view.retry_failed_meshes(), 0);
        assert!(!view.retry_requested);

        view.failed_meshes.push(LoadingMesh {
            refno: RefU64::from(1),
            path: "meshes://a.mesh".into(),
            handle: Handle::default(),
        });
        assert_eq!(view.retry_failed_meshes(), 1);
        assert!(view.retry_requested);
    }

    /// 网格还在装的模型不回执：eye 不许抢在画面前面变成「已显示」。
    #[test]
    fn a_model_reports_its_state_only_after_its_meshes_settle() {
        let refno = RefU64::from(42);
        let mut view = test_view();
        view.render_states.insert(
            refno,
            RenderState {
                visible: true,
                mesh_total: 2,
                mesh_immediate: 0,
                mesh_loaded: 0,
                mesh_failed: 0,
            },
        );
        view.render_dirty.insert(refno);

        assert!(view.take_render_states().is_empty());

        let state = view.render_states.get_mut(&refno).unwrap();
        state.mesh_loaded = 1;
        state.mesh_failed = 1;

        let reported = view.take_render_states();
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].refno, refno);
        assert_eq!((reported[0].mesh_loaded, reported[0].mesh_failed), (1, 1));
        // 终态只交付一次，之后没有新变化就不该再有回执。
        assert!(view.take_render_states().is_empty());
    }

    /// 零网格的模型当场就是终态——它没有可等的东西，回执不该被无限期扣着。
    #[test]
    fn a_model_without_meshes_settles_immediately() {
        let refno = RefU64::from(7);
        let mut view = test_view();
        view.render_states.insert(
            refno,
            RenderState {
                visible: true,
                mesh_total: 0,
                mesh_immediate: 0,
                mesh_loaded: 0,
                mesh_failed: 0,
            },
        );
        view.render_dirty.insert(refno);

        let reported = view.take_render_states();
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].mesh_total, 0);
    }

    /// 装完的模型改可见性，命令一落到实体上就回执；装载途中的只记账不回执。
    #[test]
    fn applying_visibility_reports_settled_models_and_defers_loading_ones() {
        let [settled, loading] = [RefU64::from(1), RefU64::from(2)];
        let mut view = test_view();
        view.render_states.insert(
            settled,
            RenderState {
                visible: true,
                mesh_total: 1,
                mesh_immediate: 0,
                mesh_loaded: 1,
                mesh_failed: 0,
            },
        );
        view.render_states.insert(
            loading,
            RenderState {
                visible: true,
                mesh_total: 1,
                mesh_immediate: 0,
                mesh_loaded: 0,
                mesh_failed: 0,
            },
        );

        record_applied_visibility(&mut view, vec![settled, loading], false);

        let reported = view.take_render_states();
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].refno, settled);
        assert!(!reported[0].visible);
        // 装载途中的那一个方向已经记下，等它装完再一并交付。
        assert!(!view.render_states[&loading].visible);
        let state = view.render_states.get_mut(&loading).unwrap();
        state.mesh_loaded = 1;
        view.render_dirty.insert(loading);
        let reported = view.take_render_states();
        assert_eq!(reported.len(), 1);
        assert!(!reported[0].visible);
    }

    /// 装载途中反向点击：账上留的是最后一次方向，装完按它回执。
    #[test]
    fn the_last_instruction_wins_while_a_model_is_still_loading() {
        let refno = RefU64::from(9);
        let mut view = test_view();
        view.render_states.insert(
            refno,
            RenderState {
                visible: true,
                mesh_total: 1,
                mesh_immediate: 0,
                mesh_loaded: 0,
                mesh_failed: 0,
            },
        );
        view.render_dirty.insert(refno);

        record_applied_visibility(&mut view, vec![refno], false);
        record_applied_visibility(&mut view, vec![refno], true);
        assert!(view.take_render_states().is_empty());

        view.render_states.get_mut(&refno).unwrap().mesh_loaded = 1;
        let reported = view.take_render_states();
        assert_eq!(reported.len(), 1);
        assert!(reported[0].visible);
    }

    #[test]
    fn repeated_resize_request_keeps_the_original_debounce_deadline() {
        let mut view = test_view();
        view.size = UVec2::new(1200, 700);
        let first_asked_at = Instant::now() - Duration::from_secs(1);
        view.pending_resize = Some((UVec2::new(1000, 600), first_asked_at));

        view.resize([1000, 600]);

        assert_eq!(
            view.pending_resize,
            Some((UVec2::new(1000, 600), first_asked_at)),
            "逐帧重复请求同一尺寸不应把防抖计时归零"
        );
    }

    fn test_invalid_line_state() -> InvalidTubiLineStateMaterials {
        InvalidTubiLineStateMaterials {
            highlight: Handle::default(),
            xray: Handle::default(),
        }
    }

    #[test]
    fn selection_highlight_replaces_and_restores_the_type_material() {
        let refno = RefU64::from(42);
        let owner = RefU64::from(7);
        let mut materials = Assets::<StandardMaterial>::default();
        let base = materials.add(model_material(type_color("PIPE")));
        let highlight = materials.add(model_material(SELECT_COLOR));
        assert_eq!(type_color("CE").to_srgba().to_u8_array()[3], 200);
        let mut app = App::new();
        let mut view = test_view();
        view.selected.insert(owner);
        view.selection_dirty = true;
        app.add_plugins(MinimalPlugins)
            .insert_resource(XRayMaterial(Handle::default()))
            .insert_resource(test_invalid_line_state())
            .insert_resource(view)
            .add_systems(Update, apply_selection);
        let entity = app
            .world_mut()
            .spawn((
                ModelMesh {
                    refno,
                    owner,
                    base: base.clone(),
                    highlight: highlight.clone(),
                },
                MeshMaterial3d(base.clone()),
            ))
            .id();

        app.update();
        assert_eq!(
            app.world()
                .entity(entity)
                .get::<MeshMaterial3d<StandardMaterial>>()
                .unwrap()
                .0,
            highlight
        );

        {
            let mut view = app.world_mut().resource_mut::<View3d>();
            view.selected.clear();
            view.selection_dirty = true;
        }
        app.update();
        assert_eq!(
            app.world()
                .entity(entity)
                .get::<MeshMaterial3d<StandardMaterial>>()
                .unwrap()
                .0,
            base
        );
    }

    /// 无效 TUBI 的屏幕带选中再取消，必须回到自己的类型色与 1.5px 常态宽度。
    #[test]
    fn an_invalid_tubi_line_keeps_its_own_material_across_selection() {
        let refno = RefU64::from(42);
        let owner = RefU64::from(7);
        let color = type_color("TUBI");
        let base_value = InvalidTubiLineMaterial::new(color, INVALID_TUBI_LINE_WIDTH_PX);
        let highlight_value =
            InvalidTubiLineMaterial::new(SELECT_COLOR, INVALID_TUBI_SELECTED_WIDTH_PX);
        assert_eq!(base_value.params, Vec4::new(1.5, 5.0, 4.0, 0.75));
        assert_eq!(highlight_value.params.x, 2.0);

        let mut materials = Assets::<InvalidTubiLineMaterial>::default();
        let line_base = materials.add(base_value);
        let line_highlight = materials.add(highlight_value);
        let line_xray = materials.add(InvalidTubiLineMaterial::new(
            XRAY_COLOR,
            INVALID_TUBI_LINE_WIDTH_PX,
        ));
        let mut app = App::new();
        let mut view = test_view();
        view.selected.insert(owner);
        view.selection_dirty = true;
        app.add_plugins(MinimalPlugins)
            .insert_resource(XRayMaterial(Handle::default()))
            .insert_resource(InvalidTubiLineStateMaterials {
                highlight: line_highlight.clone(),
                xray: line_xray,
            })
            .insert_resource(view)
            .add_systems(Update, apply_selection);
        let entity = app
            .world_mut()
            .spawn((
                InvalidTubiLine {
                    refno,
                    owner,
                    base: line_base.clone(),
                    highlight: line_highlight.clone(),
                },
                MeshMaterial3d(line_base.clone()),
            ))
            .id();

        app.update();
        let shown = app
            .world()
            .entity(entity)
            .get::<MeshMaterial3d<InvalidTubiLineMaterial>>()
            .unwrap()
            .0
            .clone();
        assert_eq!(shown, line_highlight);

        {
            let mut view = app.world_mut().resource_mut::<View3d>();
            view.selected.clear();
            view.selection_dirty = true;
        }
        app.update();
        assert_eq!(
            app.world()
                .entity(entity)
                .get::<MeshMaterial3d<InvalidTubiLineMaterial>>()
                .unwrap()
                .0,
            line_base
        );
    }

    #[test]
    fn room_xray_overrides_selection_and_restores_the_highlight() {
        let refno = RefU64::from(42);
        let owner = RefU64::from(7);
        let mut materials = Assets::<StandardMaterial>::default();
        let base = materials.add(model_material(type_color("PIPE")));
        let highlight = materials.add(model_material(SELECT_COLOR));
        let xray_material_value = xray_material();
        assert_eq!(
            xray_material_value.base_color.to_srgba().to_u8_array(),
            [89, 169, 255, 64]
        );
        assert_eq!(xray_material_value.alpha_mode, AlphaMode::Blend);
        assert!(xray_material_value.unlit);
        assert!(xray_material_value.double_sided);
        assert_eq!(xray_material_value.cull_mode, None);
        let xray = materials.add(xray_material_value);
        let mut app = App::new();
        let mut view = test_view();
        view.selected.insert(owner);
        view.xray.insert(refno);
        view.material_dirty = true;
        app.add_plugins(MinimalPlugins)
            .insert_resource(XRayMaterial(xray.clone()))
            .insert_resource(test_invalid_line_state())
            .insert_resource(view)
            .add_systems(Update, apply_selection);
        let entity = app
            .world_mut()
            .spawn((
                ModelMesh {
                    refno,
                    owner,
                    base: base.clone(),
                    highlight: highlight.clone(),
                },
                MeshMaterial3d(base.clone()),
            ))
            .id();

        app.update();
        assert_eq!(
            app.world()
                .entity(entity)
                .get::<MeshMaterial3d<StandardMaterial>>()
                .unwrap()
                .0,
            xray
        );

        {
            let mut view = app.world_mut().resource_mut::<View3d>();
            view.xray.clear();
            view.material_dirty = true;
        }
        app.update();
        assert_eq!(
            app.world()
                .entity(entity)
                .get::<MeshMaterial3d<StandardMaterial>>()
                .unwrap()
                .0,
            highlight
        );

        {
            let mut view = app.world_mut().resource_mut::<View3d>();
            view.selected.clear();
            view.selection_dirty = true;
        }
        app.update();
        assert_eq!(
            app.world()
                .entity(entity)
                .get::<MeshMaterial3d<StandardMaterial>>()
                .unwrap()
                .0,
            base
        );
    }

    #[test]
    fn headlight_turns_with_the_camera_and_keeps_the_reference_illuminance() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).add_systems(
            PostUpdate,
            aim_headlight.before(TransformSystems::Propagate),
        );
        let camera = app
            .world_mut()
            .spawn((
                ViewCamera,
                Transform::from_xyz(0.0, 0.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
            ))
            .id();
        let light = app
            .world_mut()
            .spawn((
                DirectionalLight {
                    illuminance: 1_600.0,
                    ..default()
                },
                Transform::default(),
                Headlight,
            ))
            .id();

        app.update();
        let before = app
            .world()
            .entity(light)
            .get::<Transform>()
            .unwrap()
            .rotation;
        app.world_mut()
            .entity_mut(camera)
            .insert(Transform::from_xyz(5.0, 0.0, 0.0).looking_at(Vec3::ZERO, Vec3::Y));
        app.update();

        let light = app.world().entity(light);
        let after = light.get::<Transform>().unwrap();
        assert!(before.angle_between(after.rotation) > 0.5);
        assert!(after.forward().y < 0.0);
        assert_eq!(
            light.get::<DirectionalLight>().unwrap().illuminance,
            1_600.0
        );
    }

    #[test]
    fn orbit_keeps_an_off_center_anchor_stable() {
        let focus = Vec3::new(2.0, 0.0, 0.0);
        let mut camera = Transform::from_xyz(0.0, 0.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y);
        let anchor_angle = camera
            .forward()
            .angle_between((focus - camera.translation).normalize());

        orbit_camera(&mut camera, focus, 1.0, 0.0);

        let moved_anchor_angle = camera
            .forward()
            .angle_between((focus - camera.translation).normalize());
        assert!(
            (moved_anchor_angle - anchor_angle).abs() < 1e-4,
            "旋转起手把指针锚点瞬间拉到了画面中心"
        );
    }

    #[test]
    fn zoom_keeps_an_off_center_anchor_stable() {
        let focus = Vec3::new(2.0, 0.0, 0.0);
        let mut camera = Transform::from_xyz(0.0, 0.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y);
        let anchor_angle = camera
            .forward()
            .angle_between((focus - camera.translation).normalize());
        let distance = camera.translation.distance(focus);

        zoom_camera(&mut camera, focus, 100.0);

        let moved_anchor_angle = camera
            .forward()
            .angle_between((focus - camera.translation).normalize());
        assert!(
            camera.translation.distance(focus) < distance,
            "相机没有拉近"
        );
        assert!(
            (moved_anchor_angle - anchor_angle).abs() < 1e-4,
            "缩放把指针锚点拉到了画面中心"
        );
    }

    /// 换档表逐档钉死。它既是显示稳定性的根，也是 HUD 那句「一格 x」的唯一
    /// 出处——档位一改，用户读到的尺寸就跟着改，所以拿具体距离对具体读数。
    #[test]
    fn grid_cells_snap_to_the_one_two_five_ladder() {
        assert_eq!(grid_cell_mm(12.0), 100.0);
        assert_eq!(grid_cell_mm(30.0), 200.0);
        assert_eq!(grid_cell_mm(50.0), 500.0);
        assert_eq!(grid_cell_mm(100.0), 1_000.0);
        assert_eq!(grid_cell_mm(1_200.0), 10_000.0);
        // 相机贴到构件表面：停在 1 mm，模型自身的精度量级。
        assert_eq!(grid_cell_mm(0.0), GRID_MIN_MM);
        // 网格与三轴收到的是同一档过一次 `MODEL_SCALE` 的世界单位。
        assert_eq!(grid_level(12.0), 1.0);
        assert_eq!(grid_level(1_200.0), 100.0);
    }

    /// 渐变面片的顶点色：下两枚是 bottom、上两枚是 top。铺反了渐变就头脚倒置，
    /// 而两套主题里都是「上深下浅 / 上浅下深」各半，肉眼未必立刻看出铺反。
    #[test]
    fn the_gradient_quad_paints_bottom_below_and_top_above() {
        let mesh = gradient_mesh(Color::srgb(1.0, 0.0, 0.0), Color::srgb(0.0, 0.0, 1.0));
        let Some(VertexAttributeValues::Float32x4(colors)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR)
        else {
            panic!("渐变面片缺顶点色");
        };
        assert!(
            colors[0][2] > 0.9 && colors[1][2] > 0.9,
            "下沿该是 bottom 色"
        );
        assert!(colors[2][0] > 0.9 && colors[3][0] > 0.9, "上沿该是 top 色");
    }

    /// Snap 动画在 0.3s 里把相机滑到目标位并对准焦点，走完自我了断。
    /// 时间手动推进：这条测的是插值终点与收尾，不是壁钟。
    #[test]
    fn snapping_glides_the_camera_to_the_target_and_finishes() {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.insert_resource(OrbitCamera::default());
        app.add_systems(Update, animate_snap);
        let camera = app
            .world_mut()
            .spawn((
                ViewCamera,
                Transform::from_xyz(0.0, 0.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
            ))
            .id();
        let start = *app.world().entity(camera).get::<Transform>().unwrap();
        app.world_mut().resource_mut::<OrbitCamera>().anim =
            Some(snap_anim(&start, Vec3::ZERO, Vec3::NEG_X, Vec3::Y, 10.0));

        for _ in 0..10 {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(Duration::from_millis(50));
            app.update();
        }

        let camera = app.world().entity(camera).get::<Transform>().unwrap();
        assert!(
            camera.translation.distance(Vec3::new(10.0, 0.0, 0.0)) < 1e-3,
            "终点位不对：{:?}",
            camera.translation
        );
        assert!(camera.forward().dot(Vec3::NEG_X) > 0.999, "没对准焦点");
        assert!(
            app.world().resource::<OrbitCamera>().anim.is_none(),
            "动画走完没有自我了断"
        );
    }
}
