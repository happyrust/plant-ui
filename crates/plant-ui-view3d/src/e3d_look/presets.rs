//! E3D 外观的光照常量、两套预设与背景渐变（E3D 外观移植计划阶段一）。
//!
//! 数值全部照搬 plant3d-web `src/viewer/e3dLook/sglLookMaterial.ts` / `sglLookPresets.ts` /
//! `sglLookPipeline.ts`，来源是对 E3D 3.1 `sglDx11.dll` 的逆向（`docs/rendering/e3d-sgl-look-prototype.md` §2）。
//! 这里不重新推导任何常数。

use bevy::prelude::Color;
use plant_ui::LookPreset;

use super::colour_table::{E3D_BACKGROUND_GREY, rgb_bytes};

/// sglDx11 `CSglSceneLightParams`：前向着色的六个常数。
///
/// `rgb = colour · (Ka + Kd · max(N·L, 0)) + Ks · pow(max(N·H, 0), Kse) · max(N·L, 0) + Kr · env(reflect(−V, N))`，
/// 灯是眼空间里的一盏头灯（`lamp_dir_eye`，单位化后即 L），无衰减、无阴影、无 gamma / tonemap。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SglLight {
    /// Ka：环境项系数。
    pub ambient: f32,
    /// Kd：漫反射系数。
    pub diffuse: f32,
    /// Ks：Blinn-Phong 白色高光系数（不乘物体色）。
    pub specular: f32,
    /// Kr：环境立方体贴图反射系数。
    pub reflection: f32,
    /// Kse：高光指数。
    pub specular_exponent: f32,
    /// 灯在眼空间的方向（单位化后即 L）。
    pub lamp_dir_eye: [f32; 3],
}

/// `CSglSceneLightParams::CSglSceneLightParams()`（sglDx11 3.1 @0x1005a1b0）的 C++ 兜底值：
/// {Ambient 0.5, Diffuse 0.8, Specular 0.3, Reflection 0.35, Exponent 64, eye (0,0,1)}。
/// 本机那台经修补脚本启动的 E3D（没跑 PML 视图默认）实测吻合的就是它。
pub const SGL_DEFAULT_LIGHT: SglLight = SglLight {
    ambient: 0.5,
    diffuse: 0.8,
    specular: 0.3,
    reflection: 0.35,
    specular_exponent: 64.0,
    lamp_dir_eye: [0.0, 0.0, 1.0],
};

/// E3D 3.1 出厂视图设置（PML `gphviewopt.pmlobj .default()` 经 VIEW 属性 79..83 写进策略 1）：
/// brightness = Ka 0.7、colourDepth = Kd 0、reflection = Kr 0.8、mirrorEffect = Ks 0、spotSize = Kse 0。
/// 即未改过设置的 E3D 3.1 没有漫反射和高光，`rgb = 0.7·c + 0.8·env(reflect(−V, N))`，体积感全靠环境贴图。
pub const SGL_E3D31_VIEW_DEFAULT_LIGHT: SglLight = SglLight {
    ambient: 0.7,
    diffuse: 0.0,
    specular: 0.0,
    reflection: 0.8,
    specular_exponent: 0.0,
    lamp_dir_eye: [0.0, 0.0, 1.0],
};

/// `CSglMaterialLightingStrategyList::SetDefaultSceneLightParams(false)` 的另两套策略：
/// `FLAT70` = 纯环境 0.7 不打光（顶点 ATTRIBFLAGS 策略 2），`UNLIT` = 原色（策略 3，辅助线 / 文字 / 手柄）。
pub const SGL_LIGHT_FLAT70: SglLight = SglLight {
    ambient: 0.7,
    diffuse: 0.0,
    specular: 0.0,
    reflection: 0.0,
    specular_exponent: 0.0,
    lamp_dir_eye: [0.0, 0.0, 1.0],
};
pub const SGL_LIGHT_UNLIT: SglLight = SglLight {
    ambient: 1.0,
    diffuse: 0.0,
    specular: 0.0,
    reflection: 0.0,
    specular_exponent: 0.0,
    lamp_dir_eye: [0.0, 0.0, 1.0],
};

/// 没有环境贴图时的解析兜底：天顶 / 地面两段灰度（plant3d-web 的 `envSkyLum` / `envGroundLum` 缺省）。
pub const SGL_ENV_SKY_LUM: f32 = 0.75;
pub const SGL_ENV_GROUND_LUM: f32 = 0.25;

/// 渐变端色：sglDx11 未显式设置时的算法是「背景色 → HLS，S × 0.1、L 拉满 → RGB」，L ≥ 240 时任何背景色都得纯白。
pub const E3D_GRADIENT_END: u32 = 0xffffff;
/// E3D 3.1 用 Direct2D 画渐变：渐变线从视口上方 0.35H 到下方 1.15H（上 = 背景色、下 = 端色），
/// 所以可见区里的渐变参数 t 从顶部 0.35/1.5 到底部 1.35/1.5。
pub const E3D_GRADIENT_TOP_T: f32 = 0.35 / 1.5;
pub const E3D_GRADIENT_BOTTOM_T: f32 = 1.35 / 1.5;

/// 一套预设的光照常量。
pub const fn preset_light(preset: LookPreset) -> SglLight {
    match preset {
        LookPreset::E3d31Factory => SGL_E3D31_VIEW_DEFAULT_LIGHT,
        LookPreset::SglMachine => SGL_DEFAULT_LIGHT,
    }
}

/// E3D 外观的背景面片上下两色（两套预设背景都是 grey #828282）：渐变开 = 在字节域里按 E3D 的
/// t 区间把 grey 往白插值（顶 ≈ #9f9f9f、底 ≈ #f2f2f2，即 plant3d-web §5.3 量到的 159 / 242）；
/// 渐变关 = 上下都是纯 grey。颜色以 sRGB 字节给 Bevy，经顶点色 → sRGB 目标输出后屏幕上就是这几个字节。
pub fn e3d_background(gradient: bool) -> (Color, Color) {
    let grey = rgb_bytes(E3D_BACKGROUND_GREY);
    if !gradient {
        let flat = Color::srgb_u8(grey[0], grey[1], grey[2]);
        return (flat, flat);
    }
    let top = mix_bytes(grey, rgb_bytes(E3D_GRADIENT_END), E3D_GRADIENT_TOP_T);
    let bottom = mix_bytes(grey, rgb_bytes(E3D_GRADIENT_END), E3D_GRADIENT_BOTTOM_T);
    (
        Color::srgb_u8(top[0], top[1], top[2]),
        Color::srgb_u8(bottom[0], bottom[1], bottom[2]),
    )
}

/// 字节域线性插值（D2D 的渐变就是在 8-bit 上插的），向下截断——plant3d-web §5.3 量到的底色是 242，
/// 130 + 125 × 0.9 = 242.5 四舍五入会得 243，截断才和测量值对上。
pub fn mix_bytes(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    let mut out = [0u8; 3];
    for i in 0..3 {
        out[i] = (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t).clamp(0.0, 255.0) as u8;
    }
    out
}

#[cfg(test)]
mod tests {
    use bevy::color::ColorToPacked;

    use super::*;

    #[test]
    fn light_constants_match_plant3d_web_to_the_digit() {
        // sglLookMaterial.ts SGL_DEFAULT_LIGHT / SGL_E3D31_VIEW_DEFAULT_LIGHT
        assert_eq!(
            SGL_DEFAULT_LIGHT,
            SglLight {
                ambient: 0.5,
                diffuse: 0.8,
                specular: 0.3,
                reflection: 0.35,
                specular_exponent: 64.0,
                lamp_dir_eye: [0.0, 0.0, 1.0]
            }
        );
        assert_eq!(
            SGL_E3D31_VIEW_DEFAULT_LIGHT,
            SglLight {
                ambient: 0.7,
                diffuse: 0.0,
                specular: 0.0,
                reflection: 0.8,
                specular_exponent: 0.0,
                lamp_dir_eye: [0.0, 0.0, 1.0]
            }
        );
        assert_eq!(
            preset_light(LookPreset::E3d31Factory),
            SGL_E3D31_VIEW_DEFAULT_LIGHT
        );
        assert_eq!(preset_light(LookPreset::SglMachine), SGL_DEFAULT_LIGHT);
        assert_eq!(SGL_LIGHT_FLAT70.ambient, 0.7);
        assert_eq!(SGL_LIGHT_UNLIT.ambient, 1.0);
    }

    #[test]
    fn the_factory_gradient_is_grey_to_white_measured_at_159_and_242() {
        // plant3d-web §5.3：出厂预设背景顶 (159,159,159) / 底 (242,242,242)
        let (top, bottom) = e3d_background(true);
        assert_eq!(top.to_srgba().to_u8_array()[..3], [159, 159, 159]);
        assert_eq!(bottom.to_srgba().to_u8_array()[..3], [242, 242, 242]);
        let (flat_top, flat_bottom) = e3d_background(false);
        assert_eq!(flat_top, flat_bottom);
        assert_eq!(flat_top.to_srgba().to_u8_array()[..3], [130, 130, 130]);
    }

    #[test]
    fn gradient_t_range_is_the_d2d_line_from_minus_035h_to_115h() {
        assert!((E3D_GRADIENT_TOP_T - 0.2333).abs() < 1e-3);
        assert!((E3D_GRADIENT_BOTTOM_T - 0.9).abs() < 1e-6);
        assert_eq!(mix_bytes([0, 0, 0], [255, 255, 255], 0.0), [0, 0, 0]);
        assert_eq!(mix_bytes([0, 0, 0], [255, 255, 255], 1.0), [255, 255, 255]);
    }
}
