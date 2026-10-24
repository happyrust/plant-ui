//! `SglLookMaterial`：复刻 AVEVA E3D（SGL DX11 后端）实体着色的 Bevy `Material`（E3D 外观移植计划阶段一）。
//!
//! 公式与默认值来自 plant3d-web `sglLookMaterial.ts`（对 `sglDx11.dll` 的逆向）：
//!
//! ```text
//! L = normalize(LampDir)   V = normalize(眼 − p)   N = normalize(n)   H = normalize(V + L)
//! rgb = colour · (Ka + Kd · max(N·L, 0))
//!     + Ks · pow(max(N·H, 0), Kse) · max(N·L, 0)      // 白色高光，不乘物体色
//!     + Kr · env(reflect(−V, N)).rgb                  // 环境立方体贴图（sglDx11 内嵌的灰度贴图）
//! a   = colour.a · opacity
//! ```
//!
//! 单盏「头灯」（眼空间 (0,0,1)），无衰减、无阴影、无 gamma / tonemap。颜色按 **sRGB 字节**直算，
//! 着色器输出前做 sRGB→linear 抵消 sRGB 目标的编码，屏幕上的字节才等于 E3D 的字节（lightgrey 在 Ka = 1
//! 下读回 189 而不是 130）。不读 Bevy 的光源——`DirectionalLight` / `AmbientLight` 留给 PBR 外观。
//!
//! prepass 片元把「参与标记」（bit0 边线、bit1 伪阴影）写进法线纹理的 alpha（`Rgb10a2Unorm` 的 2 bit），
//! 阶段二的边线 / 伪阴影通道读它。

use bevy::asset::weak_handle;
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::render::mesh::MeshVertexBufferLayoutRef;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderRef, ShaderType, SpecializedMeshPipelineError,
};

use super::colour_table::rgb_bytes;
use super::presets::{SGL_ENV_GROUND_LUM, SGL_ENV_SKY_LUM, SglLight};

/// `sgl_look.wgsl`，`View3dPlugin::build` 里 `load_internal_asset!` 装进来。
pub const SGL_LOOK_SHADER: Handle<Shader> = weak_handle!("3c9e1f52-7b0a-4d6e-9a21-5e8f0c4d2b10");

/// 参与标记：这块几何要不要进边线 / 伪阴影（sglDx11 法线纹理 .w 的两位）。
pub const EFFECT_EDGES: u32 = 1;
pub const EFFECT_SHADOWS: u32 = 2;
/// 普通构件：边线 + 伪阴影都参与（E3D 默认）。
pub const EFFECT_ALL: u32 = EFFECT_EDGES | EFFECT_SHADOWS;

/// 着色器 uniform，每个字段都是 16 字节（WebGL2 对齐）。
#[derive(Debug, Clone, Copy, PartialEq, ShaderType)]
pub struct SglLookParams {
    /// sRGB 字节/255；a 恒 1，透明度走 `shading.y`。
    pub colour: Vec4,
    /// (Ka, Kd, Ks, Kr)。
    pub coefficients: Vec4,
    /// (Kse, opacity, 兜底天顶亮度, 兜底地面亮度)。
    pub shading: Vec4,
    /// 头灯在眼空间的方向（xyz）。
    pub lamp_dir_eye: Vec4,
    /// x = 参与标记 0..3；y = 1 用环境贴图（0 用天地兜底）；z = 1 颜色不打光直出（像素探针用）；w 补齐。
    pub flags: UVec4,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct SglLookMaterial {
    #[uniform(0)]
    pub params: SglLookParams,
    /// sglDx11 内嵌的环境立方体贴图（`env_cube::SglEnvCube`）；`None` 时 `flags.y = 0`，走天地两段灰度兜底。
    #[texture(1, dimension = "cube")]
    #[sampler(2)]
    pub env: Option<Handle<Image>>,
}

impl SglLookMaterial {
    /// `colour` 是 PDMS 颜色表里的 `0xRRGGBB`（sRGB 字节）；`opacity` 1 = 不透明（E3D 的半透明百分比经
    /// [`translucency_to_alpha`] 换算）；`effect_flags` 见 [`EFFECT_ALL`]。
    pub fn new(
        colour: u32,
        opacity: f32,
        light: &SglLight,
        env: Option<Handle<Image>>,
        effect_flags: u32,
    ) -> Self {
        let [r, g, b] = rgb_bytes(colour);
        let use_env = env.is_some() as u32;
        Self {
            params: SglLookParams {
                colour: Vec4::new(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0),
                coefficients: Vec4::new(
                    light.ambient,
                    light.diffuse,
                    light.specular,
                    light.reflection,
                ),
                shading: Vec4::new(
                    light.specular_exponent,
                    opacity.clamp(0.0, 1.0),
                    SGL_ENV_SKY_LUM,
                    SGL_ENV_GROUND_LUM,
                ),
                lamp_dir_eye: Vec4::new(
                    light.lamp_dir_eye[0],
                    light.lamp_dir_eye[1],
                    light.lamp_dir_eye[2],
                    0.0,
                ),
                flags: UVec4::new(effect_flags & EFFECT_ALL, use_env, 0, 0),
            },
            env,
        }
    }

    /// 一次性换一套光照常量（切预设）。
    pub fn set_light(&mut self, light: &SglLight) {
        self.params.coefficients = Vec4::new(
            light.ambient,
            light.diffuse,
            light.specular,
            light.reflection,
        );
        self.params.shading.x = light.specular_exponent;
        self.params.lamp_dir_eye = Vec4::new(
            light.lamp_dir_eye[0],
            light.lamp_dir_eye[1],
            light.lamp_dir_eye[2],
            0.0,
        );
    }

    /// 透明度（1 = 不透明）。
    pub fn opacity(&self) -> f32 {
        self.params.shading.y
    }

    /// 颜色不打光直出（Ka = 1 的效果）：像素探针读 lightgrey 应为 189。
    pub fn flat(mut self) -> Self {
        self.params.flags.z = 1;
        self
    }
}

/// E3D 的半透明百分比（0 = 不透明，100 = 全透）→ alpha。
pub fn translucency_to_alpha(translucency_percent: f32) -> f32 {
    1.0 - translucency_percent.clamp(0.0, 100.0) / 100.0
}

impl Material for SglLookMaterial {
    fn fragment_shader() -> ShaderRef {
        SGL_LOOK_SHADER.into()
    }

    /// 同一个文件：Bevy 编 prepass 管线时定义 `PREPASS_PIPELINE`，走写标记那一半。
    fn prepass_fragment_shader() -> ShaderRef {
        SGL_LOOK_SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        if self.params.shading.y < 1.0 {
            AlphaMode::Blend
        } else {
            AlphaMode::Opaque
        }
    }

    fn specialize(
        _pipeline: &MaterialPipeline<Self>,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // PDMS 网格法线不保证朝外：不剔面（同 `model_material` 的 `cull_mode: None`），
        // 片元里按 front_facing 翻法线。
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::presets::{SGL_DEFAULT_LIGHT, SGL_E3D31_VIEW_DEFAULT_LIGHT};
    use super::*;

    #[test]
    fn the_colour_is_uploaded_as_srgb_bytes_over_255() {
        let material = SglLookMaterial::new(0xbdbdbd, 1.0, &SGL_DEFAULT_LIGHT, None, EFFECT_ALL);
        let c = material.params.colour;
        assert!((c.x - 189.0 / 255.0).abs() < 1e-6);
        assert_eq!(c.x, c.y);
        assert_eq!(c.y, c.z);
        assert_eq!(c.w, 1.0);
        assert_eq!(material.params.flags, UVec4::new(EFFECT_ALL, 0, 0, 0));
        assert_eq!(material.alpha_mode(), AlphaMode::Opaque);
    }

    #[test]
    fn the_light_constants_land_in_the_right_slots() {
        let material = SglLookMaterial::new(
            0xffff00,
            1.0,
            &SGL_E3D31_VIEW_DEFAULT_LIGHT,
            None,
            EFFECT_ALL,
        );
        assert_eq!(material.params.coefficients, Vec4::new(0.7, 0.0, 0.0, 0.8));
        assert_eq!(material.params.shading.x, 0.0);
        let mut material = material;
        material.set_light(&SGL_DEFAULT_LIGHT);
        assert_eq!(material.params.coefficients, Vec4::new(0.5, 0.8, 0.3, 0.35));
        assert_eq!(material.params.shading.x, 64.0);
        assert_eq!(material.params.lamp_dir_eye, Vec4::new(0.0, 0.0, 1.0, 0.0));
    }

    #[test]
    fn translucency_turns_into_a_blend_material() {
        assert_eq!(translucency_to_alpha(0.0), 1.0);
        assert_eq!(translucency_to_alpha(100.0), 0.0);
        assert_eq!(translucency_to_alpha(150.0), 0.0);
        let material = SglLookMaterial::new(0xbdbdbd, 0.25, &SGL_DEFAULT_LIGHT, None, EFFECT_ALL);
        assert_eq!(material.alpha_mode(), AlphaMode::Blend);
        assert_eq!(material.opacity(), 0.25);
    }

    #[test]
    fn effect_flags_are_masked_to_two_bits_and_env_switches_on_with_a_handle() {
        let material = SglLookMaterial::new(0, 1.0, &SGL_DEFAULT_LIGHT, None, 0xff);
        assert_eq!(material.params.flags.x, EFFECT_ALL);
        let material = SglLookMaterial::new(
            0,
            1.0,
            &SGL_DEFAULT_LIGHT,
            Some(Handle::default()),
            EFFECT_EDGES,
        );
        assert_eq!(material.params.flags, UVec4::new(EFFECT_EDGES, 1, 0, 0));
        assert_eq!(material.flat().params.flags.z, 1);
    }

    /// 着色器文本钉：公式的四项、sRGB 字节域输出、prepass 把线性深度写进 RGB、标记写进 alpha。改公式得连这条一起改。
    #[test]
    fn the_shader_implements_the_sgl_formula_and_writes_flags_in_the_prepass() {
        let source = include_str!("sgl_look.wgsl");
        for required in [
            "@group(2) @binding(0) var<uniform> material: SglLookParams;",
            "@group(2) @binding(1) var env_cube: texture_cube<f32>;",
            "@group(2) @binding(2) var env_sampler: sampler;",
            "pow(max(ndh, 1e-6), kse) * ndl_raw",
            "c * (ka + kd * ndl) + vec3(ks * spec) + kr * env",
            "srgb_to_linear(",
            "reflect(-v, n)",
            "f32(material.flags.x) / 3.0",
            "#ifdef PREPASS_PIPELINE",
            "#ifdef NORMAL_PREPASS",
            "const ND_DEPTH_RANGE: f32 = 65504.0;",
            "let eye = view.view_from_world * in.world_position;",
            "out.normal = vec4(encode_linear_depth(-eye.z), f32(material.flags.x) / 3.0);",
        ] {
            assert!(source.contains(required), "着色器缺了：{required}");
        }
    }
}
