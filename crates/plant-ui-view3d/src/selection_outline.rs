//! 选中描边：绕着选中件的轮廓画一圈线，被别的构件挡住的那一段也画——plant3d-web `OutlinePass` 的口径
//! （ADR-0032 修订 2026-11-18）。两种外观都有；「选了谁」由它交代，填色只管换颜色。
//!
//! 一台遮罩相机加一个全屏节点，不动模型的材质与管线：
//!
//! - `apply_materials` 给选中件（主选中、其它选中、X-Ray 叠选中）在缺省的 0 层之外多挂一层 [`MASK_LAYER`]，
//!   取消选中摘掉；悬停不描。
//! - 遮罩相机是主相机的子实体（位姿天然一致），只看 [`MASK_LAYER`]，把选中件原样画进一张遮罩图——只用它的
//!   alpha。别的构件不在这一层，所以遮罩是选中件**不被遮挡**的整个剪影。遮罩图按屏幕分辨率的
//!   [`MASK_SUPERSAMPLE`] 倍画，不开多重采样（缘由见 [`spawn_selection_mask`]）。
//! - 主相机的 `Core3d` 图里、E3D 后处理之后加一个全屏节点，分两段：先把遮罩平均成一张屏幕分辨率的覆盖率图
//!   （`selection_mask_resolve.wgsl`，抗锯齿出在这一步），再给离剪影不到 [`SelectionOutline::width_px`]
//!   像素、自己又不在剪影里的像素叠上描边色（`selection_outline.wgsl`）。
//!
//! 没有选中件时遮罩相机关着、节点不提取，不花一点帧时间；遮罩相机还跟着空闲停画的闸门（`render_gate`）一起停。
//! 遮罩用选中件自己的材质画，半透明的选中件在遮罩里也是半透明的，着色器按邻域里的最大 alpha 归一后再判内外。

use bevy::asset::{load_internal_asset, weak_handle};
use bevy::core_pipeline::core_3d::graph::{Core3d, Node3d};
use bevy::core_pipeline::fullscreen_vertex_shader::fullscreen_shader_vertex_state;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::ecs::query::QueryItem;
use bevy::prelude::*;
use bevy::render::RenderApp;
use bevy::render::camera::{ClearColorConfig, RenderTarget};
use bevy::render::extract_component::{
    ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin,
    UniformComponentPlugin,
};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_graph::{
    NodeRunError, RenderGraphApp, RenderGraphContext, RenderLabel, ViewNode, ViewNodeRunner,
};
use bevy::render::render_resource::binding_types::{sampler, texture_2d, uniform_buffer};
use bevy::render::render_resource::*;
use bevy::render::renderer::{RenderContext, RenderDevice};
use bevy::render::texture::GpuImage;
use bevy::render::view::{NoIndirectDrawing, RenderLayers, ViewTarget};

use crate::e3d_look::post::SglLookLabel;
use crate::look_materials::LookMaterials;
use crate::{ModelMesh, View3d, ViewCamera, physical_size};

/// `selection_outline.wgsl`，[`SelectionOutlinePlugin::build`] 里 `load_internal_asset!` 装进来。
pub(crate) const SELECTION_OUTLINE_SHADER: Handle<Shader> =
    weak_handle!("7d2b9c64-3f18-4e0a-b5d7-91c4a6e2f803");
/// `selection_mask_resolve.wgsl`，同上。
const SELECTION_MASK_RESOLVE_SHADER: Handle<Shader> =
    weak_handle!("c41e7a05-8d36-4b9f-a2e1-5f7093bd64a8");

/// 选中件在缺省的 0 层之外多挂的那一层；遮罩相机只看它。
pub(crate) const MASK_LAYER: usize = 1;

/// 遮罩图相对屏幕分辨率的倍数：每个屏幕像素 2×2 个遮罩像素，平均下来就是 4 个采样的覆盖率。
const MASK_SUPERSAMPLE: u32 = 2;
/// 遮罩图的像素数上限。遮罩相机开着时显存里有 4 张这么大的纹理（输出图、两张主纹理、深度，各 4 字节一像素），
/// 1600 万像素是 256 MB。视口大到 2 倍放不下时两个方向等比例缩，最少缩回屏幕分辨率。
const MASK_MAX_PIXELS: f64 = 16.0e6;
/// 覆盖率图的格式：单通道就够。它不是相机的输出（节点自己画），没有 [`mask_image`] 那层 sRGB 限制。
const COVERAGE_FORMAT: TextureFormat = TextureFormat::R8Unorm;

/// 描边宽度，逻辑点（egui 的 point）。乘上屏幕缩放才是像素：150% 缩放的屏上是 3 像素。
const OUTLINE_WIDTH_POINTS: f32 = 2.0;
/// 宽度（屏幕像素）的下限：缩放不到 100% 时也留一条看得清的线。
const OUTLINE_WIDTH_MIN_PX: f32 = 1.5;
/// 宽度的上限：着色器三圈采样还接得住细线的范围，再宽细管子的描边会断成节。
const OUTLINE_WIDTH_MAX_PX: f32 = 6.0;

/// 选中件的渲染层：缺省层（主相机照常画它）加遮罩层。
pub(crate) fn selected_layers() -> RenderLayers {
    RenderLayers::from_layers(&[0, MASK_LAYER])
}

/// 描边宽度（屏幕像素）：[`OUTLINE_WIDTH_POINTS`] 个逻辑点按屏幕缩放换算，夹在上下限之内。
pub(crate) fn outline_width_px(pixels_per_point: f32) -> f32 {
    (OUTLINE_WIDTH_POINTS * pixels_per_point).clamp(OUTLINE_WIDTH_MIN_PX, OUTLINE_WIDTH_MAX_PX)
}

/// 挂在主相机上的描边设置。总在相机上，没有选中件（[`Self::active`] 为 false）时 [`ExtractComponent`]
/// 不提取——渲染世界里没有它，节点就不跑。
#[derive(Component, Debug, Clone, PartialEq)]
pub(crate) struct SelectionOutline {
    /// 遮罩图：选中件的剪影（只用 alpha），遮罩相机的输出。尺寸见 [`mask_size`]，由 [`sync_selection_mask`]
    /// 跟着视口改。
    pub(crate) mask: Handle<Image>,
    /// 覆盖率图：遮罩平均到屏幕分辨率的结果（单通道），描边着色器读的是它。尺寸同样跟着视口改。
    pub(crate) coverage: Handle<Image>,
    /// 描边色（[`crate::look_materials::SelectionPalette::outline`]）。
    pub(crate) colour: Color,
    /// 描边宽度，屏幕像素。
    pub(crate) width_px: f32,
    /// 场景里此刻有没有选中件。
    pub(crate) active: bool,
}

impl SelectionOutline {
    fn uniform(&self) -> SelectionOutlineUniform {
        let srgb = self.colour.to_srgba();
        SelectionOutlineUniform {
            colour: Vec4::new(srgb.red, srgb.green, srgb.blue, srgb.alpha),
            params: Vec4::new(self.width_px, 0.0, 0.0, 0.0),
        }
    }
}

/// 遮罩相机的标记。它不是 [`ViewCamera`]，也不写三维离屏目标：相机控制、拾取、换图、停画那几路都找不到它。
#[derive(Component)]
pub(crate) struct SelectionMaskCamera;

/// 着色器 uniform（`selection_outline.wgsl` 的 `SelectionOutline`）。
#[derive(Component, Debug, Clone, Copy, ShaderType)]
pub(crate) struct SelectionOutlineUniform {
    /// rgb = 描边色（sRGB 字节 / 255，着色器在字节域里叠），a = 强度。
    colour: Vec4,
    /// x = 宽度（覆盖率图的像素 = 屏幕像素），其余留空。
    params: Vec4,
}

/// 渲染世界里这个视图用哪两张图。节点按它在不在来判「这一帧画不画描边」：不提取时它被摘掉，
/// 而 `DynamicUniformIndex` 会留在实体上，不能拿来判。
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct SelectionOutlineMask {
    mask: AssetId<Image>,
    coverage: AssetId<Image>,
}

impl ExtractComponent for SelectionOutline {
    type QueryData = &'static SelectionOutline;
    type QueryFilter = ();
    type Out = (SelectionOutlineUniform, SelectionOutlineMask);

    fn extract_component(item: QueryItem<'_, Self::QueryData>) -> Option<Self::Out> {
        item.active.then(|| {
            (
                item.uniform(),
                SelectionOutlineMask {
                    mask: item.mask.id(),
                    coverage: item.coverage.id(),
                },
            )
        })
    }
}

/// 遮罩图。只用 alpha，但格式得是 sRGB 的：Bevy 给相机的输出附件一律按「格式加 sRGB 后缀」排最后那段 blit 管线
/// （`OutputColorAttachment::new(view, format.add_srgb_suffix())`），线性格式的图没有 sRGB 视图，一画就是
/// 「管线目标与渲染通道不匹配」的校验错误。alpha 不受 sRGB 编码影响。
fn mask_image(size: UVec2) -> Image {
    target_image(
        "plant-ui selection mask",
        size,
        TextureFormat::bevy_default(),
    )
}

fn coverage_image(size: UVec2) -> Image {
    target_image("plant-ui selection coverage", size, COVERAGE_FORMAT)
}

/// 不带初始数据，理由同 `viewport_image`：渲染目标第一帧就被整张画满。
fn target_image(label: &'static str, size: UVec2, format: TextureFormat) -> Image {
    Image {
        texture_descriptor: TextureDescriptor {
            label: Some(label),
            size: image_extent(size),
            dimension: TextureDimension::D2,
            format,
            mip_level_count: 1,
            sample_count: 1,
            usage: TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_DST
                | TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        },
        data: None,
        ..default()
    }
}

fn image_extent(size: UVec2) -> Extent3d {
    Extent3d {
        width: size.x.max(1),
        height: size.y.max(1),
        depth_or_array_layers: 1,
    }
}

/// 主相机此刻画着的那张离屏图的视口尺寸，屏幕像素（换图预热期间是新图的）。两张图的尺寸都只跟它走，
/// 与主相机的超采样倍数无关——描边宽度按屏幕像素算。
fn viewport_size(view: &View3d) -> UVec2 {
    view.prepared_resize
        .as_ref()
        .map_or(view.size, |prepared| prepared.size)
}

/// 覆盖率图该有的尺寸：视口的屏幕分辨率。
fn coverage_size(viewport: UVec2, max_texture_dim: u32) -> UVec2 {
    physical_size(viewport, 1, max_texture_dim)
}

/// 遮罩图该有的尺寸：视口屏幕分辨率的 [`MASK_SUPERSAMPLE`] 倍；超过 [`MASK_MAX_PIXELS`] 时两个方向等比例缩到
/// 预算之内，最少缩回屏幕分辨率。不是整数倍也行：平均那一段按 uv 线性采样，取到的照样是覆盖率。
fn mask_size(viewport: UVec2, max_texture_dim: u32) -> UVec2 {
    let pixels = |size: UVec2| f64::from(size.x) * f64::from(size.y);
    let screen = coverage_size(viewport, max_texture_dim);
    let full = physical_size(viewport, MASK_SUPERSAMPLE, max_texture_dim);
    if pixels(full) <= MASK_MAX_PIXELS {
        return full;
    }
    if pixels(screen) >= MASK_MAX_PIXELS {
        return screen;
    }
    let scale = (MASK_MAX_PIXELS / pixels(screen)).sqrt();
    (screen.as_dvec2() * scale).round().as_uvec2().min(full)
}

/// 建遮罩图与覆盖率图、给主相机挂 [`SelectionOutline`]、把遮罩相机挂成主相机的子实体。`setup` 建完主相机后
/// 调一次；`viewport` 是视口的屏幕像素尺寸。
///
/// 遮罩相机排在主相机之前画（`order: -1`），起步关着，[`gate_selection_mask`] 按「有选中件且主相机在画」开关。
/// 不挂 prepass、不挂后处理。
///
/// 不开多重采样：DX12 上（实测 AMD RX590）多重采样的遮罩相机画出来的图是坏的——头一回选中整张是空的，往后几回
/// 剪影对了却带着成对的坏像素；同一台机器关掉多重采样每回都对，Vulkan 与 WebGL2 开着也对（证据见
/// `docs/evidence/2026-11-18-selection-outline.md`）。遮罩的抗锯齿不押在各家驱动的多重采样解析上：图按
/// [`MASK_SUPERSAMPLE`] 倍画，节点自己平均成覆盖率。
pub(crate) fn spawn_selection_mask(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    main_camera: Entity,
    viewport: UVec2,
    max_texture_dim: u32,
    indirect: bool,
) {
    let mask = images.add(mask_image(mask_size(viewport, max_texture_dim)));
    let coverage = images.add(coverage_image(coverage_size(viewport, max_texture_dim)));
    let mut mask_camera = commands.spawn((
        Camera3d::default(),
        Camera {
            order: -1,
            target: RenderTarget::Image(mask.clone().into()),
            clear_color: ClearColorConfig::Custom(Color::NONE),
            is_active: false,
            ..default()
        },
        Transform::IDENTITY,
        Msaa::Off,
        Tonemapping::None,
        RenderLayers::layer(MASK_LAYER),
        SelectionMaskCamera,
    ));
    // 与主相机同一个逃生口（见 `IndirectDrawing`）。
    if !indirect {
        mask_camera.insert(NoIndirectDrawing);
    }
    let mask_camera = mask_camera.id();
    commands
        .entity(main_camera)
        .insert(SelectionOutline {
            mask,
            coverage,
            colour: Color::NONE,
            width_px: outline_width_px(1.0),
            active: false,
        })
        .add_child(mask_camera);
}

/// 遮罩跟着主相机走：两张图的尺寸跟视口，投影抄主相机的（位姿由父子关系管）。排在 `Update` 链的末尾，
/// `apply_resize` 与改投影的几个系统之后，同一帧就对得上。
///
/// 只在真变了的时候写：改图会发 `AssetEvent::Modified`，每帧都写的话空闲停画的闸门永远停不下来。
pub(crate) fn sync_selection_mask(
    view: Res<View3d>,
    mut images: ResMut<Assets<Image>>,
    main: Query<(&SelectionOutline, Ref<Projection>), With<ViewCamera>>,
    mut mask: Query<&mut Projection, (With<SelectionMaskCamera>, Without<ViewCamera>)>,
) {
    let Ok((outline, projection)) = main.single() else {
        return;
    };
    let viewport = viewport_size(&view);
    for (handle, size) in [
        (&outline.mask, mask_size(viewport, view.max_texture_dim)),
        (
            &outline.coverage,
            coverage_size(viewport, view.max_texture_dim),
        ),
    ] {
        let wanted = image_extent(size);
        if images
            .get(handle)
            .is_some_and(|image| image.texture_descriptor.size != wanted)
            && let Some(image) = images.get_mut(handle)
        {
            image.texture_descriptor.size = wanted;
        }
    }
    if projection.is_changed() {
        for mut mask_projection in &mut mask {
            *mask_projection = projection.clone();
        }
    }
}

/// 每帧把「有没有选中件、描边色、宽度」写到主相机上。排在空闲停画的闸门之前：变了要醒来画一帧。
/// 有没有选中件看的是场景里的实体（挂着遮罩层的模型网格），不看选择集——选了还没装进场景的不算。
pub(crate) fn update_selection_outline(
    view: Res<View3d>,
    look_materials: Res<LookMaterials>,
    selected: Query<(), (With<ModelMesh>, With<RenderLayers>)>,
    mut cameras: Query<&mut SelectionOutline, With<ViewCamera>>,
) {
    let active = !selected.is_empty();
    let colour = look_materials.palette().outline();
    let width_px = outline_width_px(view.pixels_per_point);
    for mut outline in &mut cameras {
        // 只在真变了的时候写：写了就是 `Changed<SelectionOutline>`，闸门会被唤醒。
        if outline.active != active || outline.colour != colour || outline.width_px != width_px {
            outline.active = active;
            outline.colour = colour;
            outline.width_px = width_px;
        }
    }
}

/// 遮罩相机的开关：有选中件、且主相机这一帧要画。排在闸门之后、`CheckVisibility` 之前，理由同闸门：
/// 刚被打开的相机要赶上同一帧的可见性检查。
pub(crate) fn gate_selection_mask(
    main: Query<(&Camera, &SelectionOutline), With<ViewCamera>>,
    mut mask: Query<&mut Camera, (With<SelectionMaskCamera>, Without<ViewCamera>)>,
) {
    let Ok((camera, outline)) = main.single() else {
        return;
    };
    let active = camera.is_active && outline.active;
    for mut mask in &mut mask {
        if mask.is_active != active {
            mask.is_active = active;
        }
    }
}

pub(crate) struct SelectionOutlinePlugin;

impl Plugin for SelectionOutlinePlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(
            app,
            SELECTION_OUTLINE_SHADER,
            "selection_outline.wgsl",
            Shader::from_wgsl
        );
        load_internal_asset!(
            app,
            SELECTION_MASK_RESOLVE_SHADER,
            "selection_mask_resolve.wgsl",
            Shader::from_wgsl
        );
        app.add_plugins((
            ExtractComponentPlugin::<SelectionOutline>::default(),
            UniformComponentPlugin::<SelectionOutlineUniform>::default(),
        ));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        // 排在 E3D 后处理之后：描边不该被边线 / 伪阴影压暗，也不参与它的字节域合成；色调映射之前，FXAA 顺带把它抹匀。
        // PBR 外观下前一个节点不跑，这里就是主通道之后的第一段。
        render_app
            .add_render_graph_node::<ViewNodeRunner<SelectionOutlineNode>>(
                Core3d,
                SelectionOutlineLabel,
            )
            .add_render_graph_edges(
                Core3d,
                (SglLookLabel, SelectionOutlineLabel, Node3d::Tonemapping),
            );
    }

    fn finish(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app.init_resource::<SelectionOutlinePipeline>();
    }
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
struct SelectionOutlineLabel;

#[derive(Resource)]
struct SelectionOutlinePipeline {
    /// 第一段（遮罩 → 覆盖率）。
    resolve_layout: BindGroupLayout,
    resolve_id: CachedRenderPipelineId,
    /// 第二段（描边）。
    outline_layout: BindGroupLayout,
    outline_id: CachedRenderPipelineId,
    /// 两段共用的线性采样器。第一段靠它在屏幕像素的中心一次取到 2×2 个遮罩像素的平均；第二段按 uv 采覆盖率图，
    /// 颜色目标可以是它的整数倍（超采样），描边外沿也靠线性采样渐隐。
    sampler: Sampler,
}

impl FromWorld for SelectionOutlinePipeline {
    fn from_world(world: &mut World) -> Self {
        let device = world.resource::<RenderDevice>();
        // 遮罩（按 uv 采样）、采样器
        let resolve_layout = device.create_bind_group_layout(
            "selection mask resolve layout",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    sampler(SamplerBindingType::Filtering),
                ),
            ),
        );
        // 设置、颜色（按像素读）、覆盖率（按 uv 采样）、采样器
        let outline_layout = device.create_bind_group_layout(
            "selection outline layout",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    uniform_buffer::<SelectionOutlineUniform>(true),
                    texture_2d(TextureSampleType::Float { filterable: false }),
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    sampler(SamplerBindingType::Filtering),
                ),
            ),
        );
        let sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("selection outline sampler"),
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..default()
        });
        let pipeline_cache = world.resource::<PipelineCache>();
        let resolve_id = pipeline_cache.queue_render_pipeline(fullscreen_pipeline(
            "selection mask resolve",
            &resolve_layout,
            SELECTION_MASK_RESOLVE_SHADER,
            COVERAGE_FORMAT,
        ));
        let outline_id = pipeline_cache.queue_render_pipeline(fullscreen_pipeline(
            "selection outline",
            &outline_layout,
            SELECTION_OUTLINE_SHADER,
            TextureFormat::bevy_default(),
        ));
        Self {
            resolve_layout,
            resolve_id,
            outline_layout,
            outline_id,
            sampler,
        }
    }
}

/// 一段全屏三角形的管线：片元入口 `fragment`，整张覆盖目标，不混合。
fn fullscreen_pipeline(
    label: &'static str,
    layout: &BindGroupLayout,
    shader: Handle<Shader>,
    format: TextureFormat,
) -> RenderPipelineDescriptor {
    RenderPipelineDescriptor {
        label: Some(label.into()),
        layout: vec![layout.clone()],
        vertex: fullscreen_shader_vertex_state(),
        fragment: Some(FragmentState {
            shader,
            shader_defs: vec![],
            entry_point: "fragment".into(),
            targets: vec![Some(ColorTargetState {
                format,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
        }),
        primitive: PrimitiveState::default(),
        depth_stencil: None,
        multisample: MultisampleState::default(),
        push_constant_ranges: vec![],
        zero_initialize_workgroup_memory: false,
    }
}

#[derive(Default)]
struct SelectionOutlineNode;

impl ViewNode for SelectionOutlineNode {
    type ViewQuery = (
        &'static ViewTarget,
        &'static DynamicUniformIndex<SelectionOutlineUniform>,
        &'static SelectionOutlineMask,
    );

    fn run(
        &self,
        _graph: &mut RenderGraphContext,
        render_context: &mut RenderContext,
        (target, uniform_index, images): QueryItem<Self::ViewQuery>,
        world: &World,
    ) -> Result<(), NodeRunError> {
        // 两条管线、uniform、两张图的 GPU 纹理都齐了才翻主纹理：`post_process_write` 一调就换了源与目标，
        // 之后不写满目标就会露出上一帧的内容。
        let pipeline = world.resource::<SelectionOutlinePipeline>();
        let pipeline_cache = world.resource::<PipelineCache>();
        let (Some(resolve_pipeline), Some(outline_pipeline)) = (
            pipeline_cache.get_render_pipeline(pipeline.resolve_id),
            pipeline_cache.get_render_pipeline(pipeline.outline_id),
        ) else {
            return Ok(());
        };
        let Some(uniforms) = world
            .resource::<ComponentUniforms<SelectionOutlineUniform>>()
            .uniforms()
            .binding()
        else {
            return Ok(());
        };
        let gpu_images = world.resource::<RenderAssets<GpuImage>>();
        let (Some(mask), Some(coverage)) =
            (gpu_images.get(images.mask), gpu_images.get(images.coverage))
        else {
            return Ok(());
        };
        let post = target.post_process_write();
        let device = render_context.render_device();
        let resolve_bind_group = device.create_bind_group(
            "selection mask resolve bind group",
            &pipeline.resolve_layout,
            &BindGroupEntries::sequential((&mask.texture_view, &pipeline.sampler)),
        );
        let outline_bind_group = device.create_bind_group(
            "selection outline bind group",
            &pipeline.outline_layout,
            &BindGroupEntries::sequential((
                uniforms,
                post.source,
                &coverage.texture_view,
                &pipeline.sampler,
            )),
        );
        // 第一段：遮罩相机这一帧刚画完的遮罩（它排在主相机之前）平均成屏幕分辨率的覆盖率。
        {
            let mut pass = render_context.begin_tracked_render_pass(RenderPassDescriptor {
                label: Some("selection mask resolve"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &coverage.texture_view,
                    resolve_target: None,
                    ops: Operations::default(),
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_render_pipeline(resolve_pipeline);
            pass.set_bind_group(0, &resolve_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        // 第二段：描边叠到主纹理上。
        let mut pass = render_context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("selection outline"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: post.destination,
                resolve_target: None,
                ops: Operations::default(),
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_render_pipeline(outline_pipeline);
        pass.set_bind_group(0, &outline_bind_group, &[uniform_index.index()]);
        pass.draw(0..3, 0..1);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use bevy::asset::AssetPlugin;
    use bevy::transform::{TransformPlugin, TransformSystems};
    use bevy_egui35::egui;

    use super::*;
    use crate::e3d_look::material::SglLookMaterial;
    use crate::look_materials::SelectionPalette;
    use crate::render_gate::{
        PipelinesPending, SETTLE_FRAMES, ViewportRenderGate, gate_viewport_render,
    };
    use crate::{
        DEFAULT_MAX_TEXTURE_DIM, OrbitCamera, PreparedResize, ViewportRenderCamera, blank_view,
    };

    const SHADER: &str = include_str!("selection_outline.wgsl");
    const RESOLVE_SHADER: &str = include_str!("selection_mask_resolve.wgsl");

    /// 主相机 + 遮罩相机 + 空闲停画的闸门，没有渲染器。返回（App，主相机，遮罩相机），已经跑过一帧。
    fn outline_app() -> (App, Entity, Entity) {
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
            .init_resource::<LookMaterials>()
            .add_systems(Update, sync_selection_mask)
            .add_systems(
                PostUpdate,
                (
                    update_selection_outline,
                    gate_viewport_render.after(TransformSystems::Propagate),
                    gate_selection_mask,
                )
                    .chain(),
            );
        let main = app
            .world_mut()
            .spawn((
                Camera::default(),
                Projection::Perspective(PerspectiveProjection {
                    fov: 0.5,
                    ..default()
                }),
                Transform::from_xyz(0.0, 0.0, 10.0),
                ViewCamera,
                ViewportRenderCamera,
            ))
            .id();
        app.add_systems(
            Startup,
            move |mut commands: Commands, mut images: ResMut<Assets<Image>>| {
                spawn_selection_mask(
                    &mut commands,
                    &mut images,
                    main,
                    UVec2::new(16, 16),
                    DEFAULT_MAX_TEXTURE_DIM,
                    true,
                );
            },
        );
        app.update();
        let mut mask_cameras = app
            .world_mut()
            .query_filtered::<Entity, With<SelectionMaskCamera>>();
        let mask = mask_cameras
            .iter(app.world())
            .next()
            .expect("该有一台遮罩相机");
        (app, main, mask)
    }

    fn is_active(app: &App, camera: Entity) -> bool {
        app.world()
            .entity(camera)
            .get::<Camera>()
            .unwrap()
            .is_active
    }

    fn outline_of(app: &App, main: Entity) -> &SelectionOutline {
        app.world().entity(main).get::<SelectionOutline>().unwrap()
    }

    /// （遮罩图，覆盖率图）的尺寸。
    fn image_sizes(app: &App, main: Entity) -> (UVec2, UVec2) {
        let images = app.world().resource::<Assets<Image>>();
        let outline = outline_of(app, main);
        (
            images.get(&outline.mask).unwrap().size(),
            images.get(&outline.coverage).unwrap().size(),
        )
    }

    /// 什么都不动，跑到补帧用完为止。
    fn settle(app: &mut App) {
        for _ in 0..(SETTLE_FRAMES + 3) {
            app.update();
        }
    }

    fn model_mesh() -> ModelMesh {
        ModelMesh {
            refno: aios_core::RefU64(1),
            owner: aios_core::RefU64(1),
            colour: [255; 4],
            line: false,
            room_shell: false,
        }
    }

    /// 遮罩相机是主相机的子实体、只看遮罩层、排在主相机之前画、不开多重采样；投影抄主相机的。遮罩图是视口屏幕
    /// 分辨率的 2 倍、覆盖率图就是屏幕分辨率（换图预热期间跟新图；都与主相机的超采样倍数无关）。
    #[test]
    fn the_mask_camera_rides_on_the_main_camera_and_follows_its_target() {
        let (mut app, main, mask) = outline_app();
        assert!(
            app.world()
                .entity(main)
                .get::<Children>()
                .is_some_and(|children| children.contains(&mask)),
            "遮罩相机该是主相机的子实体，位姿才跟得上"
        );
        let entity = app.world().entity(mask);
        assert_eq!(
            entity.get::<Camera>().unwrap().order,
            -1,
            "排在主相机之前画"
        );
        assert_eq!(
            entity.get::<RenderLayers>(),
            Some(&RenderLayers::layer(MASK_LAYER))
        );
        assert_eq!(
            entity.get::<Msaa>(),
            Some(&Msaa::Off),
            "多重采样的遮罩在 DX12 上画出来是坏的，抗锯齿靠放大了画"
        );
        assert!(
            entity.get::<ViewCamera>().is_none() && entity.get::<ViewportRenderCamera>().is_none(),
            "相机控制、换图、停画都不该找到它"
        );
        let Projection::Perspective(perspective) = entity.get::<Projection>().unwrap() else {
            panic!("投影该抄主相机的透视");
        };
        assert_eq!(perspective.fov, 0.5);
        assert_eq!(
            image_sizes(&app, main),
            (UVec2::new(32, 32), UVec2::new(16, 16))
        );
        let images = app.world().resource::<Assets<Image>>();
        assert!(
            images
                .get(&outline_of(&app, main).mask)
                .unwrap()
                .texture_descriptor
                .format
                .is_srgb(),
            "相机输出附件的 blit 管线按 sRGB 格式排，线性格式的遮罩图一画就校验报错"
        );
        assert_eq!(
            images
                .get(&outline_of(&app, main).coverage)
                .unwrap()
                .texture_descriptor
                .format,
            COVERAGE_FORMAT
        );

        app.world_mut().resource_mut::<View3d>().prepared_resize = Some(PreparedResize {
            size: UVec2::new(64, 32),
            scale: 2,
            image: Handle::default(),
            texture: egui::TextureId::Managed(0),
            generation: 1,
        });
        app.update();
        assert_eq!(
            image_sizes(&app, main),
            (UVec2::new(128, 64), UVec2::new(64, 32)),
            "预热期间主相机已经在画新图，两张图跟新图的视口尺寸"
        );
        {
            let mut view = app.world_mut().resource_mut::<View3d>();
            view.prepared_resize = None;
            view.size = UVec2::new(40, 20);
        }
        app.update();
        assert_eq!(
            image_sizes(&app, main),
            (UVec2::new(80, 40), UVec2::new(40, 20))
        );

        *app.world_mut()
            .entity_mut(main)
            .get_mut::<Projection>()
            .unwrap() = Projection::Orthographic(OrthographicProjection::default_3d());
        app.update();
        assert!(
            matches!(
                app.world().entity(mask).get::<Projection>().unwrap(),
                Projection::Orthographic(_)
            ),
            "主相机换投影，遮罩相机跟着换"
        );
    }

    /// 没有选中件：遮罩相机不画、描边不提取。选中件挂上 / 摘掉遮罩层都要唤醒停画的主相机；
    /// 主相机停画时遮罩相机跟着停，这两路同步本身不能把闸门吵醒。
    #[test]
    fn the_mask_camera_runs_only_while_something_is_selected_and_the_viewport_renders() {
        let (mut app, main, mask) = outline_app();
        assert!(is_active(&app, main), "开机头几帧主相机必须画");
        assert!(!is_active(&app, mask), "没有选中件，遮罩相机不画");
        assert!(!outline_of(&app, main).active);
        assert!(SelectionOutline::extract_component(outline_of(&app, main)).is_none());
        settle(&mut app);
        assert!(
            !is_active(&app, main),
            "静止场景没有停画：选中描边的同步每帧都在写东西"
        );

        let model = app
            .world_mut()
            .spawn((model_mesh(), selected_layers()))
            .id();
        app.update();
        assert!(is_active(&app, main), "选中件挂上遮罩层没有唤醒主相机");
        assert!(is_active(&app, mask), "有选中件且主相机在画，遮罩相机该画");
        let outline = outline_of(&app, main);
        assert!(outline.active);
        assert_eq!(outline.colour, SelectionPalette::Native.outline());
        assert_eq!(outline.width_px, outline_width_px(1.0));
        assert!(SelectionOutline::extract_component(outline).is_some());

        settle(&mut app);
        assert!(!is_active(&app, main), "选中着不动也该停画");
        assert!(!is_active(&app, mask), "主相机停画，遮罩相机跟着停");
        assert!(outline_of(&app, main).active, "停画不等于取消选中");

        // 屏幕缩放变了（窗口拖到另一块屏）：宽度跟着变，醒来重画。
        app.world_mut()
            .resource_mut::<View3d>()
            .set_pixels_per_point(1.5);
        app.update();
        assert!(is_active(&app, main), "描边宽度变了没有唤醒主相机");
        assert_eq!(outline_of(&app, main).width_px, 3.0);

        settle(&mut app);
        app.world_mut().entity_mut(model).remove::<RenderLayers>();
        app.update();
        assert!(is_active(&app, main), "摘掉遮罩层也要醒来把描边擦掉");
        assert!(!is_active(&app, mask));
        assert!(!outline_of(&app, main).active);
    }

    /// 遮罩图是屏幕分辨率的 2 倍；2 倍超了像素预算就等比例缩到预算之内，最少缩回屏幕分辨率；设备的纹理边长上限
    /// 照样夹着。覆盖率图始终是屏幕分辨率。
    #[test]
    fn the_mask_is_twice_the_screen_resolution_within_a_pixel_budget() {
        let viewport = UVec2::new(1600, 900);
        assert_eq!(mask_size(viewport, 8192), UVec2::new(3200, 1800));
        assert_eq!(coverage_size(viewport, 8192), viewport);
        // 3200×1800 的视口：2 倍是 2304 万像素，超了 1600 万的预算，按 √(1600 / 576) = 5/3 倍画
        let mask = mask_size(UVec2::new(3200, 1800), 8192);
        assert_eq!(mask, UVec2::new(5333, 3000));
        assert!(f64::from(mask.x) * f64::from(mask.y) <= MASK_MAX_PIXELS);
        assert_eq!(
            mask_size(UVec2::new(6000, 3000), 8192),
            UVec2::new(6000, 3000),
            "视口自己就超了预算：停在屏幕分辨率"
        );
        assert_eq!(
            mask_size(UVec2::new(3000, 500), 4096),
            UVec2::new(4096, 1000),
            "边长不超过设备的纹理上限"
        );
    }

    /// 宽度按逻辑点定、随屏幕缩放换算成像素，两头有界。
    #[test]
    fn the_outline_width_follows_the_screen_scale() {
        assert_eq!(outline_width_px(1.0), 2.0);
        assert_eq!(outline_width_px(1.5), 3.0);
        assert_eq!(outline_width_px(0.5), 1.5, "再细就看不清了");
        assert_eq!(outline_width_px(4.0), 6.0, "再宽细管子的描边会断成节");
    }

    /// uniform 里颜色是 sRGB 字节 / 255（着色器在字节域里叠）、宽度在 params.x；着色器按同一个布局读。
    /// 两段着色器对得上：第一段把遮罩的 alpha 写进单通道的覆盖率图，第二段读它的 r。
    #[test]
    fn the_uniform_matches_the_shader_layout() {
        let outline = SelectionOutline {
            mask: Handle::default(),
            coverage: Handle::default(),
            colour: Color::srgb_u8(0xff, 0x9a, 0xe8),
            width_px: 3.0,
            active: true,
        };
        let uniform = outline.uniform();
        let expected = Vec4::new(1.0, 154.0 / 255.0, 232.0 / 255.0, 1.0);
        assert!((uniform.colour - expected).abs().max_element() < 1e-6);
        assert_eq!(uniform.params, Vec4::new(3.0, 0.0, 0.0, 0.0));
        for pin in [
            "struct SelectionOutline {",
            "colour: vec4<f32>,",
            "params: vec4<f32>,",
            "let width = outline.params.x;",
            "textureSampleLevel(coverage_tex, coverage_sampler, uv, 0.0).r",
            "outline.colour.rgb * k",
        ] {
            assert!(SHADER.contains(pin), "着色器里找不到：{pin}");
        }
        assert!(
            RESOLVE_SHADER.contains("textureSampleLevel(mask_tex, mask_sampler, in.uv, 0.0).a"),
            "第一段该读遮罩的 alpha"
        );
    }
}
