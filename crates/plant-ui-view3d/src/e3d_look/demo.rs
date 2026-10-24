//! `--look-demo`：不要后端也能做像素验收的演示厂（E3D 外观移植计划阶段一「调试入口」）。
//!
//! 对应 plant3d-web `demo/buildDemoPlant.ts` 的骨架：几根直管、一台设备盒、一根钢柱、一间半透明房、
//! 一个 aids 蓝球。几何直接用 Bevy 基本体在 PDMS 毫米系里摆，走与真模型相同的 `SceneRoot` /
//! `ModelRoot` / `ModelMesh` 路径，所以拾取、选中、换外观、换配色都照常——只是网格不来自文件。
//! debug 构建总编进去，release 构建要开 `look-demo` feature；宿主见到 `--look-demo`（浏览器端 `?look_demo=1`）就插一枚 [`LookDemo`] 资源。演示厂开着时每隔一阵往控制台
//! 打一行平均帧时间（[`report_frame_time`]），是计划里「用 FrameTimeDiagnostics 量帧时间」那一条的落点。

use std::f32::consts::FRAC_PI_2;

use aios_core::RefU64;
use bevy::color::ColorToPacked;
use bevy::prelude::*;
use web_time::{Duration, Instant};

use super::colour_table::{E3D_AIDS_COLOUR, rgb_bytes};
use crate::{
    ModelMesh, ModelPickTarget, ModelRoot, OrbitCamera, SceneRoot, View3d, ViewCamera, diag,
    extend_bounds, fallback_line_material, frame_bounds, scene_bounds_from_mm, scene_transform,
    type_color,
};

/// 帧时间汇总的窗口。
const FRAME_TIME_WINDOW: Duration = Duration::from_secs(3);

/// 演示厂的帧时间账：窗口内的帧数与起点。
#[derive(Default)]
pub(crate) struct FrameTimeWindow {
    started: Option<Instant>,
    frames: u32,
}

/// 每隔 [`FRAME_TIME_WINDOW`] 打一行平均帧时间与当前离屏图的物理尺寸。没有 [`LookDemo`] 资源就什么都不做。
pub(crate) fn report_frame_time(
    demo: Option<Res<LookDemo>>,
    view: Res<View3d>,
    mut window: Local<FrameTimeWindow>,
) {
    if demo.is_none() {
        return;
    }
    let now = Instant::now();
    let started = *window.started.get_or_insert(now);
    window.frames += 1;
    let elapsed = now.duration_since(started);
    if elapsed < FRAME_TIME_WINDOW || window.frames == 0 {
        return;
    }
    let ms = elapsed.as_secs_f64() * 1000.0 / f64::from(window.frames);
    let physical = view.size * view.image_scale;
    // 后三项是设置值；设备没有半浮点颜色附件时它们并没有生效，末尾说明，免得日志把没有的东西说成有。
    let effects_note = if view.post_effects_supported() {
        ""
    } else {
        "（设置值；这台设备没有半浮点颜色附件，边线 / 伪阴影 / 超采样都没生效）"
    };
    diag(format!(
        "[look-demo] 平均帧时间 {ms:.2} ms（{:.0} fps），离屏图 {}×{}（逻辑 {}×{} × {}），边线 {} 伪阴影 {} 抗锯齿 {:?}{effects_note}",
        1000.0 / ms,
        physical.x,
        physical.y,
        view.size.x,
        view.size.y,
        view.image_scale,
        view.look.hlr,
        view.look.ao,
        view.look.aa,
    ));
    window.started = Some(now);
    window.frames = 0;
}

/// 宿主插上它，`PostStartup` 就把演示厂摆进场景。
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct LookDemo;

/// 演示厂的构件编号从这里起：高到不与任何真 refno 撞车。
const FIRST_REFNO: u64 = 9_000_000_001;

/// PDMS 毫米系里的轴向。
#[derive(Clone, Copy)]
enum Axis {
    X,
    Y,
    Z,
}

enum DemoShape {
    /// 直管：直径、长度、轴向。
    Pipe {
        diameter: f32,
        length: f32,
        along: Axis,
    },
    /// 盒子：三边长（PDMS X / Y / Z）。
    Box { size: [f32; 3] },
}

/// 演示厂的一件构件。
struct DemoPart {
    generic: &'static str,
    shape: DemoShape,
    /// 构件中心（PDMS 毫米，Z-up）。
    centre: [f32; 3],
    /// alpha 字节；255 = 不透明。
    alpha: u8,
}

const PARTS: &[DemoPart] = &[
    DemoPart {
        generic: "PIPE",
        shape: DemoShape::Pipe {
            diameter: 300.0,
            length: 5000.0,
            along: Axis::X,
        },
        centre: [0.0, 0.0, 1200.0],
        alpha: 255,
    },
    DemoPart {
        generic: "PIPE",
        shape: DemoShape::Pipe {
            diameter: 200.0,
            length: 3500.0,
            along: Axis::Y,
        },
        centre: [1800.0, 0.0, 700.0],
        alpha: 255,
    },
    DemoPart {
        generic: "PIPE",
        shape: DemoShape::Pipe {
            diameter: 250.0,
            length: 2400.0,
            along: Axis::Z,
        },
        centre: [-1800.0, 1200.0, 1200.0],
        alpha: 255,
    },
    DemoPart {
        generic: "EQUI",
        shape: DemoShape::Box {
            size: [1600.0, 1200.0, 900.0],
        },
        centre: [0.0, -1900.0, 450.0],
        alpha: 255,
    },
    DemoPart {
        generic: "STRU",
        shape: DemoShape::Box {
            size: [200.0, 200.0, 3200.0],
        },
        centre: [2800.0, 1800.0, 1600.0],
        alpha: 255,
    },
    DemoPart {
        generic: "STRU",
        shape: DemoShape::Box {
            size: [4200.0, 150.0, 300.0],
        },
        centre: [700.0, 1800.0, 3050.0],
        alpha: 255,
    },
    // 半透明房间盒：验半透明路径（E3D 外观下也是 lightgrey，只是透）。
    DemoPart {
        generic: "ROOM",
        shape: DemoShape::Box {
            size: [7000.0, 6000.0, 3400.0],
        },
        centre: [0.0, 0.0, 1700.0],
        alpha: 64,
    },
];

/// aids 蓝球：直径与位置（PDMS 毫米）。
const AIDS_SPHERE_DIAMETER: f32 = 400.0;
const AIDS_SPHERE_CENTRE: [f32; 3] = [-600.0, -600.0, 200.0];

impl DemoShape {
    /// 网格、局部→PDMS 的旋转、以及摆好之后的半长（PDMS 轴对齐）。
    fn build(&self) -> (Mesh, Quat, Vec3) {
        match *self {
            DemoShape::Pipe {
                diameter,
                length,
                along,
            } => {
                let radius = diameter * 0.5;
                // Bevy 的 Cylinder 沿局部 Y；转到 PDMS 的轴向上。
                let (rotation, half) = match along {
                    Axis::X => (
                        Quat::from_rotation_z(-FRAC_PI_2),
                        Vec3::new(length * 0.5, radius, radius),
                    ),
                    Axis::Y => (Quat::IDENTITY, Vec3::new(radius, length * 0.5, radius)),
                    Axis::Z => (
                        Quat::from_rotation_x(FRAC_PI_2),
                        Vec3::new(radius, radius, length * 0.5),
                    ),
                };
                (Mesh::from(Cylinder::new(radius, length)), rotation, half)
            }
            DemoShape::Box { size } => {
                let size = Vec3::from_array(size);
                (
                    Mesh::from(Cuboid::from_size(size)),
                    Quat::IDENTITY,
                    size * 0.5,
                )
            }
        }
    }
}

/// 把演示厂摆进场景并取景。没有 [`LookDemo`] 资源就什么都不做。
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_look_demo(
    demo: Option<Res<LookDemo>>,
    mut commands: Commands,
    mut view: ResMut<View3d>,
    mut orbit: ResMut<OrbitCamera>,
    mut camera: Query<(&mut Transform, &mut Projection), With<ViewCamera>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if demo.is_none() {
        return;
    }
    let scene = commands
        .spawn((scene_transform(), Visibility::Visible, SceneRoot))
        .id();
    let mut all_min = Vec3::splat(f32::INFINITY);
    let mut all_max = Vec3::splat(f32::NEG_INFINITY);
    for (index, part) in PARTS.iter().enumerate() {
        let refno = RefU64(FIRST_REFNO + index as u64);
        let (mesh, rotation, half) = part.shape.build();
        let centre = Vec3::from_array(part.centre);
        if let Some((min, max)) =
            scene_bounds_from_mm((centre - half).to_array(), (centre + half).to_array())
        {
            extend_bounds(&mut view.bounds, refno, min, max);
            all_min = all_min.min(min);
            all_max = all_max.max(max);
        }
        let mut colour = type_color(part.generic).to_srgba().to_u8_array();
        colour[3] = part.alpha;
        let mesh = meshes.add(mesh);
        commands
            .spawn((
                Transform::from_translation(centre).with_rotation(rotation),
                Visibility::Visible,
                ModelRoot {
                    refno,
                    owner: refno,
                },
                ChildOf(scene),
            ))
            .with_children(|element| {
                element.spawn((
                    Mesh3d(mesh),
                    Transform::IDENTITY,
                    ModelMesh {
                        refno,
                        owner: refno,
                        colour,
                        line: false,
                    },
                    ModelPickTarget { refno },
                ));
            });
    }
    // aids 蓝球：辅助几何走无光照 StandardMaterial（同地面网格 / 三轴），不挂 `ModelMesh`，
    // 不进选中与可见性账；阶段二起它就是「不进 prepass = 不描边、不接伪阴影」的样本。
    let [r, g, b] = rgb_bytes(E3D_AIDS_COLOUR);
    commands.spawn((
        Mesh3d(meshes.add(Mesh::from(Sphere::new(AIDS_SPHERE_DIAMETER * 0.5)))),
        MeshMaterial3d(materials.add(fallback_line_material(Color::srgb_u8(r, g, b)))),
        Transform::from_translation(Vec3::from_array(AIDS_SPHERE_CENTRE)),
        ChildOf(scene),
    ));
    if all_min.is_finite()
        && all_max.is_finite()
        && let Ok((mut camera, mut projection)) = camera.single_mut()
    {
        frame_bounds(all_min, all_max, &mut orbit, &mut camera, &mut projection);
    }
}
