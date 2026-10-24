// SglLookMaterial：复刻 sglDx11 前向实体着色（plant3d-web sglLookMaterial.ts 的 WGSL 翻译）。
//
//   L = normalize(LampDir)   V = normalize(眼 − p)   N = normalize(n)   H = normalize(V + L)
//   rgb = colour · (Ka + Kd · max(N·L, 0)) + Ks · pow(max(N·H, 0), Kse) · max(N·L, 0) + Kr · env(reflect(−V, N))
//
// 颜色在 sRGB 字节域里算（和 sglDx11 直写 UNORM 目标一样），输出前转线性抵消 sRGB 目标的编码。
// 头灯方向在眼空间给（默认 (0,0,1) = 沿视线朝向观者），这里换到世界空间后与世界空间的 N / V 一起算，
// 等价于在眼空间算。环境立方体贴图用世界空间反射向量直接采：plant-ui 世界 = PDMS (x, z, −y)，
// 正是 sglDx11 采样前做的那次换轴（见 env_cube.rs）。
//
// 两条管线共用这一个文件：Bevy 编 prepass 管线时定义 PREPASS_PIPELINE，prepass 片元把**线性眼空间深度**写进
// 法线纹理的 RGB（3 × 10 bit 定点，见 encode_linear_depth）、把参与标记写进 alpha（Rgb10a2Unorm 的 2 bit，
// flags / 3 精确可表示）。后处理不读硬件深度纹理：WebGL2 的 GLSL 后端做不了深度纹理的 textureLoad
//（阶段四 spike 实测），面法线本来就是由深度差分重建的，所以法线纹理里不存法线、存深度，两端一套。

#ifdef PREPASS_PIPELINE
#import bevy_pbr::prepass_io::{VertexOutput, FragmentOutput}
#else
#import bevy_pbr::forward_io::VertexOutput
#endif
#import bevy_pbr::mesh_view_bindings::view

// ND 深度的归一范围（世界单位，1 单位 = 100 mm：65504 单位 = 6.5 km）；30 bit 定点的分辨率 ≈ 0.006 mm。
// 与 sgl_look_post.wgsl 的 decode_linear_depth 是一对。
const ND_DEPTH_RANGE: f32 = 65504.0;

// 线性深度 → 3 × 10 bit（高 / 中 / 低），每段存成 k/1023，UNORM10 能精确存回。分段在浮点里做，
// 避开 f32 只有 24 bit 尾数的限制。
fn encode_linear_depth(depth: f32) -> vec3<f32> {
    let v = clamp(depth / ND_DEPTH_RANGE, 0.0, 0.99999994);
    let s0 = v * 1024.0;
    let hi = floor(s0);
    let s1 = (s0 - hi) * 1024.0;
    let mid = floor(s1);
    let lo = floor((s1 - mid) * 1024.0);
    return vec3(hi, mid, lo) / 1023.0;
}

struct SglLookParams {
    // sRGB 字节/255，a 恒 1
    colour: vec4<f32>,
    // Ka Kd Ks Kr
    coefficients: vec4<f32>,
    // Kse, opacity, 兜底天顶亮度, 兜底地面亮度
    shading: vec4<f32>,
    // 头灯在眼空间的方向
    lamp_dir_eye: vec4<f32>,
    // x = 参与标记 0..3；y = 1 用环境贴图；z = 1 颜色直出不打光
    flags: vec4<u32>,
}

@group(2) @binding(0) var<uniform> material: SglLookParams;
@group(2) @binding(1) var env_cube: texture_cube<f32>;
@group(2) @binding(2) var env_sampler: sampler;

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + vec3(0.055)) / 1.055, vec3(2.4));
    return select(hi, lo, c <= vec3(0.04045));
}

#ifdef PREPASS_PIPELINE

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var out: FragmentOutput;
#ifdef NORMAL_PREPASS
    // RGB = 线性眼空间深度（世界单位），alpha = 参与标记（sglDx11 是法线纹理 .w 的两位）
    let eye = view.view_from_world * in.world_position;
    out.normal = vec4(encode_linear_depth(-eye.z), f32(material.flags.x) / 3.0);
#endif
#ifdef MOTION_VECTOR_PREPASS
    out.motion_vector = vec2(0.0);
#endif
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    out.frag_depth = in.unclipped_depth;
#endif
    return out;
}

#else

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> @location(0) vec4<f32> {
    var n = normalize(in.world_normal);
    if (!is_front) {
        n = -n;
    }
    let v = normalize(view.world_position - in.world_position.xyz);
    // 头灯：眼空间方向换到世界空间（方向向量，w = 0）
    let l = normalize((view.world_from_view * vec4(material.lamp_dir_eye.xyz, 0.0)).xyz);
    let h = normalize(v + l);

    let ka = material.coefficients.x;
    let kd = material.coefficients.y;
    let ks = material.coefficients.z;
    let kr = material.coefficients.w;
    let kse = material.shading.x;

    let ndl_raw = dot(n, l);
    let ndh = dot(n, h);
    // sglDx11 ps: (NdotL >= 0 && NdotH >= 0) ? pow(NdotH, Kse) * NdotL : 0
    var spec = 0.0;
    if (ndl_raw >= 0.0 && ndh >= 0.0) {
        spec = pow(max(ndh, 1e-6), kse) * ndl_raw;
    }
    let ndl = max(ndl_raw, 0.0);

    // 环境反射：Kr · texCUBE(env, reflect(−V, N))，逐通道；没贴图时用天地两段灰度兜底（世界上 = +Y）
    let r = reflect(-v, n);
    let sampled = textureSample(env_cube, env_sampler, r).rgb;
    let t = clamp(r.y * 0.5 + 0.5, 0.0, 1.0);
    let analytic = vec3(mix(material.shading.w, material.shading.z, t));
    let env = select(analytic, sampled, material.flags.y == 1u);

    let c = material.colour.rgb;
    var rgb = c * (ka + kd * ndl) + vec3(ks * spec) + kr * env;
    if (material.flags.z == 1u) {
        // 像素探针用：Ka = 1 直出，lightgrey 应读回 189
        rgb = c;
    }
    return vec4(srgb_to_linear(clamp(rgb, vec3(0.0), vec3(1.0))), material.shading.y);
}

#endif
