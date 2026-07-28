// //! 这个示例展示了如何创建一个自定义的离屏渲染通道，将内容渲染到一个纹理中，
// //! 然后在UI中显示该纹理。这里我们将渲染picking ID结果。
// //!
// //! 这是一个较底层的示例，假设读者对渲染概念和wgpu有一定了解。

// use bevy::{
//     core_pipeline::{
//         core_3d::graph::{Core3d, Node3d},
//         fullscreen_vertex_shader::fullscreen_shader_vertex_state,
//     },
//     ecs::query::QueryItem,
//     prelude::*,
//     render::{
//         render_graph::{
//             NodeRunError, RenderGraphApp, RenderGraphContext, RenderLabel, ViewNode, ViewNodeRunner,
//         },
//         render_resource::*,
//         renderer::{RenderContext, RenderDevice},
//         view::ViewTarget,
//         RenderApp,
//         render_asset::RenderAssetUsages,
//         camera::RenderTarget,
//         render_asset::PreparedRenderAsset,
//         mesh::{GpuBufferInfo, GpuMesh, MeshVertexBufferLayout},
//     },
//     ui::{node_bundles::NodeBundle, widget::ImageBundle, UiImage, Val, Style, PositionType},
// };
// use bevy::render::render_resource::binding_types::{texture_2d, sampler};
// use bevy_render::render_asset::RenderAssets;
// use std::f32::consts::PI;

// /// 着色器路径
// const SHADER_ASSET_PATH: &str = "shaders/custom_pass.wgsl";
// /// Picking ID 着色器路径
// const PICKING_SHADER_PATH: &str = "shaders/picking_id.wgsl";

// /// 形状排列常量
// const SHAPES_X_EXTENT: f32 = 14.0;
// const EXTRUSION_X_EXTENT: f32 = 16.0;
// const Z_EXTENT: f32 = 5.0;

// /// 离屏纹理尺寸
// const OFFSCREEN_TEXTURE_SIZE: u32 = 512;

// /// 标记我们的形状的组件，以便我们可以与地面分开查询它们
// #[derive(Component)]
// struct Shape;

// /// 存储离屏渲染纹理的资源
// #[derive(Resource)]
// struct OffscreenTexture(Handle<Image>);

// /// 用于Picking渲染通道的标签
// #[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
// struct PickingPassLabel;

// fn main() {
//     App::new()
//         .add_plugins(DefaultPlugins.set(ImagePlugin::default_nearest()))
//         .add_plugins(CustomViewportPlugin)
//         .add_systems(Startup, (setup, setup_offscreen_texture, setup_ui).chain())
//         .add_systems(Update, rotate)
//         .run();
// }

// /// 自定义视窗插件
// struct CustomViewportPlugin;

// impl Plugin for CustomViewportPlugin {
//     fn build(&self, app: &mut App) {
//         // 获取渲染应用程序
//         let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
//             return;
//         };

//         render_app
//             // 添加Picking渲染节点
//             .add_render_graph_node::<ViewNodeRunner<PickingPassNode>>(
//                 Core3d,
//                 PickingPassLabel,
//             )
//             // 指定Picking节点顺序
//             .add_render_graph_edges(
//                 Core3d,
//                 (
//                     Node3d::MainOpaquePass,
//                     PickingPassLabel,
//                     Node3d::EndMainPassPostProcessing,
//                 ),
//             );
//     }

//     fn finish(&self, app: &mut App) {
//         // 获取渲染应用程序
//         let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
//             return;
//         };

//         // 初始化渲染管线
//         render_app.init_resource::<CustomPassPipeline>();
//         // 初始化Picking渲染管线
//         render_app.init_resource::<PickingPassPipeline>();
//     }
// }

// // Picking Pass 渲染节点
// #[derive(Default)]
// struct PickingPassNode;

// // 实现ViewNode特性以供ViewNodeRunner使用
// impl ViewNode for PickingPassNode {
//     // 查询需要的数据
//     type ViewQuery = &'static ViewTarget;

//     // 运行节点逻辑
//     fn run(
//         &self,
//         _graph: &mut RenderGraphContext,
//         render_context: &mut RenderContext,
//         view_target: QueryItem<Self::ViewQuery>,
//         world: &World,
//     ) -> Result<(), NodeRunError> {
//         // 获取管线资源
//         let picking_pipeline = world.resource::<PickingPassPipeline>();
//         let pipeline_cache = world.resource::<PipelineCache>();

//         // 从缓存中获取管线
//         let Some(pipeline) = pipeline_cache.get_render_pipeline(picking_pipeline.pipeline_id) else {
//             return Ok(());
//         };

//         // 获取离屏纹理资源
//         let offscreen_texture = match world.get_resource::<OffscreenTexture>() {
//             Some(texture) => texture,
//             None => return Ok(()),
//         };

//         // 获取图像资源
//         let gpu_images = world.resource::<RenderAssets<Image>>();
        
//         // 尝试获取离屏纹理的GPU表示
//         let Some(gpu_image) = gpu_images.get(&offscreen_texture.0) else {
//             return Ok(());
//         };

//         // 开始渲染通道
//         let mut render_pass = render_context.begin_tracked_render_pass(RenderPassDescriptor {
//             label: Some("picking_pass"),
//             color_attachments: &[Some(RenderPassColorAttachment {
//                 view: &gpu_image.texture_view,
//                 resolve_target: None,
//                 ops: Operations {
//                     load: LoadOp::Clear(Color::rgba(0.0, 0.0, 0.0, 1.0).into()),
//                     store: StoreOp::Store,
//                 },
//             })],
//             depth_stencil_attachment: None,
//             timestamp_writes: None,
//             occlusion_query_set: None,
//         });

//         // 设置渲染管线
//         render_pass.set_render_pipeline(pipeline);

//         // 获取网格资源
//         let mesh_assets = world.resource::<RenderAssets<Mesh>>();

//         // 查询所有带有网格和变换的实体
//         let mut mesh_query = world.query::<(&Mesh3d, &GlobalTransform)>();
        
//         // 为每个实体渲染picking ID
//         for (index, (mesh_handle, transform)) in mesh_query.iter(world).enumerate() {
//             // 获取网格的GPU表示
//             let Some(gpu_mesh) = mesh_assets.get(mesh_handle) else {
//                 continue;
//             };

//             // 设置实例绑定组
//             let transform_bytes = transform.affine().to_cols_array_2d().as_std140().to_bytes();
//             let transform_buffer = render_context.render_device().create_buffer(&BufferDescriptor {
//                 label: Some("instance_transform_buffer"),
//                 size: transform_bytes.len() as u64,
//                 usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
//                 mapped_at_creation: true,
//             });
            
//             // 写入变换数据
//             transform_buffer.slice(..).unwrap().get_mapped_range_mut().copy_from_slice(&transform_bytes);
//             transform_buffer.unmap();
            
//             // 创建实例绑定组
//             let instance_bind_group = render_context.render_device().create_bind_group(
//                 "instance_bind_group",
//                 &picking_pipeline.instance_layout,
//                 &BindGroupEntries::sequential((transform_buffer.as_entire_binding(),)),
//             );
            
//             // 设置实例绑定组
//             render_pass.set_bind_group(0, &instance_bind_group, &[]);
            
//             // 设置顶点缓冲区
//             if let Some(vertex_buffer) = &gpu_mesh.vertex_buffer {
//                 render_pass.set_vertex_buffer(0, vertex_buffer.buffer.slice(..));
//             }
            
//             // 设置索引缓冲区
//             if let Some(index_buffer) = &gpu_mesh.index_buffer {
//                 render_pass.set_index_buffer(index_buffer.buffer.slice(..), 0, gpu_mesh.index_format);
//                 // 使用索引绘制
//                 render_pass.draw_indexed(0..gpu_mesh.index_count, 0, 0..1);
//             } else {
//                 // 没有索引时直接绘制顶点
//                 render_pass.draw(0..gpu_mesh.vertex_count, 0..1);
//             }
//         }

//         Ok(())
//     }
// }

// // 包含渲染管线使用的全局数据，在启动时创建一次
// #[derive(Resource)]
// struct CustomPassPipeline {
//     layout: BindGroupLayout,
//     sampler: Sampler,
//     pipeline_id: CachedRenderPipelineId,
// }

// impl FromWorld for CustomPassPipeline {
//     fn from_world(world: &mut World) -> Self {
//         let render_device = world.resource::<RenderDevice>();

//         // 定义绑定组布局
//         let layout = render_device.create_bind_group_layout(
//             "custom_pass_bind_group_layout",
//             &BindGroupLayoutEntries::sequential(
//                 ShaderStages::FRAGMENT,
//                 (
//                     // 屏幕纹理
//                     texture_2d(TextureSampleType::Float { filterable: true }),
//                     // 采样器
//                     sampler(SamplerBindingType::Filtering),
//                 ),
//             ),
//         );

//         // 创建采样器
//         let sampler = render_device.create_sampler(&SamplerDescriptor::default());

//         // 获取着色器句柄
//         let shader = world.load_asset(SHADER_ASSET_PATH);

//         let pipeline_id = world
//             .resource_mut::<PipelineCache>()
//             // 将管线添加到缓存并排队创建
//             .queue_render_pipeline(RenderPipelineDescriptor {
//                 label: Some("custom_pass_pipeline".into()),
//                 layout: vec![layout.clone()],
//                 // 设置全屏三角形的顶点状态
//                 vertex: fullscreen_shader_vertex_state(),
//                 fragment: Some(FragmentState {
//                     shader,
//                     shader_defs: vec![],
//                     // 确保与着色器中的入口点匹配
//                     entry_point: "fs_main".into(),
//                     targets: vec![Some(ColorTargetState {
//                         format: TextureFormat::bevy_default(),
//                         blend: None,
//                         write_mask: ColorWrites::ALL,
//                     })],
//                 }),
//                 primitive: PrimitiveState::default(),
//                 depth_stencil: None,
//                 multisample: MultisampleState::default(),
//                 push_constant_ranges: vec![],
//                 zero_initialize_workgroup_memory: false,
//             });

//         Self {
//             layout,
//             sampler,
//             pipeline_id,
//         }
//     }
// }

// // Picking渲染管线
// #[derive(Resource)]
// struct PickingPassPipeline {
//     instance_layout: BindGroupLayout,
//     pipeline_id: CachedRenderPipelineId,
// }

// impl FromWorld for PickingPassPipeline {
//     fn from_world(world: &mut World) -> Self {
//         let render_device = world.resource::<RenderDevice>();

//         // 定义实例变换绑定组布局
//         let instance_layout = render_device.create_bind_group_layout(
//             "instance_bind_group_layout",
//             &BindGroupLayoutEntries::sequential(
//                 ShaderStages::VERTEX,
//                 (
//                     // 模型变换矩阵
//                     BindingType::Buffer { 
//                         ty: BufferBindingType::Uniform, 
//                         has_dynamic_offset: false, 
//                         min_binding_size: None, 
//                     },
//                 ),
//             ),
//         );

//         // 获取着色器句柄
//         let shader = world.load_asset(PICKING_SHADER_PATH);

//         // 创建一个顶点布局
//         let vertex_layouts = vec![
//             // 位置
//             VertexBufferLayout {
//                 array_stride: 12,
//                 step_mode: VertexStepMode::Vertex,
//                 attributes: vec![VertexAttribute {
//                     format: VertexFormat::Float32x3,
//                     offset: 0,
//                     shader_location: 0,
//                 }],
//             },
//         ];

//         let pipeline_id = world
//             .resource_mut::<PipelineCache>()
//             // 将管线添加到缓存并排队创建
//             .queue_render_pipeline(RenderPipelineDescriptor {
//                 label: Some("picking_pass_pipeline".into()),
//                 layout: vec![instance_layout.clone()],
//                 vertex: VertexState {
//                     shader: shader.clone(),
//                     entry_point: "vertex".into(),
//                     shader_defs: vec![],
//                     buffers: vertex_layouts,
//                 },
//                 fragment: Some(FragmentState {
//                     shader,
//                     shader_defs: vec![],
//                     entry_point: "fragment".into(),
//                     targets: vec![Some(ColorTargetState {
//                         format: TextureFormat::Rgba8UnormSrgb,
//                         blend: None,
//                         write_mask: ColorWrites::ALL,
//                     })],
//                 }),
//                 primitive: PrimitiveState {
//                     topology: PrimitiveTopology::TriangleList,
//                     strip_index_format: None,
//                     front_face: FrontFace::Ccw,
//                     cull_mode: Some(Face::Back),
//                     unclipped_depth: false,
//                     polygon_mode: PolygonMode::Fill,
//                     conservative: false,
//                 },
//                 depth_stencil: None,
//                 multisample: MultisampleState::default(),
//                 push_constant_ranges: vec![],
//                 zero_initialize_workgroup_memory: false,
//             });

//         Self {
//             instance_layout,
//             pipeline_id,
//         }
//     }
// }

// /// 设置离屏纹理资源
// fn setup_offscreen_texture(
//     mut commands: Commands,
//     mut images: ResMut<Assets<Image>>,
// ) {
//     // 创建用于离屏渲染的纹理
//     let size = Extent3d {
//         width: OFFSCREEN_TEXTURE_SIZE,
//         height: OFFSCREEN_TEXTURE_SIZE,
//         depth_or_array_layers: 1,
//     };
//     let mut image = Image::new_fill(
//         size,
//         TextureDimension::D2,
//         &[0, 0, 0, 255],
//         TextureFormat::Rgba8UnormSrgb,
//         RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
//     );
    
//     // 设置纹理用途，使其可以作为渲染目标和纹理绑定
//     image.texture_descriptor.usage = TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING;
    
//     // 添加图像到资产并保存句柄
//     let image_handle = images.add(image);
//     commands.insert_resource(OffscreenTexture(image_handle));
// }

// /// 设置UI以显示离屏纹理
// fn setup_ui(
//     mut commands: Commands,
//     offscreen_texture: Res<OffscreenTexture>,
// ) {
//     // 创建UI节点来显示离屏纹理
//     commands
//         .spawn(NodeBundle {
//             style: Style {
//                 position_type: PositionType::Absolute,
//                 right: Val::Px(10.0),
//                 bottom: Val::Px(10.0),
//                 width: Val::Px(OFFSCREEN_TEXTURE_SIZE as f32 / 2.0),
//                 height: Val::Px(OFFSCREEN_TEXTURE_SIZE as f32 / 2.0),
//                 ..default()
//             },
//             ..default()
//         })
//         .with_children(|parent| {
//             // 添加图像组件，使用离屏纹理作为源
//             parent.spawn(ImageBundle {
//                 image: UiImage {
//                     texture: offscreen_texture.0.clone(),
//                     ..default()
//                 },
//                 style: Style {
//                     width: Val::Percent(100.0),
//                     height: Val::Percent(100.0),
//                     ..default()
//                 },
//                 ..default()
//             });
//         });
// }

// /// 创建一个调试UV纹理图案
// fn uv_debug_texture() -> Image {
//     const TEXTURE_SIZE: usize = 8;

//     let mut palette: [u8; 32] = [
//         255, 102, 159, 255, 255, 159, 102, 255, 236, 255, 102, 255, 121, 255, 102, 255, 102, 255,
//         198, 255, 102, 198, 255, 255, 121, 102, 255, 255, 236, 102, 255, 255,
//     ];

//     let mut texture_data = [0; TEXTURE_SIZE * TEXTURE_SIZE * 4];
//     for y in 0..TEXTURE_SIZE {
//         let offset = TEXTURE_SIZE * y * 4;
//         texture_data[offset..(offset + TEXTURE_SIZE * 4)].copy_from_slice(&palette);
//         palette.rotate_right(4);
//     }

//     Image::new_fill(
//         Extent3d {
//             width: TEXTURE_SIZE as u32,
//             height: TEXTURE_SIZE as u32,
//             depth_or_array_layers: 1,
//         },
//         TextureDimension::D2,
//         &texture_data,
//         TextureFormat::Rgba8UnormSrgb,
//         RenderAssetUsages::RENDER_WORLD,
//     )
// }

// /// 旋转形状的系统
// fn rotate(mut query: Query<&mut Transform, With<Shape>>, time: Res<Time>) {
//     for mut transform in &mut query {
//         transform.rotate_y(time.delta_secs() / 2.);
//     }
// }

// /// 创建3D场景
// fn setup(
//     mut commands: Commands,
//     mut meshes: ResMut<Assets<Mesh>>,
//     mut images: ResMut<Assets<Image>>,
//     mut materials: ResMut<Assets<StandardMaterial>>,
//     offscreen_texture: Option<Res<OffscreenTexture>>,
// ) {
//     // 添加主相机
//     commands.spawn((
//         Camera3d::default(),
//         Transform::from_translation(Vec3::new(0.0, 7.0, 14.0)).looking_at(Vec3::new(0., 1., 0.), Vec3::Y),
//     ));
    
//     // 添加离屏渲染相机（如果离屏纹理已创建）
//     if let Some(offscreen) = offscreen_texture {
//         let mut camera = Camera::default();
//         // 创建RenderTarget的正确方式
//         camera.target = RenderTarget::Image(offscreen.0.clone());
//         camera.order = -1; // 确保在主相机之前渲染
        
//         commands.spawn((
//             Camera3d::default(),
//             camera,
//             Transform::from_translation(Vec3::new(0.0, 7.0, 14.0)).looking_at(Vec3::new(0., 1., 0.), Vec3::Y),
//         ));
//     }

//     // 创建调试材质
//     let debug_material = materials.add(StandardMaterial {
//         base_color_texture: Some(images.add(uv_debug_texture())),
//         ..default()
//     });

//     // 为每个形状分配唯一的ID颜色（用于picking ID渲染）
//     let mut picking_materials = Vec::new();
//     for i in 0..16 {
//         // 创建基于ID的颜色
//         let r = ((i & 0xF) as f32) / 15.0;
//         let g = (((i >> 4) & 0xF) as f32) / 15.0;
//         let b = (((i >> 8) & 0xF) as f32) / 15.0;
        
//         picking_materials.push(materials.add(StandardMaterial {
//             base_color: Color::rgb(r, g, b),
//             unlit: true, // 使用非光照材质以保持颜色不变
//             ..default()
//         }));
//     }

//     // 3D形状
//     let shapes = [
//         meshes.add(Cuboid::default()),
//         meshes.add(Tetrahedron::default()),
//         meshes.add(Capsule3d::default()),
//         meshes.add(Torus::default()),
//         meshes.add(Cylinder::default()),
//         meshes.add(Cone::default()),
//         meshes.add(ConicalFrustum::default()),
//         meshes.add(Sphere::default().mesh().ico(5).unwrap()),
//         meshes.add(Sphere::default().mesh().uv(32, 18)),
//     ];

//     // 凸出的2D形状
//     let extrusions = [
//         meshes.add(Extrusion::new(Rectangle::default(), 1.)),
//         meshes.add(Extrusion::new(Capsule2d::default(), 1.)),
//         meshes.add(Extrusion::new(Annulus::default(), 1.)),
//         meshes.add(Extrusion::new(Circle::default(), 1.)),
//         meshes.add(Extrusion::new(Ellipse::default(), 1.)),
//         meshes.add(Extrusion::new(RegularPolygon::default(), 1.)),
//         meshes.add(Extrusion::new(Triangle2d::default(), 1.)),
//     ];

//     // 添加3D形状到场景
//     let num_shapes = shapes.len();
//     for (i, shape) in shapes.into_iter().enumerate() {
//         // 使用唯一的ID材质（用于picking ID）
//         let material = if offscreen_texture.is_some() {
//             picking_materials[i % picking_materials.len()].clone()
//         } else {
//             debug_material.clone()
//         };
        
//         commands.spawn((
//             Mesh3d(shape),
//             MeshMaterial3d(material),
//             Transform::from_xyz(
//                 -SHAPES_X_EXTENT / 2. + i as f32 / (num_shapes - 1) as f32 * SHAPES_X_EXTENT,
//                 2.0,
//                 Z_EXTENT / 2.,
//             )
//             .with_rotation(Quat::from_rotation_x(-PI / 4.)),
//             Shape,
//         ));
//     }

//     // 添加凸出的2D形状到场景
//     let num_extrusions = extrusions.len();
//     for (i, shape) in extrusions.into_iter().enumerate() {
//         // 使用唯一的ID材质（用于picking ID）
//         let material = if offscreen_texture.is_some() {
//             picking_materials[(i + shapes.len()) % picking_materials.len()].clone()
//         } else {
//             debug_material.clone()
//         };
        
//         commands.spawn((
//             Mesh3d(shape),
//             MeshMaterial3d(material),
//             Transform::from_xyz(
//                 -EXTRUSION_X_EXTENT / 2.
//                     + i as f32 / (num_extrusions - 1) as f32 * EXTRUSION_X_EXTENT,
//                 2.0,
//                 -Z_EXTENT / 2.,
//             )
//             .with_rotation(Quat::from_rotation_x(-PI / 4.)),
//             Shape,
//         ));
//     }
    
//     // 添加地面平面
//     commands.spawn((
//         Mesh3d(meshes.add(Plane3d::default().mesh().size(50.0, 50.0).subdivisions(10))),
//         MeshMaterial3d(materials.add(Color::rgb(0.8, 0.8, 0.8))),
//     ));
    
//     // 添加光源
//     commands.spawn((
//         PointLight {
//             shadows_enabled: true,
//             intensity: 10_000_000.,
//             range: 100.0,
//             shadow_depth_bias: 0.2,
//             ..default()
//         },
//         Transform::from_xyz(8.0, 16.0, 8.0),
//     ));
// } 

fn main(){
    
}