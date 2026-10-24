//! E3D 外观的后处理链（E3D 外观移植计划阶段二）：边线（HLR）与伪阴影（HBAO）。
//!
//! 从计划里那个已验证的 prepass spike 长出来（spike 的 example 已删，它的两段着色器就是这里的正文）：一个
//! [`ViewNode`] 插在 `Core3d` 图的 `EndMainPass → Tonemapping` 之间，读 Bevy prepass 的法线纹理（`SglLookMaterial`
//! 的 prepass 片元把线性深度写进 RGB、参与标记写进 alpha），依次跑 ND → HBAO → 双边模糊×2 → HLR → 去杂 → 合成，
//! 中间纹理由 `TextureCache` 按视图分配。全部片元着色器，不碰 compute（WebGL2 没有）。
//!
//! 参数全部照搬 plant3d-web `sglLookPipeline.ts`（E3D 3.1 sglDx11 逆向），毫米换成世界单位（1 单位 = 100 mm）。
//! 相机上的 [`SglLookPost`] 是开关与参数：PBR 外观时不提取到渲染世界，节点整个不跑；E3D 外观下两样效果
//! 都关也照跑合成——背景渐变与超采样的字节域平均都在合成里做。

use std::f32::consts::FRAC_PI_6;

use bevy::asset::{load_internal_asset, weak_handle};
use bevy::core_pipeline::core_3d::graph::{Core3d, Node3d};
use bevy::core_pipeline::fullscreen_vertex_shader::fullscreen_shader_vertex_state;
use bevy::core_pipeline::prepass::ViewPrepassTextures;
use bevy::ecs::query::QueryItem;
use bevy::prelude::*;
use bevy::render::extract_component::{
    ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin,
    UniformComponentPlugin,
};
use bevy::render::render_graph::{
    NodeRunError, RenderGraphApp, RenderGraphContext, RenderLabel, ViewNode, ViewNodeRunner,
};
use bevy::render::render_resource::binding_types::{texture_2d, uniform_buffer};
use bevy::render::render_resource::*;
use bevy::render::renderer::{RenderAdapter, RenderContext, RenderDevice};
use bevy::render::texture::{CachedTexture, TextureCache};
use bevy::render::view::{ViewTarget, ViewUniform, ViewUniformOffset, ViewUniforms};
use bevy::render::{Render, RenderApp, RenderSystems};

use super::presets::e3d_background;
use crate::MODEL_SCALE;

/// `sgl_look_post.wgsl`，[`SglLookPostPlugin::build`] 里 `load_internal_asset!` 装进来。
pub const SGL_LOOK_POST_SHADER: Handle<Shader> =
    weak_handle!("3c9e1f52-7b0a-4d6e-9a21-5e8f0c4d2b11");

// --- E3D 3.1 sglDx11 硬编码的 HBAO / 双边模糊数值（plant3d-web `E3D31_HBAO`）---------------------------

/// g_R：采样半径，毫米。
pub const E3D31_HBAO_RADIUS_MM: f32 = 392.7327;
/// g_NumDir。
pub const E3D31_HBAO_NUM_DIRS: u32 = 8;
/// g_NumSteps。
pub const E3D31_HBAO_NUM_STEPS: u32 = 4;
/// g_AngleBias（30°）。
pub const E3D31_HBAO_ANGLE_BIAS: f32 = FRAC_PI_6;
/// g_Attenuation。
pub const E3D31_HBAO_ATTENUATION: f32 = 0.2;
/// g_Contrast。
pub const E3D31_HBAO_CONTRAST: f32 = 1.25;
/// g_BlurRadius（像素）；Falloff 由它算：1/(2·((R+1)/2)²)。
pub const E3D31_HBAO_BLUR_RADIUS_PX: f32 = 12.0;
/// g_Sharpness 的分子：锐度 = 16 / (深度范围 / 2)。
pub const E3D31_HBAO_SHARPNESS_NUMERATOR: f32 = 16.0;
/// 算不出深度范围时的兜底锐度（plant3d-web `blurSharpness` 0.01 /mm）。
pub const SGL_BLUR_SHARPNESS_FALLBACK_PER_MM: f32 = 0.01;

// --- sglDx11 HLR PS 的常量（plant3d-web `SglHlrParams` 默认）-----------------------------------------

/// 深度差阈值，毫米。
pub const SGL_HLR_DEPTH_THRESHOLD_MM: f32 = 50.0;
/// 法线夹角阈值：|cos| < 该值判边。
pub const SGL_HLR_NORMAL_THRESHOLD: f32 = 0.6;
/// 深度梯度方向判据：|dot| < 该值→台阶。
pub const SGL_HLR_GRADIENT_DOT_THRESHOLD: f32 = 0.9999;
/// 梯度向量里的像素步长常数 h = gradientStep · InvResolution，毫米。
pub const SGL_HLR_GRADIENT_STEP_MM: f32 = 1000.0;

// --- sglDx11 HLR 去杂级（plant3d-web `E3D31_HLR_DECLUTTER`）--------------------------------------------

/// g_MinColour。
pub const E3D31_HLR_DECLUTTER_MIN: f32 = 0.0;
/// g_MaxColour。
pub const E3D31_HLR_DECLUTTER_MAX: f32 = 0.3;
/// 2×2 窗口每个样本的权重（dxbc 里写死 1/9）。
pub const E3D31_HLR_DECLUTTER_SAMPLE_WEIGHT: f32 = 1.0 / 9.0;

/// 毫米 → 世界单位。
pub const fn mm_to_world(mm: f32) -> f32 {
    mm * MODEL_SCALE
}

/// E3D 3.1 每帧的模糊深度锐度（`Blur_slot4`）：半深度范围 h > 0.001 mm 时 16 / h，否则 `None`（沿用上一帧）。
/// 返回的是本管线口径（权重 exp(−(Δz·s)²)），单位是世界单位的倒数。
pub fn blur_sharpness_for_depth_range(depth_range: f32) -> Option<f32> {
    let half = depth_range * 0.5;
    (half > mm_to_world(0.001)).then(|| E3D31_HBAO_SHARPNESS_NUMERATOR / half)
}

/// 一个世界 AABB 在相机眼空间的线性深度范围：8 个角点的 −z，近端截到 `near`。范围 ≤ 0 返回 `None`。
pub fn eye_depth_range(min: Vec3, max: Vec3, view_from_world: &Mat4, near: f32) -> Option<f32> {
    let mut lo = f32::INFINITY;
    let mut hi = f32::NEG_INFINITY;
    for i in 0..8 {
        let corner = Vec3::new(
            if i & 1 != 0 { max.x } else { min.x },
            if i & 2 != 0 { max.y } else { min.y },
            if i & 4 != 0 { max.z } else { min.z },
        );
        let d = -view_from_world.transform_point3(corner).z;
        lo = lo.min(d);
        hi = hi.max(d);
    }
    let lo = lo.max(near.max(0.0));
    let range = hi - lo;
    (range.is_finite() && range > 0.0).then_some(range)
}

/// 挂在主相机上的后处理开关与参数（也是着色器 uniform，每字段 16 字节）。总在相机上，
/// PBR 外观（[`Self::off`]）时 [`ExtractComponent`] 不提取——渲染世界里没有它，节点就不跑。
#[derive(Component, Debug, Clone, Copy, PartialEq, ShaderType)]
pub struct SglLookPost {
    /// x = 边线开，y = 伪阴影开，z = 合成生效（E3D 外观），w = 超采样倍数（ND 像素 / 屏幕像素；
    /// AO / 模糊 / 去杂跑屏幕分辨率）。
    pub toggles: UVec4,
    /// x = HBAO 半径（世界单位），y = 1/R²，z = 角度偏置，w = 衰减。
    pub ao: Vec4,
    /// x = 对比度，y = 方向数，z = 步数，w = 模糊深度锐度。
    pub ao2: Vec4,
    /// x = 深度差阈值，y = 法线阈值，z = 梯度 |dot| 阈值，w = 梯度步长常数（世界单位）。
    pub hlr: Vec4,
    /// x = 去杂 min，y = max，z = 样本权重，w = 模糊半径（像素）。
    pub declutter: Vec4,
    /// 背景渐变在视口顶 / 底的两色（sRGB 字节 / 255）。合成在字节域里把背景画进离屏图并输出不透明，
    /// egui 就不再参与混合——它在线性空间里混，会把落在背景上的边线压浅，还会对预乘色再乘一次 alpha。
    pub bg_top: Vec4,
    pub bg_bottom: Vec4,
}

impl SglLookPost {
    /// 按外观开关、本帧模糊锐度与 AO 分辨率除数（超采样时 = 2，AO 跑在屏幕分辨率）组一份；
    /// 其余参数全是 E3D 3.1 常量。
    pub fn new(hlr: bool, ao: bool, blur_sharpness: f32, ao_scale: u32) -> Self {
        let radius = mm_to_world(E3D31_HBAO_RADIUS_MM);
        Self {
            toggles: UVec4::new(hlr as u32, ao as u32, 1, ao_scale.max(1)),
            ao: Vec4::new(
                radius,
                1.0 / (radius * radius),
                E3D31_HBAO_ANGLE_BIAS,
                E3D31_HBAO_ATTENUATION,
            ),
            ao2: Vec4::new(
                E3D31_HBAO_CONTRAST,
                E3D31_HBAO_NUM_DIRS as f32,
                E3D31_HBAO_NUM_STEPS as f32,
                blur_sharpness,
            ),
            hlr: Vec4::new(
                mm_to_world(SGL_HLR_DEPTH_THRESHOLD_MM),
                SGL_HLR_NORMAL_THRESHOLD,
                SGL_HLR_GRADIENT_DOT_THRESHOLD,
                mm_to_world(SGL_HLR_GRADIENT_STEP_MM),
            ),
            declutter: Vec4::new(
                E3D31_HLR_DECLUTTER_MIN,
                E3D31_HLR_DECLUTTER_MAX,
                E3D31_HLR_DECLUTTER_SAMPLE_WEIGHT,
                E3D31_HBAO_BLUR_RADIUS_PX,
            ),
            bg_top: Vec4::ZERO,
            bg_bottom: Vec4::ZERO,
        }
        .with_background(e3d_background(false))
    }

    /// 换上视口背景（[`e3d_background`] 给的顶 / 底两色）。
    pub fn with_background(mut self, (top, bottom): (Color, Color)) -> Self {
        self.bg_top = srgb_bytes_vec4(top);
        self.bg_bottom = srgb_bytes_vec4(bottom);
        self
    }

    /// PBR 外观：不提取，节点不跑。
    pub fn off() -> Self {
        let mut post = Self::new(false, false, fallback_blur_sharpness(), 1);
        post.toggles.z = 0;
        post
    }

    /// E3D 外观的合成生效（PBR 外观为 false）。
    pub fn active(&self) -> bool {
        self.toggles.z != 0
    }

    /// AO / 模糊纹理相对 ND 的缩小倍数。
    pub fn ao_scale(&self) -> u32 {
        self.toggles.w.max(1)
    }

    pub fn hlr_on(&self) -> bool {
        self.toggles.x != 0
    }

    pub fn ao_on(&self) -> bool {
        self.toggles.y != 0
    }

    /// legacy：两样效果都关，只跑合成（ND / HBAO / HLR 都不跑）。
    pub fn effects_off(&self) -> bool {
        !self.hlr_on() && !self.ao_on()
    }
}

/// 没有深度范围可算时的锐度（世界单位的倒数）。
pub fn fallback_blur_sharpness() -> f32 {
    SGL_BLUR_SHARPNESS_FALLBACK_PER_MM / MODEL_SCALE
}

/// 颜色 → uniform 里的 sRGB 字节 / 255（合成在字节域里算）。
fn srgb_bytes_vec4(colour: Color) -> Vec4 {
    let srgb = colour.to_srgba();
    Vec4::new(srgb.red, srgb.green, srgb.blue, 1.0)
}

impl ExtractComponent for SglLookPost {
    type QueryData = &'static SglLookPost;
    type QueryFilter = ();
    type Out = SglLookPost;

    fn extract_component(item: QueryItem<'_, Self::QueryData>) -> Option<Self::Out> {
        item.active().then_some(*item)
    }
}

pub struct SglLookPostPlugin;

impl Plugin for SglLookPostPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(
            app,
            SGL_LOOK_POST_SHADER,
            "sgl_look_post.wgsl",
            Shader::from_wgsl
        );
        app.add_plugins((
            ExtractComponentPlugin::<SglLookPost>::default(),
            UniformComponentPlugin::<SglLookPost>::default(),
        ));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .add_systems(
                Render,
                prepare_textures.in_set(RenderSystems::PrepareResources),
            )
            .add_render_graph_node::<ViewNodeRunner<SglLookNode>>(Core3d, SglLookLabel)
            .add_render_graph_edges(
                Core3d,
                (Node3d::EndMainPass, SglLookLabel, Node3d::Tonemapping),
            );
    }

    fn finish(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app.init_resource::<SglLookPipelines>();
    }
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
struct SglLookLabel;

/// 每个视图的中间纹理。`nd` / `hlr` 全分辨率；`ao` / `blur` / `ao_blurred` / `declutter` 按 `ao_scale` 缩小
/// （超采样时 = 屏幕分辨率）。`nd`：rgb = 面法线 × 标记长度、a = 线性深度；其余单通道 0..1。
#[derive(Component)]
struct SglLookTextures {
    nd: CachedTexture,
    ao: CachedTexture,
    blur: CachedTexture,
    ao_blurred: CachedTexture,
    hlr: CachedTexture,
    declutter: CachedTexture,
}

fn prepare_textures(
    mut commands: Commands,
    mut texture_cache: ResMut<TextureCache>,
    render_device: Res<RenderDevice>,
    views: Query<(Entity, &ViewTarget, &SglLookPost)>,
) {
    for (entity, target, post) in &views {
        let size = target.main_texture().size();
        let scale = post.ao_scale();
        let ao_size = Extent3d {
            width: size.width.div_ceil(scale).max(1),
            height: size.height.div_ceil(scale).max(1),
            depth_or_array_layers: 1,
        };
        let mut make = |label: &'static str, format: TextureFormat, size: Extent3d| {
            texture_cache.get(
                &render_device,
                TextureDescriptor {
                    label: Some(label),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format,
                    usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
            )
        };
        let nd = make("sgl look nd", TextureFormat::Rgba16Float, size);
        let ao = make("sgl look ao", TextureFormat::R8Unorm, ao_size);
        let blur = make("sgl look blur", TextureFormat::R8Unorm, ao_size);
        let ao_blurred = make("sgl look ao blurred", TextureFormat::R8Unorm, ao_size);
        let hlr = make("sgl look hlr", TextureFormat::R8Unorm, size);
        let declutter = make("sgl look declutter", TextureFormat::R8Unorm, ao_size);
        commands.entity(entity).insert(SglLookTextures {
            nd,
            ao,
            blur,
            ao_blurred,
            hlr,
            declutter,
        });
    }
}

/// 后处理的 ND 纹理要把 `Rgba16Float` 当颜色附件：原生后端恒可，WebGL2 看扩展（wgpu 在 WebGL2 上按
/// `EXT_color_buffer_float` 判）。不可就是「边线 / 伪阴影在这台设备上不跑」：相机不挂 prepass、这里一条管线都不排。
pub(crate) fn float_targets_supported(adapter: &RenderAdapter) -> bool {
    adapter
        .get_texture_format_features(TextureFormat::Rgba16Float)
        .allowed_usages
        .contains(TextureUsages::RENDER_ATTACHMENT)
}

/// 六段全屏管线的编号。设备不支持半浮点颜色附件时整组都不排：`PipelineCache` 是排了就编的，ND 那条会在
/// 创建时被 wgpu 校验打回（“Format Rgba16Float is not renderable”），Bevy 的错误处理把它当 panic——阶段四模拟缺扩展时踩到。
struct SglLookPipelineIds {
    nd: CachedRenderPipelineId,
    ao: CachedRenderPipelineId,
    blur_x: CachedRenderPipelineId,
    blur_y: CachedRenderPipelineId,
    hlr: CachedRenderPipelineId,
    declutter: CachedRenderPipelineId,
    composite: CachedRenderPipelineId,
}

#[derive(Resource)]
struct SglLookPipelines {
    nd_layout: BindGroupLayout,
    ao_layout: BindGroupLayout,
    blur_layout: BindGroupLayout,
    hlr_layout: BindGroupLayout,
    declutter_layout: BindGroupLayout,
    composite_layout: BindGroupLayout,
    /// `None` = 这台设备跑不了（见 [`float_targets_supported`]），节点不跑。
    queued: Option<SglLookPipelineIds>,
}

fn plain_texture() -> BindGroupLayoutEntryBuilder {
    texture_2d(TextureSampleType::Float { filterable: false })
}

fn queue_pipeline(
    cache: &PipelineCache,
    label: &'static str,
    layout: &BindGroupLayout,
    defs: &[&str],
    format: TextureFormat,
) -> CachedRenderPipelineId {
    cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some(label.into()),
        layout: vec![layout.clone()],
        vertex: fullscreen_shader_vertex_state(),
        fragment: Some(FragmentState {
            shader: SGL_LOOK_POST_SHADER,
            shader_defs: defs.iter().map(|def| (*def).into()).collect(),
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
    })
}

impl FromWorld for SglLookPipelines {
    fn from_world(world: &mut World) -> Self {
        let device = world.resource::<RenderDevice>();
        let nd_layout = device.create_bind_group_layout(
            "sgl look nd layout",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (uniform_buffer::<ViewUniform>(true), plain_texture()),
            ),
        );
        let ao_layout = device.create_bind_group_layout(
            "sgl look ao layout",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    uniform_buffer::<SglLookPost>(true),
                    uniform_buffer::<ViewUniform>(true),
                    plain_texture(),
                ),
            ),
        );
        let blur_layout = device.create_bind_group_layout(
            "sgl look blur layout",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    uniform_buffer::<SglLookPost>(true),
                    plain_texture(),
                    plain_texture(),
                ),
            ),
        );
        let hlr_layout = device.create_bind_group_layout(
            "sgl look hlr layout",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (uniform_buffer::<SglLookPost>(true), plain_texture()),
            ),
        );
        let declutter_layout = device.create_bind_group_layout(
            "sgl look declutter layout",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (uniform_buffer::<SglLookPost>(true), plain_texture()),
            ),
        );
        let composite_layout = device.create_bind_group_layout(
            "sgl look composite layout",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    uniform_buffer::<SglLookPost>(true),
                    plain_texture(),
                    plain_texture(),
                    plain_texture(),
                ),
            ),
        );

        if !float_targets_supported(world.resource::<RenderAdapter>()) {
            return Self {
                nd_layout,
                ao_layout,
                blur_layout,
                hlr_layout,
                declutter_layout,
                composite_layout,
                queued: None,
            };
        }
        let cache = world.resource::<PipelineCache>();
        let nd = queue_pipeline(
            cache,
            "sgl look nd",
            &nd_layout,
            &["SGL_ND"],
            TextureFormat::Rgba16Float,
        );
        let ao = queue_pipeline(
            cache,
            "sgl look ao",
            &ao_layout,
            &["SGL_AO"],
            TextureFormat::R8Unorm,
        );
        let blur_x = queue_pipeline(
            cache,
            "sgl look blur x",
            &blur_layout,
            &["SGL_BLUR"],
            TextureFormat::R8Unorm,
        );
        let blur_y = queue_pipeline(
            cache,
            "sgl look blur y",
            &blur_layout,
            &["SGL_BLUR", "SGL_BLUR_Y"],
            TextureFormat::R8Unorm,
        );
        let hlr = queue_pipeline(
            cache,
            "sgl look hlr",
            &hlr_layout,
            &["SGL_HLR"],
            TextureFormat::R8Unorm,
        );
        let declutter = queue_pipeline(
            cache,
            "sgl look declutter",
            &declutter_layout,
            &["SGL_DECLUTTER"],
            TextureFormat::R8Unorm,
        );
        let composite = queue_pipeline(
            cache,
            "sgl look composite",
            &composite_layout,
            &["SGL_COMPOSITE"],
            TextureFormat::bevy_default(),
        );
        Self {
            nd_layout,
            ao_layout,
            blur_layout,
            hlr_layout,
            declutter_layout,
            composite_layout,
            queued: Some(SglLookPipelineIds {
                nd,
                ao,
                blur_x,
                blur_y,
                hlr,
                declutter,
                composite,
            }),
        }
    }
}

/// 一次全屏三角形：绑定组 0 + 动态偏移，画到 `target`。
fn draw_fullscreen(
    render_context: &mut RenderContext,
    label: &'static str,
    pipeline: &RenderPipeline,
    bind_group: &BindGroup,
    offsets: &[u32],
    target: &TextureView,
) {
    let mut pass = render_context.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: target,
            resolve_target: None,
            ops: Operations::default(),
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    pass.set_render_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, offsets);
    pass.draw(0..3, 0..1);
}

#[derive(Default)]
struct SglLookNode;

impl ViewNode for SglLookNode {
    type ViewQuery = (
        &'static ViewTarget,
        &'static ViewPrepassTextures,
        &'static ViewUniformOffset,
        &'static SglLookTextures,
        &'static DynamicUniformIndex<SglLookPost>,
        &'static SglLookPost,
    );

    fn run(
        &self,
        _graph: &mut RenderGraphContext,
        render_context: &mut RenderContext,
        (target, prepass, view_offset, textures, settings_index, settings): QueryItem<
            Self::ViewQuery,
        >,
        world: &World,
    ) -> Result<(), NodeRunError> {
        if !settings.active() {
            return Ok(());
        }
        let pipelines = world.resource::<SglLookPipelines>();
        let Some(ids) = &pipelines.queued else {
            return Ok(());
        };
        let cache = world.resource::<PipelineCache>();
        let (
            Some(nd_pipeline),
            Some(ao_pipeline),
            Some(blur_x_pipeline),
            Some(blur_y_pipeline),
            Some(hlr_pipeline),
            Some(declutter_pipeline),
            Some(composite_pipeline),
        ) = (
            cache.get_render_pipeline(ids.nd),
            cache.get_render_pipeline(ids.ao),
            cache.get_render_pipeline(ids.blur_x),
            cache.get_render_pipeline(ids.blur_y),
            cache.get_render_pipeline(ids.hlr),
            cache.get_render_pipeline(ids.declutter),
            cache.get_render_pipeline(ids.composite),
        )
        else {
            return Ok(());
        };
        let Some(normal) = prepass.normal_view() else {
            return Ok(());
        };
        let Some(view_binding) = world.resource::<ViewUniforms>().uniforms.binding() else {
            return Ok(());
        };
        let Some(settings_binding) = world
            .resource::<ComponentUniforms<SglLookPost>>()
            .uniforms()
            .binding()
        else {
            return Ok(());
        };
        let settings_offset = settings_index.index();
        let device = render_context.render_device().clone();

        // ① ND：prepass 法线纹理（线性深度 + 标记）→ 面法线 × 标记长度、线性深度（两样效果都关时没人读，不跑）。
        // 不读硬件深度纹理：WebGL2 的 GLSL 后端做不了深度纹理的 textureLoad（阶段四 spike 实测）。
        if !settings.effects_off() {
            let nd_bind_group = device.create_bind_group(
                "sgl look nd bind group",
                &pipelines.nd_layout,
                &BindGroupEntries::sequential((view_binding.clone(), normal)),
            );
            draw_fullscreen(
                render_context,
                "sgl look nd",
                nd_pipeline,
                &nd_bind_group,
                &[view_offset.offset],
                &textures.nd.default_view,
            );
        }

        // ② HBAO + 双边模糊 X / Y。
        if settings.ao_on() {
            let ao_bind_group = device.create_bind_group(
                "sgl look ao bind group",
                &pipelines.ao_layout,
                &BindGroupEntries::sequential((
                    settings_binding.clone(),
                    view_binding.clone(),
                    &textures.nd.default_view,
                )),
            );
            draw_fullscreen(
                render_context,
                "sgl look ao",
                ao_pipeline,
                &ao_bind_group,
                &[settings_offset, view_offset.offset],
                &textures.ao.default_view,
            );
            let blur_x_bind_group = device.create_bind_group(
                "sgl look blur x bind group",
                &pipelines.blur_layout,
                &BindGroupEntries::sequential((
                    settings_binding.clone(),
                    &textures.nd.default_view,
                    &textures.ao.default_view,
                )),
            );
            draw_fullscreen(
                render_context,
                "sgl look blur x",
                blur_x_pipeline,
                &blur_x_bind_group,
                &[settings_offset],
                &textures.blur.default_view,
            );
            let blur_y_bind_group = device.create_bind_group(
                "sgl look blur y bind group",
                &pipelines.blur_layout,
                &BindGroupEntries::sequential((
                    settings_binding.clone(),
                    &textures.nd.default_view,
                    &textures.blur.default_view,
                )),
            );
            draw_fullscreen(
                render_context,
                "sgl look blur y",
                blur_y_pipeline,
                &blur_y_bind_group,
                &[settings_offset],
                &textures.ao_blurred.default_view,
            );
        }

        // ③ HLR 边线 + 去杂。
        if settings.hlr_on() {
            let hlr_bind_group = device.create_bind_group(
                "sgl look hlr bind group",
                &pipelines.hlr_layout,
                &BindGroupEntries::sequential((
                    settings_binding.clone(),
                    &textures.nd.default_view,
                )),
            );
            draw_fullscreen(
                render_context,
                "sgl look hlr",
                hlr_pipeline,
                &hlr_bind_group,
                &[settings_offset],
                &textures.hlr.default_view,
            );
            let declutter_bind_group = device.create_bind_group(
                "sgl look declutter bind group",
                &pipelines.declutter_layout,
                &BindGroupEntries::sequential((
                    settings_binding.clone(),
                    &textures.hlr.default_view,
                )),
            );
            draw_fullscreen(
                render_context,
                "sgl look declutter",
                declutter_pipeline,
                &declutter_bind_group,
                &[settings_offset],
                &textures.declutter.default_view,
            );
        }

        // ④ 合成：source → destination，画背景、字节域合成与超采样平均（关着的通道由 toggles 在着色器里忽略，
        // 纹理照绑不读）。
        let post = target.post_process_write();
        let composite_bind_group = device.create_bind_group(
            "sgl look composite bind group",
            &pipelines.composite_layout,
            &BindGroupEntries::sequential((
                settings_binding.clone(),
                post.source,
                &textures.declutter.default_view,
                &textures.ao_blurred.default_view,
            )),
        );
        draw_fullscreen(
            render_context,
            "sgl look composite",
            composite_pipeline,
            &composite_bind_group,
            &[settings_offset],
            post.destination,
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHADER: &str = include_str!("sgl_look_post.wgsl");

    /// E3D 3.1 的常量一字不差，毫米换算按 1 单位 = 100 mm。
    #[test]
    fn e3d31_constants_are_pinned_and_converted_to_world_units() {
        let post = SglLookPost::new(true, true, 2.0, 2);
        assert_eq!(post.toggles, UVec4::new(1, 1, 1, 2));
        assert_eq!(post.ao_scale(), 2);
        assert_eq!(
            SglLookPost::new(true, true, 2.0, 0).ao_scale(),
            1,
            "除数至少 1"
        );
        assert!(
            (post.ao.x - 3.927327).abs() < 1e-6,
            "R 392.7327 mm = 3.927327 单位"
        );
        assert!((post.ao.y - 1.0 / (3.927327f32 * 3.927327)).abs() < 1e-6);
        assert!((post.ao.z - FRAC_PI_6).abs() < 1e-7);
        assert_eq!(post.ao.w, 0.2);
        assert_eq!(post.ao2, Vec4::new(1.25, 8.0, 4.0, 2.0));
        assert!((post.hlr.x - 0.5).abs() < 1e-6, "50 mm");
        assert_eq!(post.hlr.y, 0.6);
        assert_eq!(post.hlr.z, 0.9999);
        assert!((post.hlr.w - 10.0).abs() < 1e-5, "1000 mm");
        assert_eq!(post.declutter.x, 0.0);
        assert_eq!(post.declutter.y, 0.3);
        assert!((post.declutter.z - 1.0 / 9.0).abs() < 1e-7);
        assert_eq!(post.declutter.w, 12.0);
    }

    /// `Blur_slot4`：16 / (范围 / 2)；范围太小不算。PBR 外观不提取；E3D 外观两样效果都关也照跑合成。
    #[test]
    fn blur_sharpness_follows_the_depth_range_and_only_pbr_skips_extraction() {
        assert_eq!(blur_sharpness_for_depth_range(40.0), Some(16.0 / 20.0));
        assert_eq!(blur_sharpness_for_depth_range(0.0), None);
        assert_eq!(fallback_blur_sharpness(), 1.0, "0.01 /mm = 1 /单位");
        let off = SglLookPost::off();
        assert!(!off.active());
        assert_eq!(SglLookPost::extract_component(&off), None);
        let legacy = SglLookPost::new(false, false, 1.0, 2);
        assert!(legacy.effects_off());
        assert!(legacy.active(), "E3D 外观两样都关仍要画背景、做超采样平均");
        assert_eq!(SglLookPost::extract_component(&legacy), Some(legacy));
        assert!(SglLookPost::new(true, false, 1.0, 1).hlr_on());
        assert!(!SglLookPost::new(true, false, 1.0, 1).effects_off());
        assert_eq!(off.ao_scale(), 1);
    }

    /// 背景两色按 sRGB 字节进 uniform：出厂渐变顶 159、底 242，缺省是纯 grey 130。
    #[test]
    fn background_colours_are_uploaded_as_srgb_bytes() {
        let bytes = |v: Vec4| [v.x, v.y, v.z].map(|c| (c * 255.0).round() as u8);
        let post = SglLookPost::new(true, true, 1.0, 2).with_background(e3d_background(true));
        assert_eq!(bytes(post.bg_top), [159, 159, 159]);
        assert_eq!(bytes(post.bg_bottom), [242, 242, 242]);
        let flat = SglLookPost::new(true, true, 1.0, 1);
        assert_eq!(bytes(flat.bg_top), [130, 130, 130]);
        assert_eq!(flat.bg_top, flat.bg_bottom);
    }

    /// 眼空间深度范围：相机在 z = +10 看向原点，盒子 [-1, 1]³ 的深度是 9..11；近端截到 near。
    #[test]
    fn eye_depth_range_spans_the_box_corners_and_clamps_to_near() {
        let camera = Transform::from_xyz(0.0, 0.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y);
        let view_from_world = camera.compute_matrix().inverse();
        let range =
            eye_depth_range(Vec3::splat(-1.0), Vec3::splat(1.0), &view_from_world, 0.1).unwrap();
        assert!((range - 2.0).abs() < 1e-4, "{range}");
        let clamped =
            eye_depth_range(Vec3::splat(-1.0), Vec3::splat(1.0), &view_from_world, 10.0).unwrap();
        assert!((clamped - 1.0).abs() < 1e-4, "{clamped}");
        assert_eq!(
            eye_depth_range(Vec3::ZERO, Vec3::ZERO, &view_from_world, 0.1),
            None
        );
    }

    /// 着色器文本钉住几个不该漂的判据（照 `invalid_tubi_shader_uses_bevys_material_bind_group` 的写法）。
    #[test]
    fn post_shader_pins_the_sgl_rules() {
        assert!(
            SHADER.contains("return depth <= 0.0;"),
            "背景判据 = 线性深度 0"
        );
        assert!(
            SHADER.contains("floor(length(n) * 4.0 + 0.5)) - 1"),
            "标记解码"
        );
        assert!(
            SHADER.contains("nd_at(px + vec2(step, 0), size)")
                && SHADER.contains("nd_at(px + vec2(0, step), size)"),
            "HLR 只看右 / 下邻居，距 1 个屏幕像素"
        );
        assert!(
            SHADER.contains("return vec4(srgb_to_linear(bytes), 1.0);"),
            "合成输出不透明，背景在这里画"
        );
        assert!(
            SHADER.contains("mix(bg, linear_to_srgb(clamp(straight, vec3(0.0), vec3(1.0))), a)"),
            "半透明在字节域里叠到背景上"
        );
        assert!(SHADER.contains("let bytes = col * hlr * ao;"), "字节域相乘");
        assert!(
            SHADER.contains("col /= f32(s * s);"),
            "超采样在字节域取平均"
        );
        assert!(
            SHADER.contains("edge_at(px + vec2(-1, -1), screen, s)"),
            "去杂 2×2 窗口含左上，跑在屏幕分辨率上"
        );
        assert!(
            SHADER.contains("return acc / f32(s * s);"),
            "去杂的每个样本先对子像素取平均"
        );
        assert!(
            SHADER.contains(
                "abs(2.0 * affine_depth(dl) - affine_depth(depth_at(px + vec2(-2, 0), size)) - a)"
            ),
            "ND 按外推误差选差分的一侧（透视在 1/深度域里比）"
        );
        assert!(
            !SHADER.contains("texture_depth_2d")
                && SHADER.contains("decode_linear_depth(textureLoad(normal_tex,"),
            "ND 只读法线纹理里的线性深度：WebGL2 的 GLSL 后端做不了深度纹理的 textureLoad"
        );
        assert!(
            SHADER.contains("const ND_DEPTH_RANGE: f32 = 65504.0;")
                && include_str!("../sgl_look.wgsl")
                    .contains("const ND_DEPTH_RANGE: f32 = 65504.0;"),
            "编码 / 解码两头的深度范围要一致"
        );
        for def in [
            "SGL_ND",
            "SGL_AO",
            "SGL_BLUR",
            "SGL_BLUR_Y",
            "SGL_HLR",
            "SGL_DECLUTTER",
            "SGL_COMPOSITE",
        ] {
            assert!(SHADER.contains(&format!("#ifdef {def}")), "{def}");
        }
    }

    /// prepass 把线性深度拆成 3 × 10 bit 存进 Rgb10a2Unorm，ND 再拼回来：按两段着色器同样的算式在 f32 里走一遍
    ///（中间过一次 UNORM10 的整数化），误差不超过一个定点步长（65504 / 2³⁰ ≈ 0.006 mm）加浮点舍入；0 原样回 0。
    #[test]
    fn linear_depth_survives_the_three_ten_bit_channels() {
        const RANGE: f32 = 65504.0;
        let encode = |depth: f32| -> [f32; 3] {
            let v = (depth / RANGE).clamp(0.0, 0.99999994);
            let s0 = v * 1024.0;
            let hi = s0.floor();
            let s1 = (s0 - hi) * 1024.0;
            let mid = s1.floor();
            let lo = ((s1 - mid) * 1024.0).floor();
            // 着色器写 k/1023，Rgb10a2Unorm 存成 round(f × 1023) 的 10 bit 整数，读回 u/1023
            [hi, mid, lo].map(|k| ((k / 1023.0) * 1023.0).round() / 1023.0)
        };
        let decode = |rgb: [f32; 3]| -> f32 {
            let q = rgb.map(|c| (c * 1023.0).round());
            (q[0] / 1024.0 + q[1] / 1_048_576.0 + q[2] / 1_073_741_824.0) * RANGE
        };
        let step = RANGE / 1_073_741_824.0;
        for depth in [0.001_f32, 0.37, 1.0, 12.5, 123.456, 2_500.0, 65_000.0] {
            let back = decode(encode(depth));
            assert!(
                (back - depth).abs() <= step + depth * 4.0 * f32::EPSILON,
                "{depth} 编码再解码成了 {back}"
            );
        }
        assert_eq!(
            decode(encode(0.0)),
            0.0,
            "清零的法线纹理解出来是背景（深度 0）"
        );
        assert!(
            decode(encode(1.0e9)) < RANGE,
            "超出范围的深度夹在范围内，不会溢出 f16 的 ND 纹理"
        );
    }
}
