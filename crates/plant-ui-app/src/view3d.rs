use std::collections::{HashMap, VecDeque};
use std::f32::consts::FRAC_PI_2;
use std::time::{Duration, Instant};

use aios_core::shape::pdms_shape::PlantMesh;
use aios_core::{GeomInstQuery, RefU64};
use bevy::asset::io::Reader;
use bevy::asset::{AssetLoader, AssetServer, Assets, LoadContext};
use bevy::prelude::*;
use bevy::render::camera::{ClearColorConfig, RenderTarget};
use bevy::render::render_resource::{
    Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
};
use bevy_egui35::{EguiUserTextures, egui};
use plant_ui::{CameraGesture, CameraMotion, ModelAction};

const INITIAL_SIZE: UVec2 = UVec2::new(1200, 700);
const RESIZE_DEBOUNCE: Duration = Duration::from_millis(120);
const MODEL_SCALE: f32 = 0.01;

pub struct View3dPlugin;

impl Plugin for View3dPlugin {
    fn build(&self, app: &mut bevy::prelude::App) {
        app.init_asset_loader::<MeshLoader>()
            .add_systems(Startup, setup)
            .add_systems(Update, (load_models, apply_resize, apply_commands));
    }
}

#[derive(Resource)]
pub struct View3d {
    pub texture: egui::TextureId,
    pub size: UVec2,
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
        if wanted != self.size {
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
        self.commands.push_back(ViewCommand::Pick(uv));
    }

    pub fn take_picked(&mut self) -> Option<RefU64> {
        self.picked.take()
    }
}

enum ViewCommand {
    Camera(CameraGesture),
    Model(ModelAction),
    Pick([f32; 2]),
}

#[derive(Component)]
struct ViewCamera;

#[derive(Component)]
struct SceneRoot;

#[derive(Component, Clone, Copy)]
struct ModelRoot(RefU64);

#[derive(Component, Clone, Copy)]
struct ModelMesh(RefU64);

#[derive(Resource)]
struct OrbitCamera {
    focus: Vec3,
    motion: Option<CameraMotion>,
}

impl Default for OrbitCamera {
    fn default() -> Self {
        Self {
            focus: Vec3::ZERO,
            motion: None,
        }
    }
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
) {
    let image = images.add(viewport_image(INITIAL_SIZE));
    let texture = egui_textures.add_image(image.clone());
    commands.insert_resource(View3d {
        texture,
        size: INITIAL_SIZE,
        image: image.clone(),
        commands: VecDeque::new(),
        pending_models: None,
        pending_resize: None,
        picked: None,
        bounds: HashMap::new(),
    });
    commands.insert_resource(OrbitCamera::default());
    commands.spawn((
        Camera3d::default(),
        Camera {
            target: RenderTarget::Image(image.into()),
            clear_color: ClearColorConfig::Custom(Color::srgb(0.055, 0.065, 0.08)),
            ..default()
        },
        Transform::from_xyz(8.0, 6.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
        ViewCamera,
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 18_000.0,
            shadows_enabled: false,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.8, -0.7, 0.0)),
    ));
}

fn load_models(
    mut commands: Commands,
    mut view: ResMut<View3d>,
    mut orbit: ResMut<OrbitCamera>,
    mut camera: Query<&mut Transform, With<ViewCamera>>,
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
        view.bounds.insert(model.refno.refno(), (min, max));
    }
    let mut all_min = Vec3::splat(f32::INFINITY);
    let mut all_max = Vec3::splat(f32::NEG_INFINITY);
    for &(min, max) in view.bounds.values() {
        all_min = all_min.min(min);
        all_max = all_max.max(max);
    }
    if all_min.is_finite()
        && all_max.is_finite()
        && let Ok(mut camera) = camera.single_mut()
    {
        frame_bounds(all_min, all_max, &mut orbit, &mut camera);
    }
    for entity in &roots {
        commands.entity(entity).despawn();
    }
    let material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.54, 0.68, 0.78),
        metallic: 0.05,
        perceptual_roughness: 0.72,
        ..default()
    });
    commands
        .spawn((scene_transform, Visibility::Visible, SceneRoot))
        .with_children(|scene| {
            for model in models {
                let refno = model.refno.refno();
                scene
                    .spawn((model.world_trans, Visibility::Visible, ModelRoot(refno)))
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
    mut camera: Query<(&Camera, &GlobalTransform, &mut Transform), With<ViewCamera>>,
    mut roots: Query<(&ModelRoot, &mut Visibility)>,
    meshes: Query<&ModelMesh>,
    mut ray_cast: MeshRayCast,
) {
    let Ok((camera_component, camera_global, mut camera_transform)) = camera.single_mut() else {
        return;
    };
    while let Some(command) = view.commands.pop_front() {
        match command {
            ViewCommand::Pick(uv) => {
                view.picked =
                    surface_hit(uv, camera_component, camera_global, &meshes, &mut ray_cast)
                        .map(|(refno, _)| refno);
            }
            ViewCommand::Model(action) => match action {
                ModelAction::SetVisible { refnos, visible } => {
                    for (root, mut visibility) in &mut roots {
                        if refnos.contains(&root.0) {
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
                        frame_bounds(min, max, &mut orbit, &mut camera_transform);
                    }
                }
                ModelAction::FitAll => {
                    let mut min = Vec3::splat(f32::INFINITY);
                    let mut max = Vec3::splat(f32::NEG_INFINITY);
                    for (root, visibility) in &roots {
                        if *visibility == Visibility::Hidden {
                            continue;
                        }
                        if let Some(&(model_min, model_max)) = view.bounds.get(&root.0) {
                            min = min.min(model_min);
                            max = max.max(model_max);
                        }
                    }
                    if min.is_finite() && max.is_finite() {
                        frame_bounds(min, max, &mut orbit, &mut camera_transform);
                    }
                }
                ModelAction::ToggleMeasure => {}
            },
            ViewCommand::Camera(gesture) => match gesture {
                CameraGesture::Begin { motion, anchor } => {
                    orbit.motion = Some(motion);
                    if let Some((_, point)) = surface_hit(
                        anchor,
                        camera_component,
                        camera_global,
                        &meshes,
                        &mut ray_cast,
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
                        &mut ray_cast,
                    ) {
                        orbit.focus = point;
                    }
                    let offset = camera_transform.translation - orbit.focus;
                    camera_transform.translation =
                        orbit.focus + offset * (amount * -0.002).exp().clamp(0.2, 5.0);
                    camera_transform.look_at(orbit.focus, Vec3::Y);
                }
                CameraGesture::End => orbit.motion = None,
            },
        }
    }
}

fn frame_bounds(min: Vec3, max: Vec3, orbit: &mut OrbitCamera, camera: &mut Transform) {
    orbit.focus = (min + max) * 0.5;
    let radius = (max - min).length().max(0.1) * 0.5;
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
    let size = camera.logical_viewport_size()?;
    let ray = camera
        .viewport_to_world(camera_transform, Vec2::new(uv[0] * size.x, uv[1] * size.y))
        .ok()?;
    let is_model = |entity| meshes.contains(entity);
    let settings = MeshRayCastSettings::default()
        .with_visibility(RayCastVisibility::Visible)
        .with_filter(&is_model);
    let (entity, hit) = ray_cast.cast_ray(ray, &settings).first()?;
    Some((meshes.get(*entity).ok()?.0, hit.point))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framing_centers_and_keeps_the_camera_outside_the_model() {
        let mut orbit = OrbitCamera::default();
        let mut camera = Transform::from_xyz(4.0, 3.0, 5.0);
        frame_bounds(
            Vec3::new(-2.0, -1.0, -3.0),
            Vec3::new(2.0, 1.0, 3.0),
            &mut orbit,
            &mut camera,
        );
        assert_eq!(orbit.focus, Vec3::ZERO);
        assert!(camera.translation.length() > Vec3::new(4.0, 2.0, 6.0).length() * 0.5);
        assert!(
            camera
                .forward()
                .dot((orbit.focus - camera.translation).normalize())
                > 0.999
        );
    }
}
