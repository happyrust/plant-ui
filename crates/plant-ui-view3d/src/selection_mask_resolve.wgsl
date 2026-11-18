// 选中描边的第一段（ADR-0032 修订 2026-11-18）：遮罩相机按屏幕分辨率的 2 倍画的遮罩（只用 alpha），平均成一张
// 屏幕分辨率的覆盖率图（单通道）。第二段 `selection_outline.wgsl` 读的是这张图。
//
// 目标的每个像素中心正好落在 2×2 个遮罩像素的交点上，线性采样一次取到的就是这 4 个的平均——等于自己做了一遍
// 4 个采样的多重采样解析，选中件的边缘在覆盖率图里是连续的灰度。遮罩不开硬件多重采样的缘由见
// `selection_outline.rs` 的 `spawn_selection_mask`。
// 视口大到遮罩放不下 2 倍时它会缩到不足 2 倍：取到的是像素中心周围 4 个遮罩像素的加权平均，照样是覆盖率，只是粗一些。

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

@group(0) @binding(0) var mask_tex: texture_2d<f32>;
@group(0) @binding(1) var mask_sampler: sampler;

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let coverage = textureSampleLevel(mask_tex, mask_sampler, in.uv, 0.0).a;
    return vec4<f32>(coverage, 0.0, 0.0, 1.0);
}
