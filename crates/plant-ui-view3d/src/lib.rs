//! 独立三维视口模块：模型装卸、离屏渲染、相机控制与拾取，以及视口显示基建
//! ——主题渐变背景、地面网格与原点三色轴、ViewCube 视角跳转动画。
//!
//! 立方体本体不在这儿：它是 egui 侧的正交小部件（`plant_ui::workbench::viewcube`），
//! 这一侧只负责把相机姿态逐帧发布出去、并执行它发回的 `SnapView`。曾有一版
//! 第二正交相机 + RenderLayer 的场景内立方体走到一半，被换掉的原因写在
//! viewcube 模块头上：中文面标、悬停高亮、26 区命中在 egui 里是二十行投影，
//! 在场景里是字体栅格化加射线拾取一整套。

use std::collections::{HashMap, VecDeque};
use std::f32::consts::FRAC_PI_2;
use web_time::{Duration, Instant};

use aios_core::shape::pdms_shape::PlantMesh;
use aios_core::{GeomInstQuery, RefU64};
use bevy::asset::io::Reader;
use bevy::asset::{AssetLoader, AssetServer, Assets, LoadContext};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::prelude::*;
use bevy::render::camera::{ClearColorConfig, RenderTarget, ScalingMode};
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::{
    Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
};
use bevy::render::view::RenderLayers;
use bevy::transform::TransformSystems;
use bevy_egui35::{EguiUserTextures, egui};
use plant_ui::{CameraGesture, CameraMotion, ModelAction};

const INITIAL_SIZE: UVec2 = UVec2::new(1200, 700);
const RESIZE_DEBOUNCE: Duration = Duration::from_millis(120);
const MODEL_SCALE: f32 = 0.01;
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
/// 原点三轴长（单位：格）。轴比网格短，尖端落在格子里而不是伸出场外。
const AXIS_CELLS: f32 = 6.0;
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
/// 开机默认配色（深色主题的 viewport tokens：#232F3A / #0E1318 / #46586A）。
/// 首帧 App 就会按当前主题发 `SetViewportBackground` 盖掉，这里只求
/// 「主题命令到达前别闪白」。
const DEFAULT_TOP: Color = Color::srgb(0.137, 0.184, 0.227);
const DEFAULT_BOTTOM: Color = Color::srgb(0.055, 0.075, 0.094);
const DEFAULT_GRID: Color = Color::srgb(0.275, 0.345, 0.416);

pub struct View3dPlugin;

impl Plugin for View3dPlugin {
    fn build(&self, app: &mut bevy::prelude::App) {
        app.init_asset_loader::<MeshLoader>()
            .add_systems(Startup, setup)
            .add_systems(
                Update,
                (
                    load_models,
                    apply_resize,
                    apply_commands,
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
    }
}

#[derive(Resource)]
pub struct View3d {
    pub texture: egui::TextureId,
    pub size: UVec2,
    /// 相机世界旋转的三列：right / up / back。egui 侧 ViewCube 的全部输入。
    pub camera_rot: [[f32; 3]; 3],
    /// X / Y / Z 轴端标签在渲染纹理上的归一化 UV；出画或在相机身后为 None。
    pub axis_labels: [Option<[f32; 2]>; 3],
    image: Handle<Image>,
    commands: VecDeque<ViewCommand>,
    pending_models: Option<Vec<GeomInstQuery>>,
    pending_resize: Option<(UVec2, Instant)>,
    picked: Option<RefU64>,
    bounds: HashMap<RefU64, (Vec3, Vec3)>,
}

impl View3d {
    pub fn load(&mut self, models: Vec<GeomInstQuery>) {
        self.pending_models = Some(models);
    }

    pub fn resize(&mut self, size: [u32; 2]) {
        let wanted = UVec2::new(size[0].max(16), size[1].max(16));
        if wanted == self.size {
            self.pending_resize = None;
        } else if self.pending_resize.map(|(size, _)| size) != Some(wanted) {
            self.pending_resize = Some((wanted, Instant::now()));
        }
    }

    pub fn camera(&mut self, gesture: CameraGesture) {
        self.commands.push_back(ViewCommand::Camera(gesture));
    }

    pub fn model(&mut self, action: ModelAction) {
        self.commands.push_back(ViewCommand::Model(action));
    }

    pub fn pick(&mut self, uv: [f32; 2]) {
        // TODO(诊断): 拾取排查完删除。
        diag(format!("[诊断] UI 发出拾取 uv={uv:?}"));
        self.commands.push_back(ViewCommand::Pick(uv));
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

#[derive(Component, Clone, Copy)]
struct ModelMesh(RefU64);

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

fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut egui_textures: ResMut<EguiUserTextures>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let image = images.add(viewport_image(INITIAL_SIZE));
    let texture = egui_textures.add_image(image.clone());
    commands.insert_resource(View3d {
        texture,
        size: INITIAL_SIZE,
        camera_rot: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        axis_labels: [None; 3],
        image: image.clone(),
        commands: VecDeque::new(),
        pending_models: None,
        pending_resize: None,
        picked: None,
        bounds: HashMap::new(),
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

    // 地面网格（PDMS Z=0，过装载旋转即世界 y=0 面）与原点三色轴。
    let grid_mesh = meshes.add(build_grid_mesh(1.0, DEFAULT_GRID));
    commands.insert_resource(GridMesh(grid_mesh.clone()));
    commands.insert_resource(GridState {
        level: 1.0,
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
    let rod_mesh = meshes.add(Cylinder::new(0.032, AXIS_CELLS));
    let tip_mesh = meshes.add(Cone {
        radius: 0.11,
        height: 0.45,
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
                            Transform::from_xyz(0.0, AXIS_CELLS + 0.22, 0.0),
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

/// 相机离焦点多远，格距取几：10 的整幂，视野里保持十来格的密度。
/// 上下限把「相机贴脸」与「拉到平流层」两头都夹住。
fn grid_level(dist: f32) -> f32 {
    let raw = (dist.max(0.001) / 12.0).log10().round() as i32;
    10f32.powi(raw).clamp(0.001, 1_000_000.0)
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
    let tip = (AXIS_CELLS + 0.9) * grid.level;
    for (axis, dir) in AXIS_DIRS.into_iter().enumerate() {
        view.axis_labels[axis] = camera
            .world_to_ndc(transform, dir * tip)
            .filter(|ndc| ndc.z > 0.0 && ndc.z < 1.0)
            .map(|ndc| [ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5]);
    }
}

fn load_models(
    mut commands: Commands,
    mut view: ResMut<View3d>,
    mut orbit: ResMut<OrbitCamera>,
    mut camera: Query<(&mut Transform, &mut Projection), With<ViewCamera>>,
    roots: Query<Entity, With<SceneRoot>>,
    assets: Res<AssetServer>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(models) = view.pending_models.take() else {
        return;
    };
    view.bounds.clear();
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
    for entity in &roots {
        commands.entity(entity).despawn();
    }
    let material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.54, 0.68, 0.78),
        unlit: true,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    commands
        .spawn((scene_transform, Visibility::Visible, SceneRoot))
        .with_children(|scene| {
            for model in models {
                let refno = model.refno.refno();
                scene
                    .spawn((
                        model.world_trans,
                        Visibility::Visible,
                        ModelRoot {
                            refno,
                            owner: model.owner.refno(),
                        },
                    ))
                    .with_children(|element| {
                        for inst in model.insts {
                            element.spawn((
                                Mesh3d(assets.load(format!("meshes/{}.mesh", inst.geo_hash))),
                                MeshMaterial3d(material.clone()),
                                inst.transform,
                                ModelMesh(refno),
                            ));
                        }
                    });
            }
        });
}

fn apply_resize(mut view: ResMut<View3d>, mut images: ResMut<Assets<Image>>) {
    let Some((wanted, asked_at)) = view.pending_resize else {
        return;
    };
    if asked_at.elapsed() < RESIZE_DEBOUNCE {
        return;
    }
    view.pending_resize = None;
    let Some(image) = images.get_mut(&view.image) else {
        return;
    };
    image.resize(Extent3d {
        width: wanted.x,
        height: wanted.y,
        ..default()
    });
    view.size = wanted;
}

#[allow(clippy::too_many_arguments)]
fn apply_commands(
    mut view: ResMut<View3d>,
    mut orbit: ResMut<OrbitCamera>,
    mut camera: Query<
        (&Camera, &GlobalTransform, &mut Transform, &mut Projection),
        With<ViewCamera>,
    >,
    mut roots: Query<(&ModelRoot, &mut Visibility)>,
    meshes: Query<&ModelMesh>,
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
                    for (root, visibility) in &roots {
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
                ModelAction::SetVisible { refnos, visible } => {
                    for (root, mut visibility) in &mut roots {
                        if refnos.contains(&root.refno) || refnos.contains(&root.owner) {
                            *visibility = if visible {
                                Visibility::Visible
                            } else {
                                Visibility::Hidden
                            };
                        }
                    }
                }
                ModelAction::HideAll => {
                    for (_, mut visibility) in &mut roots {
                        *visibility = Visibility::Hidden;
                    }
                }
                ModelAction::ShowAll => {
                    for (_, mut visibility) in &mut roots {
                        *visibility = Visibility::Visible;
                    }
                }
                ModelAction::Focus(refno) => {
                    if let Some(&(min, max)) = view.bounds.get(&refno) {
                        orbit.anim = None;
                        frame_bounds(min, max, &mut orbit, &mut camera_transform, &mut projection);
                    }
                }
                ModelAction::FitAll => {
                    let mut min = Vec3::splat(f32::INFINITY);
                    let mut max = Vec3::splat(f32::NEG_INFINITY);
                    for (root, visibility) in &roots {
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
                            let offset = camera_transform.translation - orbit.focus;
                            let yaw = Quat::from_rotation_y(-x * 0.005);
                            let right = camera_transform.rotation * Vec3::X;
                            let pitch = Quat::from_axis_angle(right, -y * 0.005);
                            camera_transform.translation = orbit.focus + yaw * pitch * offset;
                            camera_transform.look_at(orbit.focus, Vec3::Y);
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
                        let offset = camera_transform.translation - orbit.focus;
                        camera_transform.translation =
                            orbit.focus + offset * (amount * -0.002).exp().clamp(0.2, 5.0);
                        camera_transform.look_at(orbit.focus, Vec3::Y);
                    }
                    CameraGesture::End => orbit.motion = None,
                }
            }
        }
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
    meshes: &Query<&ModelMesh>,
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
    Some((meshes.get(*entity).ok()?.0, hit.point))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::render::mesh::VertexAttributeValues;

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
    fn repeated_resize_request_keeps_the_original_debounce_deadline() {
        let mut view = View3d {
            texture: egui::TextureId::Managed(0),
            size: UVec2::new(1200, 700),
            camera_rot: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            axis_labels: [None; 3],
            image: Handle::default(),
            commands: VecDeque::new(),
            pending_models: None,
            pending_resize: None,
            picked: None,
            bounds: HashMap::new(),
        };
        let first_asked_at = Instant::now() - Duration::from_secs(1);
        view.pending_resize = Some((UVec2::new(1000, 600), first_asked_at));

        view.resize([1000, 600]);

        assert_eq!(
            view.pending_resize,
            Some((UVec2::new(1000, 600), first_asked_at)),
            "逐帧重复请求同一尺寸不应把防抖计时归零"
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

    /// 格距永远是 10 的整幂，距离归零也有下限——网格换档表是显示稳定性的根。
    #[test]
    fn grid_level_snaps_to_powers_of_ten() {
        assert_eq!(grid_level(12.0), 1.0);
        assert_eq!(grid_level(1.0), 0.1);
        assert_eq!(grid_level(1_200.0), 100.0);
        assert_eq!(grid_level(0.0), 0.001);
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
