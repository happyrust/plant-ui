// 选中描边的第二段（ADR-0032 修订 2026-11-18）：读选中件剪影的覆盖率图，在剪影外沿叠一圈描边色。
// 遮罩里只有选中件、没有别的构件，所以被挡住的那一段轮廓也在：描边会画在挡它的构件上（plant3d-web 的 OutlinePass 同口径）。
//
// 覆盖率图（单通道，屏幕分辨率）是第一段 `selection_mask_resolve.wgsl` 把遮罩相机画的 alpha 平均出来的，选中件的边缘
// 在里面是连续的灰度。这里按 uv 线性采样它；颜色目标可以是它的整数倍（E3D 外观的超采样），按像素坐标读。
// 遮罩是选中件用自己的材质画出来的：半透明的选中件覆盖率也不到 1，所以内外不按绝对值判，按邻域里的
// 最大值（level）归一后再判——不透明件 level = 1，归一等于没做。

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

struct SelectionOutline {
    // rgb = 描边色（sRGB 字节 / 255），a = 强度
    colour: vec4<f32>,
    // x = 描边宽度（覆盖率图的像素 = 屏幕像素），其余留空
    params: vec4<f32>,
}

@group(0) @binding(0) var<uniform> outline: SelectionOutline;
@group(0) @binding(1) var colour_tex: texture_2d<f32>;
@group(0) @binding(2) var coverage_tex: texture_2d<f32>;
@group(0) @binding(3) var coverage_sampler: sampler;

// 估 level 的那一圈比描边外圈再往里多探的像素数：外圈的采样点刚蹭到剪影边（只拿到半个像素的覆盖率）时，
// 同方向再进 1.5 像素的那一点已经完全在剪影里，拿到的才是这个件真正的 alpha——描边外沿靠这两者之比渐隐。
const LEVEL_REACH: f32 = 1.5;
// level 低于 1/8 的（几乎全透明的选中件）描边跟着淡下去，不把噪声放大成一圈实线。
const LEVEL_FULL: f32 = 0.125;

fn coverage_at(uv: vec2<f32>) -> f32 {
    return textureSampleLevel(coverage_tex, coverage_sampler, uv, 0.0).r;
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((max(c, vec3<f32>(0.0)) + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let src = textureLoad(colour_tex, vec2<i32>(in.position.xy), 0);
    let centre = coverage_at(in.uv);
    // 不透明选中件的内部：描边只画在剪影外面
    if (centre >= 0.999) {
        return src;
    }
    let texel = 1.0 / vec2<f32>(textureDimensions(coverage_tex));
    let width = outline.params.x;
    // 16 个方向，22.5° 一个。外圈与内圈用偶数位，中圈用奇数位，错开半格：细线从两个外圈采样点中间穿过去时，
    // 中圈能接住。
    var dirs = array<vec2<f32>, 16>(
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.92388, 0.382683),
        vec2<f32>(0.707107, 0.707107),
        vec2<f32>(0.382683, 0.92388),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(-0.382683, 0.92388),
        vec2<f32>(-0.707107, 0.707107),
        vec2<f32>(-0.92388, 0.382683),
        vec2<f32>(-1.0, 0.0),
        vec2<f32>(-0.92388, -0.382683),
        vec2<f32>(-0.707107, -0.707107),
        vec2<f32>(-0.382683, -0.92388),
        vec2<f32>(0.0, -1.0),
        vec2<f32>(0.382683, -0.92388),
        vec2<f32>(0.707107, -0.707107),
        vec2<f32>(0.92388, -0.382683),
    );
    // near：宽度之内有没有选中件（三圈 × 8 方向取最大）
    var near = 0.0;
    for (var i = 0; i < 8; i++) {
        let even = dirs[i * 2] * texel;
        let odd = dirs[i * 2 + 1] * texel;
        near = max(near, coverage_at(in.uv + even * width));
        near = max(near, coverage_at(in.uv + odd * (width * 0.6667)));
        near = max(near, coverage_at(in.uv + even * (width * 0.3333)));
    }
    var level = max(centre, near);
    if (level <= 0.0) {
        return src;
    }
    for (var i = 0; i < 8; i++) {
        level = max(level, coverage_at(in.uv + dirs[i * 2] * texel * (width + LEVEL_REACH)));
    }
    // 在描边带里（near）又不在剪影里（1 − inside）：两头都是连续量，内沿跟着覆盖率走，外沿跟着线性采样走
    let present = clamp(near / level, 0.0, 1.0);
    let inside = clamp(centre / level, 0.0, 1.0);
    let k = present * (1.0 - inside) * clamp(level / LEVEL_FULL, 0.0, 1.0) * outline.colour.a;
    if (k <= 0.0) {
        return src;
    }
    // 在 sRGB 字节域里叠（E3D 外观的合成段同口径，线性空间里叠会把描边的抗锯齿边缘压浅），按直通 alpha 做 over：
    // PBR 外观的离屏图底是透明的，egui 拿它当直通 alpha 铺到视口上；E3D 外观合成后 alpha 恒为 1，就是普通的插值。
    let src_bytes = linear_to_srgb(clamp(src.rgb, vec3<f32>(0.0), vec3<f32>(1.0)));
    let out_a = k + src.a * (1.0 - k);
    let bytes = (outline.colour.rgb * k + src_bytes * (src.a * (1.0 - k))) / max(out_a, 1.0e-5);
    return vec4<f32>(srgb_to_linear(bytes), out_a);
}
