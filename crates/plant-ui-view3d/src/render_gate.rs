//! 三维视口的空闲停画：场景这一帧没变，就把离屏相机关掉（`Camera::is_active = false`），整条渲染链
//! （prepass、主通道、E3D 后处理）都不跑；离屏图保留上一帧的内容，egui 照常把它铺到视口上。
//!
//! 不走 winit 的被动刷新：数据桥的轮询按 Bevy 帧数计时，主循环一降频，它们就会拖长、后台结果也唤不醒
//! 主循环。这里只停三维这一路，Update 与 UI 仍按原帧率走。
//!
//! 「变了」的判据都在 [`SceneChanges`] 与 [`gate_viewport_render`] 里，任一项成立就画，之后再补画
//! [`SETTLE_FRAMES`] 帧，用来盖住异步编译的管线、GPU 资产上传和流水线渲染慢一拍的滞后。
//! `PLANT_UI_IDLE_RENDER=0` 关掉闸门（永远画），排查画面问题时用。

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use bevy::anti_aliasing::fxaa::Fxaa;
use bevy::core_pipeline::prepass::NormalPrepass;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::render::render_resource::PipelineCache;
use bevy::render::view::PreviousVisibleEntities;

use crate::e3d_look::material::SglLookMaterial;
use crate::e3d_look::post::SglLookPost;
use crate::{OrbitCamera, View3d, ViewCamera, ViewportRenderCamera};

/// 最后一次变化之后再画几帧。管线编完、资产传上 GPU 的那一帧画面才完整；流水线渲染下 render world
/// 回报的「还有管线没编完」又比主世界晚一帧，三帧把这几处滞后都盖住。
pub(crate) const SETTLE_FRAMES: u32 = 3;

#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ViewportRenderGate {
    /// 不再有变化时还要补画的帧数。开机就从满额起算：首帧前几拍没有任何「变化」可依，也得先画出来。
    pub(crate) settle_frames_left: u32,
    /// 闸门开着才会停画；`PLANT_UI_IDLE_RENDER=0` 时为 false，相机永远活跃。
    pub(crate) enabled: bool,
}

impl Default for ViewportRenderGate {
    fn default() -> Self {
        Self {
            settle_frames_left: SETTLE_FRAMES,
            enabled: parse_idle_render_flag(std::env::var("PLANT_UI_IDLE_RENDER").ok().as_deref()),
        }
    }
}

/// 缺省开，只有明确的否定值才关；与 `PLANT_UI_INDIRECT_DRAW` 同一套口径。
pub(crate) fn parse_idle_render_flag(value: Option<&str>) -> bool {
    !value.is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "no" | "off"
        )
    })
}

/// render world 的 `PipelineCache` 里还有没编完的管线。主世界与 render world 共用同一个原子量：
/// 着色器是异步编译的，编完之前停画会把缺了网格或后处理的那一帧定格在视口上。
#[derive(Resource, Clone, Default)]
pub(crate) struct PipelinesPending(Arc<AtomicBool>);

impl PipelinesPending {
    pub(crate) fn get(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    pub(crate) fn set(&self, pending: bool) {
        self.0.store(pending, Ordering::Release);
    }
}

/// render world 一侧：每帧渲染完回报一次队列是否清空。排在 `render_system` 之后，本帧新排进来的管线
/// 这时已经在编译中，算作未完成。
pub(crate) fn publish_pending_pipelines(cache: Res<PipelineCache>, pending: Res<PipelinesPending>) {
    pending.set(cache.waiting_pipelines().next().is_some());
}

type EntityChanged = Or<(
    Changed<GlobalTransform>,
    Changed<Visibility>,
    Changed<InheritedVisibility>,
    Changed<Mesh3d>,
    Changed<MeshMaterial3d<StandardMaterial>>,
    Changed<MeshMaterial3d<SglLookMaterial>>,
    Changed<DirectionalLight>,
)>;

/// 主相机上影响画面的组件。不查 `Changed<Camera>`：闸门自己写 `is_active`，查了就会把自己唤醒。
type CameraChanged = Or<(
    Added<ViewCamera>,
    Changed<Projection>,
    Changed<Msaa>,
    Changed<SglLookPost>,
    Added<NormalPrepass>,
    Added<Fxaa>,
)>;

/// 会让离屏画面变样的实体、组件与资产变化。事件读取器与移除记录每帧都读空，免得旧事件拖到下一帧。
#[derive(SystemParam)]
pub(crate) struct SceneChanges<'w, 's> {
    entities: Query<'w, 's, (), EntityChanged>,
    cameras: Query<'w, 's, (), (With<ViewCamera>, CameraChanged)>,
    removed_meshes: RemovedComponents<'w, 's, Mesh3d>,
    removed_pbr: RemovedComponents<'w, 's, MeshMaterial3d<StandardMaterial>>,
    removed_sgl: RemovedComponents<'w, 's, MeshMaterial3d<SglLookMaterial>>,
    removed_prepass: RemovedComponents<'w, 's, NormalPrepass>,
    removed_fxaa: RemovedComponents<'w, 's, Fxaa>,
    mesh_events: EventReader<'w, 's, AssetEvent<Mesh>>,
    standard_events: EventReader<'w, 's, AssetEvent<StandardMaterial>>,
    sgl_events: EventReader<'w, 's, AssetEvent<SglLookMaterial>>,
    image_events: EventReader<'w, 's, AssetEvent<Image>>,
    ambient: Option<Res<'w, AmbientLight>>,
}

impl SceneChanges<'_, '_> {
    fn any(&mut self) -> bool {
        // 全部求值、不短路：读取器都要读空。
        let mut changed = !self.entities.is_empty() || !self.cameras.is_empty();
        changed |= drain_removed(&mut self.removed_meshes);
        changed |= drain_removed(&mut self.removed_pbr);
        changed |= drain_removed(&mut self.removed_sgl);
        changed |= drain_removed(&mut self.removed_prepass);
        changed |= drain_removed(&mut self.removed_fxaa);
        changed |= drain_assets(&mut self.mesh_events);
        changed |= drain_assets(&mut self.standard_events);
        changed |= drain_assets(&mut self.sgl_events);
        changed |= drain_assets(&mut self.image_events);
        changed |= self
            .ambient
            .as_ref()
            .is_some_and(|ambient| ambient.is_changed());
        changed
    }
}

fn drain_removed<T: Component>(removed: &mut RemovedComponents<T>) -> bool {
    let any = !removed.is_empty();
    removed.clear();
    any
}

/// `Unused` 只是句柄掉光了的通知，资产本身还没动，不算变化。
fn drain_assets<A: Asset>(events: &mut EventReader<AssetEvent<A>>) -> bool {
    events.read().fold(false, |any, event| {
        any | !matches!(event, AssetEvent::Unused { .. })
    })
}

/// 视口自己还有没走完的活：装模型、换离屏图、房间体与孔洞的待办。
///
/// `prepared_resize` 尤其不能停：`ViewportResizeSync` 的回执跟着 render world 的每一帧走、不问相机画没画，
/// 新图预热期间相机一停，回执照样到达，egui 就会换上一张从没画过的空图。
fn view_busy(view: &View3d) -> bool {
    !view.pending_models.is_empty()
        || !view.loading_meshes.is_empty()
        || !view.retrying_meshes.is_empty()
        || view.pending_resize.is_some()
        || view.prepared_resize.is_some()
        || view.image_scale != view.render_scale
        || !view.room_clip_ops.is_empty()
        || !view.virtual_hole_ops.is_empty()
}

/// 跑在变换与继承可见性传播之后、`CheckVisibility` 之前：本帧的相机移动和显隐都看得见，而相机一旦被
/// 重新激活，同一帧的可见性检查就会为它收集可见实体（不活跃的相机会被 `check_visibility` 跳过）。
pub(crate) fn gate_viewport_render(
    mut gate: ResMut<ViewportRenderGate>,
    mut changes: SceneChanges,
    view: Res<View3d>,
    orbit: Res<OrbitCamera>,
    pending: Res<PipelinesPending>,
    mut cameras: Query<&mut Camera, With<ViewportRenderCamera>>,
) {
    let changed = changes.any() | view_busy(&view) | orbit.anim.is_some() | pending.get();
    let active = if changed {
        gate.settle_frames_left = SETTLE_FRAMES;
        true
    } else if gate.settle_frames_left > 0 {
        gate.settle_frames_left -= 1;
        true
    } else {
        false
    };
    let active = active || !gate.enabled;
    for mut camera in &mut cameras {
        // 只在真要切换时写：写了就是 `Changed<Camera>`，渲染侧白白多一次重提取。
        if camera.is_active != active {
            camera.is_active = active;
        }
    }
}

/// 停画期间冻结 `ViewVisibility`。不活跃的相机被 `check_visibility` 跳过，没人再把网格标成可见，
/// `mark_newly_hidden_entities_invisible` 就会把上一帧可见的全部改成不可见，醒来再全部改回去。每改一次，
/// render world 都要把整库网格重提取一遍。停画时清掉「上一帧可见」的名单，可见性原样留着；停画期间场景
/// 本来就没变（一变就会醒），冻住的结果不会过时。
pub(crate) fn freeze_view_visibility_while_idle(
    cameras: Query<&Camera, With<ViewportRenderCamera>>,
    mut previous: ResMut<PreviousVisibleEntities>,
) {
    if !cameras.is_empty() && cameras.iter().all(|camera| !camera.is_active) {
        previous.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Headlight, aim_headlight, blank_view};
    use bevy::asset::AssetPlugin;
    use bevy::transform::{TransformPlugin, TransformSystems};

    fn gate_app() -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), TransformPlugin))
            .init_asset::<Mesh>()
            .init_asset::<StandardMaterial>()
            .init_asset::<SglLookMaterial>()
            .init_asset::<Image>()
            .insert_resource(blank_view())
            .insert_resource(OrbitCamera::default())
            .insert_resource(ViewportRenderGate {
                settle_frames_left: SETTLE_FRAMES,
                enabled: true,
            })
            .init_resource::<PipelinesPending>()
            .add_systems(
                PostUpdate,
                gate_viewport_render.after(TransformSystems::Propagate),
            );
        let camera = app
            .world_mut()
            .spawn((
                Camera::default(),
                Projection::Perspective(PerspectiveProjection::default()),
                Msaa::Off,
                SglLookPost::off(),
                Transform::from_xyz(0.0, 0.0, 10.0),
                ViewCamera,
                ViewportRenderCamera,
            ))
            .id();
        (app, camera)
    }

    fn is_active(app: &App, camera: Entity) -> bool {
        app.world()
            .entity(camera)
            .get::<Camera>()
            .unwrap()
            .is_active
    }

    /// 什么都不动，跑到补帧用完为止，返回此时相机是否已停。
    fn settle(app: &mut App, camera: Entity) -> bool {
        for _ in 0..(SETTLE_FRAMES + 3) {
            app.update();
        }
        !is_active(app, camera)
    }

    #[test]
    fn idle_render_is_on_unless_explicitly_switched_off() {
        for off in ["0", "false", "OFF", " no "] {
            assert!(!parse_idle_render_flag(Some(off)), "{off}");
        }
        for on in ["", "1", "true", "unexpected"] {
            assert!(parse_idle_render_flag(Some(on)), "{on}");
        }
        assert!(parse_idle_render_flag(None));
    }

    #[test]
    fn a_still_scene_stops_rendering_after_the_settle_frames() {
        let (mut app, camera) = gate_app();
        app.update();
        assert!(is_active(&app, camera), "开机头几帧必须画");
        assert!(settle(&mut app, camera), "静止场景没有停画");
        // 停下之后保持停着：闸门自己写 is_active 不能把自己唤醒。
        for _ in 0..5 {
            app.update();
            assert!(!is_active(&app, camera));
        }
    }

    #[test]
    fn the_last_change_is_followed_by_exactly_the_settle_frames() {
        let (mut app, camera) = gate_app();
        assert!(settle(&mut app, camera));
        app.world_mut()
            .entity_mut(camera)
            .get_mut::<Transform>()
            .unwrap()
            .translation
            .x += 1.0;
        app.update();
        assert!(is_active(&app, camera));
        for frame in 0..SETTLE_FRAMES {
            app.update();
            assert!(is_active(&app, camera), "补画第 {frame} 帧就停了");
        }
        app.update();
        assert!(!is_active(&app, camera));
    }

    #[test]
    fn scene_changes_wake_the_camera() {
        type Poke = fn(&mut App, Entity, Entity);
        let pokes: [(&str, Poke); 6] = [
            ("移动相机", |app, camera, _| {
                app.world_mut()
                    .entity_mut(camera)
                    .get_mut::<Transform>()
                    .unwrap()
                    .rotate_y(0.1);
            }),
            ("切换可见性", |app, _, model| {
                app.world_mut().entity_mut(model).insert(Visibility::Hidden);
            }),
            ("换材质", |app, _, model| {
                let handle = app
                    .world_mut()
                    .resource_mut::<Assets<StandardMaterial>>()
                    .add(StandardMaterial::default());
                app.world_mut()
                    .entity_mut(model)
                    .insert(MeshMaterial3d(handle));
            }),
            ("发起 resize", |app, _, _| {
                app.world_mut().resource_mut::<View3d>().pending_resize =
                    Some((UVec2::new(64, 64), web_time::Instant::now()));
            }),
            ("视角跳转动画", |app, camera, _| {
                let start = *app.world().entity(camera).get::<Transform>().unwrap();
                app.world_mut().resource_mut::<OrbitCamera>().anim = Some(crate::snap_anim(
                    &start,
                    Vec3::ZERO,
                    Vec3::NEG_X,
                    Vec3::Y,
                    10.0,
                ));
            }),
            ("后处理参数变了", |app, camera, _| {
                app.world_mut()
                    .entity_mut(camera)
                    .insert(SglLookPost::new(true, true, 1.0, 1));
            }),
        ];
        for (what, poke) in pokes {
            let (mut app, camera) = gate_app();
            let model = app
                .world_mut()
                .spawn((Transform::default(), Visibility::Visible))
                .id();
            assert!(settle(&mut app, camera), "{what}：前置静止没有停画");
            poke(&mut app, camera, model);
            app.update();
            assert!(is_active(&app, camera), "{what} 没有唤醒相机");
        }
    }

    #[test]
    fn mesh_spawn_and_despawn_wake_the_camera() {
        let (mut app, camera) = gate_app();
        assert!(settle(&mut app, camera));
        let mesh = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(Mesh::from(Cuboid::default()));
        let entity = app.world_mut().spawn(Mesh3d(mesh)).id();
        app.update();
        assert!(is_active(&app, camera), "新网格没有唤醒相机");

        assert!(settle(&mut app, camera));
        app.world_mut().despawn(entity);
        app.update();
        assert!(is_active(&app, camera), "删网格没有唤醒相机");
    }

    #[test]
    fn pending_pipelines_keep_the_camera_rendering() {
        let (mut app, camera) = gate_app();
        app.world().resource::<PipelinesPending>().set(true);
        for _ in 0..(SETTLE_FRAMES * 4) {
            app.update();
            assert!(is_active(&app, camera), "管线没编完就停画了");
        }
        app.world().resource::<PipelinesPending>().set(false);
        assert!(settle(&mut app, camera));
    }

    #[test]
    fn a_disabled_gate_never_stops_rendering() {
        let (mut app, camera) = gate_app();
        app.world_mut().resource_mut::<ViewportRenderGate>().enabled = false;
        for _ in 0..(SETTLE_FRAMES * 4) {
            app.update();
            assert!(is_active(&app, camera));
        }
    }

    #[test]
    fn view_visibility_is_frozen_only_while_the_viewport_is_idle() {
        let (mut app, camera) = gate_app();
        app.init_resource::<PreviousVisibleEntities>().add_systems(
            PostUpdate,
            freeze_view_visibility_while_idle.after(gate_viewport_render),
        );
        let model = app.world_mut().spawn_empty().id();
        let mark_visible = |app: &mut App| {
            app.world_mut()
                .resource_mut::<PreviousVisibleEntities>()
                .insert(model);
        };

        mark_visible(&mut app);
        app.update();
        assert!(is_active(&app, camera));
        assert!(
            app.world()
                .resource::<PreviousVisibleEntities>()
                .contains(&model),
            "相机活跃时不能动可见性名单"
        );

        assert!(settle(&mut app, camera));
        mark_visible(&mut app);
        app.update();
        assert!(
            app.world().resource::<PreviousVisibleEntities>().is_empty(),
            "停画时名单没清，网格会被标成不可见"
        );
    }

    #[test]
    fn the_headlight_is_not_rewritten_while_the_camera_is_still() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).add_systems(
            PostUpdate,
            aim_headlight.before(TransformSystems::Propagate),
        );
        app.world_mut().spawn((
            ViewCamera,
            Transform::from_xyz(0.0, 0.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
        ));
        let light = app
            .world_mut()
            .spawn((Transform::default(), Headlight))
            .id();
        app.update();
        let aimed = app
            .world()
            .entity(light)
            .get_ref::<Transform>()
            .unwrap()
            .last_changed();
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(
            app.world()
                .entity(light)
                .get_ref::<Transform>()
                .unwrap()
                .last_changed(),
            aimed,
            "相机没动，头灯的 Transform 却每帧都被标记为已改"
        );
    }
}
