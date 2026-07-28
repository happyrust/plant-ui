//! Simple example demonstrating the use of the [`Readback`] component to read back data from the GPU
//! using both a storage buffer and texture.

use bevy::{
    prelude::*,
    render::{
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        gpu_readback::{Readback, ReadbackComplete},
        render_asset::{RenderAssetUsages, RenderAssets},
        render_graph::{self, Node, NodeRunError, RenderGraphContext, RenderLabel},
        render_resource::{
            binding_types::{storage_buffer, texture_storage_2d},
            *,
        },
        renderer::{RenderContext, RenderDevice},
        storage::{GpuShaderStorageBuffer, ShaderStorageBuffer},
        texture::GpuImage,
        Render, RenderApp, RenderSystems, ExtractSchedule,
    },
};
use bevy_render::{Extract, RenderSet};
use bevy_render::render_graph::RenderGraph;

/// This example uses a shader source file from the assets subdirectory
const SHADER_ASSET_PATH: &str = "shaders/gpu_readback.wgsl";

// The length of the buffer sent to the gpu
const BUFFER_LEN: usize = 16;

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins,
            GpuReadbackPlugin,
            OneTimePipelinePlugin,
            ExtractResourcePlugin::<ReadbackBuffer>::default(),
            ExtractResourcePlugin::<ReadbackImage>::default(),
        ))
        .insert_resource(ClearColor(Color::BLACK))
        .add_systems(Startup, setup)
        .run();
}

// We need a plugin to organize all the systems and render node required for this example
struct GpuReadbackPlugin;
impl Plugin for GpuReadbackPlugin {
    fn build(&self, _app: &mut App) {}

    fn finish(&self, app: &mut App) {
        let render_app = app.sub_app_mut(RenderApp);
        render_app.init_resource::<ComputePipeline>().add_systems(
            Render,
            prepare_bind_group
                .in_set(RenderSystems::PrepareBindGroups)
                // We don't need to recreate the bind group every frame
                .run_if(not(resource_exists::<GpuBufferBindGroup>)),
        );

        // Add the compute node as a top level node to the render graph
        // This means it will only execute once per frame
        render_app
            .world_mut()
            .resource_mut::<RenderGraph>()
            .add_node(ComputeNodeLabel, ComputeNode::default());
    }
}

#[derive(Resource, ExtractResource, Clone)]
struct ReadbackBuffer(Handle<ShaderStorageBuffer>);

#[derive(Resource, ExtractResource, Clone)]
struct ReadbackImage(Handle<Image>);

fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut buffers: ResMut<Assets<ShaderStorageBuffer>>,
) {
    // Create a storage buffer with some data
    let buffer = vec![0u32; BUFFER_LEN];
    let mut buffer = ShaderStorageBuffer::from(buffer);
    // We need to enable the COPY_SRC usage so we can copy the buffer to the cpu
    buffer.buffer_description.usage |= BufferUsages::COPY_SRC;
    let buffer = buffers.add(buffer);

    // Create a storage texture with some data
    let size = Extent3d {
        width: BUFFER_LEN as u32,
        height: 1,
        ..default()
    };
    // We create an uninitialized image since this texture will only be used for getting data out
    // of the compute shader, not getting data in, so there's no reason for it to exist on the CPU
    let mut image = Image::new_uninit(
        size,
        TextureDimension::D2,
        TextureFormat::R32Uint,
        RenderAssetUsages::RENDER_WORLD,
    );
    // We also need to enable the COPY_SRC, as well as STORAGE_BINDING so we can use it in the
    // compute shader
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC | TextureUsages::STORAGE_BINDING;
    let image = images.add(image);

    // Spawn the readback components. For each frame, the data will be read back from the GPU
    // asynchronously and trigger the `ReadbackComplete` event on this entity. Despawn the entity
    // to stop reading back the data.
    commands.spawn(Readback::buffer(buffer.clone())).observe(
        |trigger: Trigger<ReadbackComplete>| {
            // This matches the type which was used to create the `ShaderStorageBuffer` above,
            // and is a convenient way to interpret the data.
            let data: Vec<u32> = trigger.event().to_shader_type();
            info!("Buffer {:?}", data);
        },
    );
    // This is just a simple way to pass the buffer handle to the render app for our compute node
    commands.insert_resource(ReadbackBuffer(buffer));

    // Textures can also be read back from the GPU. Pay careful attention to the format of the
    // texture, as it will affect how the data is interpreted.
    commands.spawn(Readback::texture(image.clone())).observe(
        |trigger: Trigger<ReadbackComplete>| {
            // You probably want to interpret the data as a color rather than a `ShaderType`,
            // but in this case we know the data is a single channel storage texture, so we can
            // interpret it as a `Vec<u32>`
            let data: Vec<u32> = trigger.event().to_shader_type();
            info!("Image {:?}", data);
        },
    );
    commands.insert_resource(ReadbackImage(image));
}

#[derive(Resource)]
struct GpuBufferBindGroup(BindGroup);

fn prepare_bind_group(
    mut commands: Commands,
    pipeline: Res<ComputePipeline>,
    render_device: Res<RenderDevice>,
    buffer: Res<ReadbackBuffer>,
    image: Res<ReadbackImage>,
    buffers: Res<RenderAssets<GpuShaderStorageBuffer>>,
    images: Res<RenderAssets<GpuImage>>,
) {
    let buffer = buffers.get(&buffer.0).unwrap();
    let image = images.get(&image.0).unwrap();
    let bind_group = render_device.create_bind_group(
        None,
        &pipeline.layout,
        &BindGroupEntries::sequential((
            buffer.buffer.as_entire_buffer_binding(),
            image.texture_view.into_binding(),
        )),
    );
    commands.insert_resource(GpuBufferBindGroup(bind_group));
}

#[derive(Resource)]
struct ComputePipeline {
    layout: BindGroupLayout,
    pipeline: CachedComputePipelineId,
}

impl FromWorld for ComputePipeline {
    fn from_world(world: &mut World) -> Self {
        let render_device = world.resource::<RenderDevice>();
        let layout = render_device.create_bind_group_layout(
            None,
            &BindGroupLayoutEntries::sequential(
                ShaderStages::COMPUTE,
                (
                    storage_buffer::<Vec<u32>>(false),
                    texture_storage_2d(TextureFormat::R32Uint, StorageTextureAccess::WriteOnly),
                ),
            ),
        );
        let shader = world.load_asset(SHADER_ASSET_PATH);
        let pipeline_cache = world.resource::<PipelineCache>();
        let pipeline = pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some("GPU readback compute shader".into()),
            layout: vec![layout.clone()],
            push_constant_ranges: Vec::new(),
            shader: shader.clone(),
            shader_defs: Vec::new(),
            entry_point: "main".into(),
            zero_initialize_workgroup_memory: false,
        });
        ComputePipeline { layout, pipeline }
    }
}

/// Label to identify the node in the render graph
#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
struct ComputeNodeLabel;

/// The node that will execute the compute shader
#[derive(Default)]
struct ComputeNode {}
impl render_graph::Node for ComputeNode {
    fn run(
        &self,
        _graph: &mut render_graph::RenderGraphContext,
        render_context: &mut RenderContext,
        world: &World,
    ) -> Result<(), render_graph::NodeRunError> {
        let pipeline_cache = world.resource::<PipelineCache>();
        let pipeline = world.resource::<ComputePipeline>();
        let bind_group = world.resource::<GpuBufferBindGroup>();

        if let Some(init_pipeline) = pipeline_cache.get_compute_pipeline(pipeline.pipeline) {
            let mut pass =
                render_context
                    .command_encoder()
                    .begin_compute_pass(&ComputePassDescriptor {
                        label: Some("GPU readback compute pass"),
                        ..default()
                    });

            pass.set_bind_group(0, &bind_group.0, &[]);
            pass.set_pipeline(init_pipeline);
            pass.dispatch_workgroups(BUFFER_LEN as u32, 1, 1);
        }
        Ok(())
    }
}

// 1. 为主世界和渲染世界分别创建资源
#[derive(Resource, Clone, Default, ExtractResource)]  // 添加 ExtractResource 特性
struct MainPipelineState {
    should_execute: bool,
    has_executed: bool,
}

#[derive(Resource, Default)]
struct RenderPipelineState {
    should_execute: bool,
    has_executed: bool,
}

// 2. 修改插件实现
pub struct OneTimePipelinePlugin;

// 添加渲染节点标签定义
#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
struct OneTimePipelineNode;

// 添加渲染节点结构体
#[derive(Default)]
struct OneTimeRenderNode {}

impl Plugin for OneTimePipelinePlugin {
    fn build(&self, app: &mut App) {
        // 在主世界初始化
        app.init_resource::<MainPipelineState>()
           .add_systems(Update, check_keyboard_input);
    }

    fn finish(&self, app: &mut App) {
        let render_app = app.sub_app_mut(RenderApp);

        // 在渲染世界初始化，添加提取插件
        render_app
            .init_resource::<RenderPipelineState>()
            .add_systems(ExtractSchedule, extract_pipeline_state)  // 添加提取系统
            .add_systems(Render, update_render_state.in_set(RenderSet::Queue))  // 重命名系统以避免混淆
            .add_systems(Render, mark_as_executed.in_set(RenderSet::Cleanup));

        // 添加渲染节点
        render_app
            .world_mut()
            .resource_mut::<RenderGraph>()
            .add_node(OneTimePipelineNode, OneTimeRenderNode::default());
    }
}

// 3. 修改渲染节点实现
impl Node for OneTimeRenderNode {
    fn run(
        &self,
        _graph: &mut RenderGraphContext,
        render_context: &mut RenderContext,
        world: &World,
    ) -> Result<(), NodeRunError> {
        let state = world.resource::<RenderPipelineState>();

        if state.should_execute && !state.has_executed {
            // 执行一次性渲染逻辑
            info!("执行一次性渲染管线！");

            // 渲染代码...
        }

        Ok(())
    }
}

// 4. 监听键盘触发渲染
fn check_keyboard_input(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<MainPipelineState>,
) {
    if keyboard_input.just_pressed(KeyCode::Space) {
        info!("触发一次性渲染管线！");
        state.should_execute = true;
        state.has_executed = false;
    }
}

// 5. 添加提取系统 - 从主世界提取到渲染世界
fn extract_pipeline_state(
    mut commands: Commands,
    main_state: Extract<Res<MainPipelineState>>,
) {
    // 提取主世界资源到渲染世界
    commands.insert_resource(main_state.clone());
}

// 6. 更新渲染状态 - 使用提取的主世界状态更新渲染世界状态
fn update_render_state(
    main_state: Res<MainPipelineState>,  // 现在这是渲染世界中的提取资源
    mut render_state: ResMut<RenderPipelineState>,
) {
    // 从提取的主世界状态更新渲染世界状态
    render_state.should_execute = main_state.should_execute;
    render_state.has_executed = main_state.has_executed;
}

// 7. 标记渲染已执行
fn mark_as_executed(
    mut render_state: ResMut<RenderPipelineState>,
    mut commands: Commands,
    mut main_state: ResMut<MainPipelineState>,  // 现在这是渲染世界中的提取资源
) {
    if render_state.should_execute && !render_state.has_executed {
        render_state.has_executed = true;

        // 使用命令队列修改主世界资源，避免直接访问冲突
        // 注意：这只会更新渲染世界中的提取资源副本，而不是主世界中的原始资源
        // 在下一帧，主世界资源会再次被提取，所以需要在主世界中也更新状态
        // commands.insert_resource(MainPipelineState {
        //     should_execute: false,
        //     has_executed: true,
        // });
        // main_state.should_execute = false;
        // main_state.has_executed = true;
    }
}