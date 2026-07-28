//! 演示如何创建区域感兴趣(ROI)相机投影，只关注屏幕的一个特定区域。
//!
//! ROI相机允许在一个小窗口中显示场景的特定区域，同时保持主相机的完整视图。
//! 这在需要同时显示全局和细节视图的应用中非常有用。

use bevy::prelude::*;
use bevy::render::camera::{Camera3d, CameraProjection, Projection, SubCameraView, Viewport};
use std::f32::consts::PI;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_systems(Startup, setup)
        .add_systems(Update, (update_roi_camera, move_roi_camera))
        .run();
}

// 标记我们的ROI相机
#[derive(Component)]
struct RoiCameraMarker;

// 存储ROI区域的移动方向和速度
#[derive(Resource)]
struct RoiSettings {
    movement_direction: Vec2,
    movement_speed: f32,
    bounds_min: Vec2,
    bounds_max: Vec2,
}

impl Default for RoiSettings {
    fn default() -> Self {
        Self {
            movement_direction: Vec2::new(1.0, 0.8).normalize(),
            movement_speed: 100.0,
            bounds_min: Vec2::ZERO,
            bounds_max: Vec2::new(800.0, 600.0),
        }
    }
}

// 我们的ROI投影组件
#[derive(Debug, Clone)]
struct RoiProjection {
    // 原始透视投影参数
    perspective: PerspectiveProjection,
    
    // ROI区域参数（屏幕像素）
    roi_min: Vec2,
    roi_size: Vec2,
    screen_size: Vec2,
    
    // 缓存计算出的变换矩阵
    roi_matrix: Mat4,
    
    // 缓存最终投影矩阵
    cached_projection_matrix: Option<Mat4>,
}

impl Default for RoiProjection {
    fn default() -> Self {
        Self {
            perspective: PerspectiveProjection::default(),
            roi_min: Vec2::ZERO,
            roi_size: Vec2::new(300.0, 200.0),
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
        // 更新屏幕尺寸
        self.screen_size = Vec2::new(width, height);
        
        // 确保roi不超出屏幕
        self.roi_min.x = self.roi_min.x.clamp(0.0, width - 1.0);
        self.roi_min.y = self.roi_min.y.clamp(0.0, height - 1.0);
        self.roi_size.x = self.roi_size.x.min(width - self.roi_min.x);
        self.roi_size.y = self.roi_size.y.min(height - self.roi_min.y);
        
        // 更新基础投影
        self.perspective.update(width, height);
        
        // 更新ROI变换矩阵
        self.roi_matrix = create_roi_projection_matrix(
            self.roi_min,
            self.roi_size,
            self.screen_size,
        );
        
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
    roi_min: Vec2,  // 区域左下角（像素）
    roi_size: Vec2, // 区域大小（像素）
    screen_size: Vec2, // 屏幕尺寸（像素）
) -> Mat4 {
    // 计算缩放因子：屏幕大小/ROI大小
    let scale = Vec2::new(
        screen_size.x / roi_size.x,
        screen_size.y / roi_size.y,
    );
    
    // 计算平移量：-roi_min
    let translation = -roi_min;
    
    // 构建变换矩阵：按照变换顺序：平移->缩放
    let translation_mat = Mat4::from_translation(Vec3::new(translation.x, translation.y, 0.0));
    let scale_mat = Mat4::from_scale(Vec3::new(scale.x, scale.y, 1.0));
    
    // 最终ROI变换矩阵
    scale_mat * translation_mat
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    windows: Query<&Window>,
) {
    // 添加场景物体
    
    // 地面平面
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(15.0, 15.0))),
        MeshMaterial3d(materials.add(Color::rgb(0.3, 0.5, 0.3))),
    ));
    
    // 中央立方体
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
        MeshMaterial3d(materials.add(Color::RED)),
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
                MeshMaterial3d(materials.add(Color::rgb(0.2, 0.4, 0.8))),
                Transform::from_xyz(x, 1.0, z),
            ));
        } else {
            commands.spawn((
                Mesh3d(meshes.add(Torus::default())),
                MeshMaterial3d(materials.add(Color::rgb(0.8, 0.6, 0.2))),
                Transform::from_xyz(x, 0.5, z)
                    .with_rotation(Quat::from_rotation_x(PI / 2.0)),
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
        Transform::from_xyz(0.0, 10.0, 0.0)
            .looking_at(Vec3::ZERO, Vec3::Z),
    ));
    
    // 1. 主相机 - 渲染整个屏幕
    let camera_transform = Transform::from_xyz(0.0, 5.0, 10.0)
        .looking_at(Vec3::ZERO, Vec3::Y);
        
    commands.spawn((
        Camera3d::default(),
        camera_transform.clone(),
    ));
    
    // 获取窗口尺寸
    let window = windows.single();
    let screen_size = Vec2::new(window.width(), window.height());
    
    // 设置ROI参数
    let roi_min = Vec2::new(50.0, 50.0);
    let roi_size = Vec2::new(300.0, 200.0);
    
    // 添加ROI设置资源
    commands.insert_resource(RoiSettings {
        bounds_min: Vec2::ZERO,
        bounds_max: Vec2::new(screen_size.x - roi_size.x, screen_size.y - roi_size.y),
        ..default()
    });
    
    // 创建自定义ROI投影
    let mut roi_projection = RoiProjection {
        perspective: PerspectiveProjection {
            aspect_ratio: roi_size.x / roi_size.y,
            ..default()
        },
        roi_min,
        roi_size,
        screen_size,
        ..default()
    };
    
    // 确保ROI投影已更新
    roi_projection.update(screen_size.x, screen_size.y);
    
    // 创建ROI相机 - 只渲染小区域
    commands.spawn((
        Camera3d::default(),
        Camera {
            // 设置viewport - 只渲染ROI区域
            viewport: Some(Viewport {
                physical_position: UVec2::new(roi_min.x as u32, roi_min.y as u32),
                physical_size: UVec2::new(roi_size.x as u32, roi_size.y as u32),
                ..default()
            }),
            // 确保在主相机后渲染
            order: 1,
            // 添加边框以便于识别ROI区域
            viewport_border_width: 2.0,
            viewport_border_color: Color::BLACK,
            ..default()
        },
        Projection::custom(roi_projection),
        camera_transform,
        RoiCameraMarker,
    ));
    
    // 添加使用说明UI
    commands.spawn(
        TextBundle::from_section(
            "区域感兴趣(ROI)相机示例\nROI窗口会自动在屏幕上移动",
            TextStyle {
                font_size: 18.0,
                color: Color::WHITE,
                ..default()
            },
        )
        .with_style(Style {
            position_type: PositionType::Absolute,
            top: Val::Px(10.0),
            left: Val::Px(10.0),
            ..default()
        }),
    );
}

// 更新ROI相机设置，保持同步
fn update_roi_camera(
    windows: Query<&Window>,
    mut roi_cameras: Query<(&mut Camera, &mut Projection), With<RoiCameraMarker>>,
) {
    let window = windows.single();
    let screen_size = Vec2::new(window.width(), window.height());
    
    for (mut camera, mut projection) in roi_cameras.iter_mut() {
        if let Projection::Custom(custom_proj) = &mut *projection {
            if let Some(roi_proj) = custom_proj.dyn_projection.downcast_mut::<RoiProjection>() {
                // 确保投影矩阵保持更新
                roi_proj.update(screen_size.x, screen_size.y);
                
                // 确保viewport也保持同步
                camera.viewport = Some(Viewport {
                    physical_position: UVec2::new(roi_proj.roi_min.x as u32, roi_proj.roi_min.y as u32),
                    physical_size: UVec2::new(roi_proj.roi_size.x as u32, roi_proj.roi_size.y as u32),
                    ..default()
                });
            }
        }
    }
}

// 移动ROI窗口在屏幕上
fn move_roi_camera(
    time: Res<Time>,
    mut settings: ResMut<RoiSettings>,
    mut roi_cameras: Query<&mut Projection, With<RoiCameraMarker>>,
) {
    let delta = time.delta_seconds() * settings.movement_speed;
    
    for mut projection in roi_cameras.iter_mut() {
        if let Projection::Custom(custom_proj) = &mut *projection {
            if let Some(roi_proj) = custom_proj.dyn_projection.downcast_mut::<RoiProjection>() {
                // 计算新的位置
                let mut new_min = roi_proj.roi_min + settings.movement_direction * delta;
                
                // 检查边界，如果到达边界则反弹
                if new_min.x < settings.bounds_min.x || new_min.x > settings.bounds_max.x {
                    settings.movement_direction.x *= -1.0;
                    new_min.x = new_min.x.clamp(settings.bounds_min.x, settings.bounds_max.x);
                }
                
                if new_min.y < settings.bounds_min.y || new_min.y > settings.bounds_max.y {
                    settings.movement_direction.y *= -1.0;
                    new_min.y = new_min.y.clamp(settings.bounds_min.y, settings.bounds_max.y);
                }
                
                // 更新ROI位置
                roi_proj.roi_min = new_min;
            }
        }
    }
} 