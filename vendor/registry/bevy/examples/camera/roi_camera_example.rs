//! 演示如何创建区域感兴趣(ROI)相机投影，只关注屏幕的一个特定区域。
//!
//! ROI相机允许在一个小窗口中显示场景的特定区域，同时保持主相机的完整视图。
//! 这在需要同时显示全局和细节视图的应用中非常有用。

use bevy::core_pipeline::core_3d::Camera3dDepthLoadOp;
use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::prelude::*;
use bevy::render::{
    camera::{CameraProjection, Projection, RenderTarget, SubCameraView},
    render_resource::{
        Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
    },
    view::RenderLayers,
};
use bevy_image::TextureFormatPixelInfo;
use std::f32::consts::PI;

const ROI_SIZE: u32 = 128;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .init_resource::<RoiTexture>()
        .init_resource::<CameraControls>()
        .add_systems(Startup, setup)
        .add_systems(Update, (update_roi_camera_with_mouse, camera_controller))
        .run();
}

// 标记我们的ROI相机
#[derive(Component)]
struct RoiCameraMarker;

// 标记主相机
#[derive(Component)]
struct MainCamera;

// 相机控制设置
#[derive(Resource)]
struct CameraControls {
    // 移动速度
    speed: f32,
    // 旋转速度
    rotation_speed: f32,
    // 缩放速度
    zoom_speed: f32,
}

impl Default for CameraControls {
    fn default() -> Self {
        Self {
            speed: 5.0,
            rotation_speed: 1.0,
            zoom_speed: 1.0,
        }
    }
}

// 存储ROI纹理句柄
#[derive(Resource, Default)]
struct RoiTexture {
    texture_handle: Option<Handle<Image>>,
}

// ROI相机配置
#[derive(Resource)]
struct RoiConfig {
    roi_size: Vec2, // ROI区域大小（像素）
}

impl Default for RoiConfig {
    fn default() -> Self {
        Self {
            roi_size: Vec2::new(ROI_SIZE as f32, ROI_SIZE as f32),
        }
    }
}

// 我们的ROI投影组件
#[derive(Debug, Clone)]
struct RoiProjection {
    // 原始透视投影参数
    perspective: PerspectiveProjection,

    // ROI区域参数（屏幕像素）
    roi_center: Vec2,  // 鼠标位置
    roi_size: Vec2,    // ROI区域大小
    screen_size: Vec2, // 屏幕尺寸

    // 缓存计算出的变换矩阵
    roi_matrix: Mat4,

    // 缓存最终投影矩阵
    cached_projection_matrix: Option<Mat4>,
}

impl Default for RoiProjection {
    fn default() -> Self {
        Self {
            perspective: PerspectiveProjection::default(),
            roi_center: Vec2::ZERO,
            roi_size: Vec2::new(ROI_SIZE as f32, ROI_SIZE as f32),
            screen_size: Vec2::new(800.0, 600.0),
            roi_matrix: Mat4::IDENTITY,
            cached_projection_matrix: None,
        }
    }
}

// 实现CameraProjection trait，让我们的RoiProjection可以作为一个相机投影
impl CameraProjection for RoiProjection {
    fn get_clip_from_view(&self) -> Mat4 {
        if let Some(matrix) = self.cached_projection_matrix {
            matrix
        } else {
            // 首先获取透视投影矩阵
            let perspective_matrix = self.perspective.get_clip_from_view();

            // 应用ROI变换
            self.roi_matrix * perspective_matrix
        }
    }

    // 对子视图的特殊处理
    fn get_clip_from_view_for_sub(&self, sub_view: &SubCameraView) -> Mat4 {
        // 对于子视图，我们可能需要调整ROI
        // 简单实现：将子视图的投影也应用ROI变换
        let perspective_matrix = self.perspective.get_clip_from_view_for_sub(sub_view);
        self.roi_matrix * perspective_matrix
    }

    fn update(&mut self, width: f32, height: f32) {
        // 计算roi区域的左下角（基于中心点和大小）
        let roi_min = self.roi_center - self.roi_size / 2.0;

        // 确保roi不超出屏幕，同时确保clamp函数的max值始终大于min值
        let safe_max_x = (self.screen_size.x - self.roi_size.x).max(0.0);
        let safe_max_y = (self.screen_size.y - self.roi_size.y).max(0.0);

        let roi_min = Vec2::new(
            roi_min.x.clamp(0.0, safe_max_x),
            roi_min.y.clamp(0.0, safe_max_y),
        );

        // 更新基础投影
        self.perspective
            .update(self.screen_size.x, self.screen_size.y);

        // 更新ROI变换矩阵
        self.roi_matrix = create_roi_projection_matrix(roi_min, self.roi_size, self.screen_size);

        // 清除缓存的投影矩阵，下次需要时重新计算
        self.cached_projection_matrix = None;
    }

    fn far(&self) -> f32 {
        self.perspective.far()
    }

    fn get_frustum_corners(&self, z_near: f32, z_far: f32) -> [Vec3A; 8] {
        // 使用基础透视投影的视锥体角点
        self.perspective.get_frustum_corners(z_near, z_far)
    }
}

// 计算ROI变换矩阵 - 核心功能
fn create_roi_projection_matrix(
    roi_min: Vec2,     // 区域左下角（像素）
    roi_size: Vec2,    // 区域大小（像素）
    screen_size: Vec2, // 屏幕尺寸（像素）
) -> Mat4 {
    // 步骤1: 将坐标从屏幕空间映射到 [-1,1] 的 NDC 空间

    // 缩放因子: 屏幕尺寸 -> ROI尺寸
    let scale_factor = Vec2::new(screen_size.x / roi_size.x, screen_size.y / roi_size.y);

    // 步骤2: 平移使ROI区域中心成为新的中心点
    // 计算ROI中心在屏幕空间中的位置
    let roi_center = roi_min + roi_size / 2.0;

    // 将屏幕中心到ROI中心的偏移向量转换为NDC空间
    let center_offset = Vec2::new(
        (screen_size.x / 2.0 - roi_center.x) / (screen_size.x / 2.0),
        -(screen_size.y / 2.0 - roi_center.y) / (screen_size.y / 2.0),
    );

    // 创建最终变换矩阵
    // 首先缩放，然后平移
    let scale_mat = Mat4::from_scale(Vec3::new(scale_factor.x, scale_factor.y, 1.0));
    let translation_mat = Mat4::from_translation(Vec3::new(center_offset.x, center_offset.y, 0.0));

    // 组合变换: 先缩放，后平移
    scale_mat * translation_mat
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut roi_texture: ResMut<RoiTexture>,
    windows: Query<&Window>,
) {
    // 添加场景物体

    // 地面平面
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(15.0, 15.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.3, 0.5, 0.3))),
    ));

    // 中央立方体
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
        MeshMaterial3d(materials.add(Color::srgb_u8(255, 0, 0))),
        Transform::from_xyz(0.0, 0.5, 0.0),
    ));

    // 添加一些围绕中心的物体，便于观察
    let num_objects = 8;
    let radius = 4.0;

    for i in 0..num_objects {
        let angle = (i as f32) * 2.0 * PI / (num_objects as f32);
        let x = radius * angle.cos();
        let z = radius * angle.sin();

        // 交替放置不同形状的物体
        if i % 2 == 0 {
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(0.8, 2.0, 0.8))),
                MeshMaterial3d(materials.add(Color::srgb(0.2, 0.4, 0.8))),
                Transform::from_xyz(x, 1.0, z),
            ));
        } else {
            commands.spawn((
                Mesh3d(meshes.add(Torus::default())),
                MeshMaterial3d(materials.add(Color::srgb(0.8, 0.6, 0.2))),
                Transform::from_xyz(x, 0.5, z).with_rotation(Quat::from_rotation_x(PI / 2.0)),
            ));
        }
    }

    // 添加灯光
    commands.spawn((
        PointLight {
            intensity: 1500.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 4.0),
    ));

    commands.spawn((
        DirectionalLight {
            illuminance: 5000.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(0.0, 10.0, 0.0).looking_at(Vec3::ZERO, Vec3::Z),
    ));

    // 获取窗口尺寸
    let window = windows.single().expect("找不到主窗口");
    let screen_size = Vec2::new(
        window.physical_width() as f32,
        window.physical_height() as f32,
    );

    // 设置ROI配置
    let roi_size = Vec2::new(ROI_SIZE as f32, ROI_SIZE as f32);

    // 添加ROI配置资源
    commands.insert_resource(RoiConfig { roi_size });

    // 创建渲染目标纹理
    let size = Extent3d {
        width: ROI_SIZE,
        height: ROI_SIZE,
        ..default()
    };

    // 为ROI相机创建渲染目标纹理 - 计算正确的数据大小
    let format = TextureFormat::Bgra8UnormSrgb;
    let format_size = format.pixel_size();
    let pixels_size = (size.width * size.height) as usize * format_size;

    let image = Image {
        texture_descriptor: TextureDescriptor {
            label: Some("roi_render_texture"),
            size,
            dimension: TextureDimension::D2,
            format,
            mip_level_count: 1,
            sample_count: 1,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        },
        data: Some(vec![0; pixels_size]), // 使用Some()包装Vec<u8>
        ..default()
    };

    // 将纹理添加到资源中并保存句柄
    let texture_handle = images.add(image);
    roi_texture.texture_handle = Some(texture_handle.clone());

    // 创建自定义ROI投影
    let roi_projection = RoiProjection {
        perspective: PerspectiveProjection {
            aspect_ratio: roi_size.x / roi_size.y,
            ..default()
        },
        roi_center: Vec2::new(screen_size.x / 2.0, screen_size.y / 2.0), // 初始中心点
        roi_size,
        screen_size,
        ..default()
    };

    // 创建UI显示渲染目标纹理 - 使用Node和Image
    commands.spawn((
        // 父级节点，用于定位和边框
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(10.0),
            top: Val::Px(10.0),
            width: Val::Px(ROI_SIZE as f32),
            height: Val::Px(ROI_SIZE as f32),
            border: UiRect::all(Val::Px(2.0)),
            ..default()
        },
        // 显示纹理的背景图像
        ImageNode::new(texture_handle.clone()),
        // 边框颜色
        BorderColor::all(LinearRgba::RED.into()),
        // 背景颜色设为透明
        BackgroundColor(Color::NONE),
    ));

    // 1. 主相机 - 渲染整个屏幕
    let camera_transform = Transform::from_xyz(0.0, 5.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y);

    commands.spawn((
        Camera3d::default(),
        camera_transform.clone(),
        RenderLayers::layer(0),
        // 给主相机设置order为0
        Camera {
            order: 0,
            ..default()
        },
        MainCamera, // 添加标记组件
    ));

    // UI相机
    commands.spawn((
        Camera2d,
        // 给UI相机设置order为2，确保在ROI相机之后渲染
        Camera {
            order: 2,
            ..default()
        },
    ));

    // 创建ROI相机 - 渲染到纹理
    commands.spawn((
        Camera3d::default(),
        Camera {
            target: RenderTarget::Image(texture_handle.clone().into()),
            // 确保在主相机后渲染
            order: 1,
            // 设置viewport为右上角的小窗口
            viewport: Some(bevy::render::camera::Viewport {
                physical_position: UVec2::new(0, 0),
                physical_size: UVec2::new(ROI_SIZE, ROI_SIZE),
                ..default()
            }),
            clear_color: ClearColorConfig::Custom(Color::srgb(0.8, 0.8, 0.8)),
            ..default()
        },
        Projection::custom(roi_projection),
        camera_transform,
        RoiCameraMarker,
        RenderLayers::layer(0),
    ));

    // 添加相机控制说明文本
    commands.spawn((
        // 使用Text::new替代Text::from_section
        Text::new("相机控制: WASD - 移动, Q/E - 上下移动, 鼠标右键拖动 - 旋转, 鼠标滚轮 - 缩放"),
        TextFont {
            font_size: 14.0,
            ..default()
        },
        TextShadow::default(),
        // 设置文本对齐方式
        // TextLayout::left(),
        // 使用Node替代NodeBundle
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(10.0),
            bottom: Val::Px(10.0),
            padding: UiRect::all(Val::Px(5.0)),
            ..default()
        },
        // 添加背景颜色
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
    ));
}

// 使用鼠标位置更新ROI相机
fn update_roi_camera_with_mouse(
    windows: Query<&Window>,
    roi_config: Res<RoiConfig>,
    mut roi_cameras: Query<(&mut Projection, &mut Camera), With<RoiCameraMarker>>,
) {
    let window = windows.single().expect("找不到主窗口");

    // 获取鼠标位置
    if let Some(mouse_position) = window.physical_cursor_position() {
        let screen_size = Vec2::new(
            window.physical_width() as f32,
            window.physical_height() as f32,
        );

        for (mut projection, mut camera) in roi_cameras.iter_mut() {
            if let Projection::Custom(custom_proj) = &mut *projection {
                //let perspective = custom_proj.get::<RoiProjection>().unwrap();
                if let Some(roi_proj_mut) = custom_proj.get_mut::<RoiProjection>() {
                    // 更新ROI中心为鼠标位置
                    roi_proj_mut.roi_center = mouse_position;
                    roi_proj_mut.screen_size = screen_size;
                    // dbg!(&roi_proj_mut.roi_center);

                    // 确保ROI大小保持为配置的值
                    roi_proj_mut.roi_size = roi_config.roi_size;
                    // println!(
                    //     "ROI中心: {:?}, ROI大小: {:?}, 屏幕大小: {:?}",
                    //     roi_proj_mut.roi_center, roi_proj_mut.roi_size, roi_proj_mut.screen_size
                    // );

                    // 更新投影
                }
            }

            // 将viewport位置设置为跟随鼠标位置
            if let Some(viewport) = &mut camera.viewport {
                // 计算viewport的位置，使其以鼠标位置为中心
                let half_size = ROI_SIZE as f32 / 2.0;
                let x = (mouse_position.x - half_size).max(0.0) as u32;
                let y = (mouse_position.y - half_size).max(0.0) as u32;

                // 确保viewport不超出屏幕边界
                let max_x = (window.physical_width() as u32).saturating_sub(ROI_SIZE);
                let max_y = (window.physical_height() as u32).saturating_sub(ROI_SIZE);

                // viewport.physical_position = UVec2::new(
                //     x,
                //     y
                // );

                // viewport.physical_position = UVec2::new(
                //     50,
                //     50
                // );
                // viewport.physical_size = UVec2::new(ROI_SIZE, ROI_SIZE);
            }
        }
    }
}

// 相机控制系统
fn camera_controller(
    time: Res<Time>,
    controls: Res<CameraControls>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut mouse_motion: EventReader<MouseMotion>,
    mut mouse_wheel: EventReader<MouseWheel>,
    mut main_camera: Query<&mut Transform, With<MainCamera>>,
    mut roi_camera: Query<&mut Transform, (With<RoiCameraMarker>, Without<MainCamera>)>,
) {
    let dt = time.delta_secs();
    let mut camera_transform = main_camera.single_mut().unwrap();

    // 前后左右移动
    let mut direction = Vec3::ZERO;
    if keyboard.pressed(KeyCode::KeyW) {
        direction += *camera_transform.forward();
    }
    if keyboard.pressed(KeyCode::KeyS) {
        direction += *camera_transform.back();
    }
    if keyboard.pressed(KeyCode::KeyA) {
        direction += *camera_transform.left();
    }
    if keyboard.pressed(KeyCode::KeyD) {
        direction += *camera_transform.right();
    }

    // 上下移动
    if keyboard.pressed(KeyCode::KeyQ) {
        direction += Vec3::new(0.0, -1.0, 0.0);
    }
    if keyboard.pressed(KeyCode::KeyE) {
        direction += Vec3::new(0.0, 1.0, 0.0);
    }

    // 归一化并应用移动
    if direction != Vec3::ZERO {
        let scaled_movement = direction.normalize() * controls.speed * dt;
        camera_transform.translation += scaled_movement;
    }

    // 鼠标右键旋转
    if mouse.pressed(MouseButton::Right) {
        let mut rotation_delta = Vec2::ZERO;
        for ev in mouse_motion.read() {
            rotation_delta += ev.delta;
        }

        if rotation_delta != Vec2::ZERO {
            // 水平旋转（围绕Y轴）
            let yaw = Quat::from_rotation_y(-rotation_delta.x * controls.rotation_speed * dt);

            // 垂直旋转（围绕X轴）
            let pitch = Quat::from_rotation_x(-rotation_delta.y * controls.rotation_speed * dt);

            // 应用旋转
            camera_transform.rotation = yaw * camera_transform.rotation * pitch;
        }
    }

    // 鼠标滚轮缩放
    let mut scroll = 0.0;
    for ev in mouse_wheel.read() {
        scroll += ev.y;
    }

    if scroll != 0.0 {
        // 向前或向后移动相机来实现缩放
        let forwad = camera_transform.forward();
        camera_transform.translation += forwad * scroll * controls.zoom_speed;
    }

    // 将主相机的变换同步到ROI相机
    if let Ok(mut roi_transform) = roi_camera.single_mut() {
        *roi_transform = *camera_transform;
    }
}
