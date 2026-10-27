// E3D 外观后处理链：plant3d-web `sglLookPipeline.ts` 六段 GLSL 的 WGSL 翻译（来源是对 E3D 3.1 sglDx11 的逆向）。
// 同一个文件按 shader_def 分六条管线，每条自己一套绑定：
//   SGL_ND         prepass 法线纹理（RGB = 线性深度 3×10 bit、a = 标记）→ ND（rgb = 眼空间面法线 × 标记长度，
//                  a = 线性深度，背景 0）。不读硬件深度：WebGL2 的 GLSL 后端做不了深度纹理的 textureLoad。
//                  ND 纹理是 Rgba32Float（同来源 `_ndType = canFloat ? FloatType : HalfFloatType`）：f16 只有 11 bit
//                  尾数，百米外（> 1024 单位）的量化步长 ≥ 1 单位，超过 50 mm 的边线深度阈值，每跨一个量化台阶
//                  HLR 判据③就打一条边——2026-10-25 整库全视里筒体上的条纹、穹顶上的同心环就是它与子像素格的莫尔纹。
//                  设备只有半浮点颜色附件时才退回 Rgba16Float（百米外照旧会出条纹，同来源）。
//   SGL_AO         HBAO（NVIDIA 同款参数，法线模式）
//   SGL_BLUR       深度感知的可分离模糊（SGL_BLUR_Y 定义时走纵向）
//   SGL_HLR        边线（逐句照 sglDx11 HLR PS）
//   SGL_DECLUTTER  去杂（sglDx11 HLRDeclutter）
//   SGL_COMPOSITE  合成 rgb = mix(背景, colour, colour.a) · hlr · ao，输出不透明
//
// 长度单位是世界单位（1 单位 = 100 mm）：settings 里的半径 / 阈值由 Rust 侧换好。
// 背景哨兵：sglDx11 是深度 1e18，这里是线性深度 0（Bevy reverse-Z 清 0）。
// 像素坐标 y 向下（wgpu），GLSL 原文里 uv 的 +y 是屏幕向上，所有邻居方向都按此换过。
// 超采样（toggles.w = s > 1）时 ND / HLR / 合成跑在 s 倍分辨率上，AO / 模糊 / 去杂跑在屏幕分辨率上。

#import bevy_render::view::View
#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

struct SglLookPost {
    // x = 边线开，y = 伪阴影开，z = 合成生效（E3D 外观），w = 超采样倍数（ND 像素 / 屏幕像素；AO / 模糊 / 去杂
    //     跑屏幕分辨率，HLR 的邻居仍是 1 个屏幕像素 = w 个子像素，同 sglDx11 MSAA 版每个采样点读 ±1 像素邻居）
    toggles: vec4<u32>,
    // x = HBAO 半径 R，y = 1/R²，z = 角度偏置（弧度），w = 衰减
    ao: vec4<f32>,
    // x = 对比度，y = 方向数，z = 步数，w = 模糊深度锐度（世界单位的倒数）
    ao2: vec4<f32>,
    // x = 深度差阈值，y = 法线 |cos| 阈值，z = 梯度方向 |dot| 阈值，w = 梯度步长常数（世界单位，再乘像素步长）
    hlr: vec4<f32>,
    // x = 去杂 g_MinColour，y = g_MaxColour，z = 样本权重 1/9，w = 模糊半径（像素）
    declutter: vec4<f32>,
    // 背景渐变在视口顶 / 底的两色（sRGB 字节 / 255；渐变关时两色相同）
    bg_top: vec4<f32>,
    bg_bottom: vec4<f32>,
    // x = 调试直出模式（0 关；见 SGL_COMPOSITE 段的 debug_colour），其余留空
    debug: vec4<u32>,
}

fn is_background(depth: f32) -> bool {
    return depth <= 0.0;
}

// 标记编在法线长度里：0.25 × (1 + 码)，码 bit0 = 边线、bit1 = 伪阴影（同 sglEffectFlagScale）
fn effect_code(n: vec3<f32>) -> i32 {
    return i32(floor(length(n) * 4.0 + 0.5)) - 1;
}

fn edge_participant(n: vec3<f32>) -> bool {
    let c = effect_code(n);
    return c == 1 || c == 3;
}

fn shadow_participant(n: vec3<f32>) -> bool {
    return effect_code(n) >= 2;
}

// 与 sgl_look.wgsl 的 encode_linear_depth 是一对：法线纹理 RGB 的 3 × 10 bit 定点 → 线性眼空间深度（世界单位）。
// 清零的纹理（没画过的像素）解出 0 = 背景。
const ND_DEPTH_RANGE: f32 = 65504.0;

fn decode_linear_depth(rgb: vec3<f32>) -> f32 {
    let q = round(rgb * 1023.0);
    return (q.x / 1024.0 + q.y / 1048576.0 + q.z / 1073741824.0) * ND_DEPTH_RANGE;
}

// ---------------------------------------------------------------------------
#ifdef SGL_ND

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var normal_tex: texture_2d<f32>;

fn is_perspective() -> bool {
    return view.clip_from_view[2][3] != 0.0;
}

// 像素 + 线性深度 → 眼空间位置（透视 ndc·depth/focal，正交 (ndc − offset)/focal；同 SGL_AO 段的 eye_pos）
fn view_pos(px: vec2<i32>, size: vec2<i32>, depth: f32) -> vec3<f32> {
    let uv = (vec2<f32>(px) + vec2(0.5)) / vec2<f32>(size);
    let ndc = vec2(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let focal = vec2(view.clip_from_view[0][0], view.clip_from_view[1][1]);
    if (is_perspective()) {
        return vec3(ndc * depth / focal, -depth);
    }
    let offset = vec2(view.clip_from_view[3][0], view.clip_from_view[3][1]);
    return vec3((ndc - offset) / focal, -depth);
}

fn depth_at(px: vec2<i32>, size: vec2<i32>) -> f32 {
    return decode_linear_depth(textureLoad(normal_tex, clamp(px, vec2(0), size - vec2(1)), 0).rgb);
}

// 选差分一侧时用的「仿射深度」：平面上在屏幕里是仿射的那个量 —— 透视是 1/深度（硬件深度的线性函数），
// 正交是深度本身。背景记 0：透视域里 0 = 无穷远，正交域里 0 = 相机处，对前景像素都是一个大外推误差，选不中。
fn affine_depth(depth: f32) -> f32 {
    if (is_perspective()) {
        return select(0.0, 1.0 / depth, depth > 0.0);
    }
    return depth;
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(normal_tex));
    let px = vec2<i32>(in.position.xy);
    let d = depth_at(px, size);
    if (is_background(d)) {
        return vec4(0.0);
    }
    let p = view_pos(px, size, d);
    // 每个轴取「用该侧两点线性外推最能命中中心深度」的一侧做差分：平面上的仿射深度（见 affine_depth）在屏幕上
    // 是仿射的，同一面上外推误差为 0，棱线两侧的像素于是各取自己那一面。只比深度差大小的话，深度连续的凸 / 凹棱
    // 两侧都会跨棱取样、得到两面法线的中间值，判据④漏判，棱线随亚像素位置断续。
    let dl = depth_at(px + vec2(-1, 0), size);
    let dr = depth_at(px + vec2(1, 0), size);
    let du = depth_at(px + vec2(0, -1), size);
    let dd = depth_at(px + vec2(0, 1), size);
    let a = affine_depth(d);
    let left_err = abs(2.0 * affine_depth(dl) - affine_depth(depth_at(px + vec2(-2, 0), size)) - a);
    let right_err = abs(2.0 * affine_depth(dr) - affine_depth(depth_at(px + vec2(2, 0), size)) - a);
    let up_err = abs(2.0 * affine_depth(du) - affine_depth(depth_at(px + vec2(0, -2), size)) - a);
    let down_err = abs(2.0 * affine_depth(dd) - affine_depth(depth_at(px + vec2(0, 2), size)) - a);
    let dx = select(
        view_pos(px + vec2(1, 0), size, dr) - p,
        p - view_pos(px + vec2(-1, 0), size, dl),
        left_err < right_err,
    );
    let dy = select(
        view_pos(px + vec2(0, 1), size, dd) - p,
        p - view_pos(px + vec2(0, -1), size, du),
        up_err < down_err,
    );
    // 屏幕向下 = 眼空间 -y，cross(-y, +x) = +z 朝向相机；再按视线兜一次底。两侧都是背景的一像素细条
    // 差分会退化（背景深度 0 落在相机处），退成正对相机，免得 NaN 混进标记长度。
    let raw = cross(dy, dx);
    let len2 = dot(raw, raw);
    var n = vec3(0.0, 0.0, 1.0);
    if (len2 > 0.0 && len2 < 1.0e30) {
        n = raw * inverseSqrt(len2);
    }
    if (dot(n, -p) < 0.0) {
        n = -n;
    }
    let code = u32(round(textureLoad(normal_tex, px, 0).a * 3.0));
    let scale = 0.25 * f32(1u + code);
    return vec4(n * scale, d);
}

#endif

// ---------------------------------------------------------------------------
#ifdef SGL_AO

@group(0) @binding(0) var<uniform> settings: SglLookPost;
@group(0) @binding(1) var<uniform> view: View;
@group(0) @binding(2) var nd_tex: texture_2d<f32>;

fn is_perspective() -> bool {
    return view.clip_from_view[2][3] != 0.0;
}

// 像素 + 线性深度 → 眼空间位置（AO_FRAGMENT 的 viewPos：透视 ndc·depth/focal，正交 (ndc − offset)/focal）
fn eye_pos(px: vec2<f32>, size: vec2<f32>, depth: f32) -> vec3<f32> {
    let uv = (px + vec2(0.5)) / size;
    let ndc = vec2(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let focal = vec2(view.clip_from_view[0][0], view.clip_from_view[1][1]);
    if (is_perspective()) {
        return vec3(ndc * depth / focal, -depth);
    }
    let offset = vec2(view.clip_from_view[3][0], view.clip_from_view[3][1]);
    return vec3((ndc - offset) / focal, -depth);
}

fn hash12(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + vec3(33.33));
    return fract((p3.x + p3.y) * p3.z);
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(nd_tex));
    let fsize = vec2<f32>(size);
    // AO 目标可能比 ND 小（超采样时 AO 跑屏幕分辨率）：本像素对应 ND 里那一块的中心，其余全在 ND 像素里算
    let scale = i32(settings.toggles.w);
    let px = min(vec2<i32>(in.position.xy) * scale + scale / 2, size - vec2(1));
    let nd = textureLoad(nd_tex, px, 0);
    let depth = nd.a;
    // 背景、以及不参与伪阴影的对象不接收 AO
    if (is_background(depth) || !shadow_participant(nd.rgb)) {
        return vec4(1.0);
    }
    let p = eye_pos(vec2<f32>(px), fsize, depth);
    let n = normalize(nd.rgb);
    let focal_y = view.clip_from_view[1][1];
    var r_pix = settings.ao.x * focal_y * fsize.y * 0.5;
    if (is_perspective()) {
        r_pix = r_pix / depth;
    }
    if (r_pix < 1.0) {
        return vec4(1.0);
    }
    let num_dirs = settings.ao2.y;
    let num_steps = min(settings.ao2.z, r_pix);
    let step_pix = r_pix / (num_steps + 1.0);
    let jitter = hash12(in.position.xy);
    let alpha = 6.28318530718 / num_dirs;
    var ao = 0.0;
    for (var d = 0; d < 32; d++) {
        if (f32(d) >= num_dirs) {
            break;
        }
        let ang = alpha * (f32(d) + jitter);
        // 方向在眼空间 / NDC 里（y 向上）；走像素时 y 取反
        let dir = vec2(cos(ang), sin(ang));
        // 屏幕方向投到切平面 → 切线仰角（NVIDIA HBAO 的 useNormal 路）
        var t = vec3(dir, 0.0);
        t -= n * dot(t, n);
        let tan_angle = atan2(t.z, max(length(t.xy), 1e-5)) + settings.ao.z;
        var sin_h = sin(tan_angle);
        for (var s = 0; s < 32; s++) {
            if (f32(s) >= num_steps) {
                break;
            }
            let offset = vec2(dir.x, -dir.y) * (step_pix * (f32(s) + 1.0 + 0.5 * jitter));
            let spx_f = vec2<f32>(px) + offset;
            if (spx_f.x < 0.0 || spx_f.y < 0.0 || spx_f.x >= fsize.x || spx_f.y >= fsize.y) {
                break;
            }
            let spx = vec2<i32>(spx_f);
            let nd_s = textureLoad(nd_tex, spx, 0);
            // 不参与伪阴影的对象也不遮挡邻居
            if (is_background(nd_s.a) || !shadow_participant(nd_s.rgb)) {
                continue;
            }
            let s_pos = eye_pos(vec2<f32>(spx), fsize, nd_s.a);
            let h = s_pos - p;
            let len2 = dot(h, h);
            if (len2 * settings.ao.y > 1.0) {
                continue;
            }
            let sin_s = h.z * inverseSqrt(max(len2, 1e-8));
            if (sin_s > sin_h) {
                let att = 1.0 - len2 * settings.ao.y * settings.ao.w;
                ao += (sin_s - sin_h) * att;
                sin_h = sin_s;
            }
        }
    }
    ao = 1.0 - ao / num_dirs * settings.ao2.x;
    return vec4(vec3(clamp(ao, 0.0, 1.0)), 1.0);
}

#endif

// ---------------------------------------------------------------------------
#ifdef SGL_BLUR

@group(0) @binding(0) var<uniform> settings: SglLookPost;
@group(0) @binding(1) var nd_tex: texture_2d<f32>;
@group(0) @binding(2) var source_tex: texture_2d<f32>;

// AO 分辨率的像素 → ND 里那一块的中心深度
fn nd_depth_at(px_ao: vec2<i32>, nd_size: vec2<i32>, scale: i32) -> f32 {
    return textureLoad(nd_tex, min(px_ao * scale + scale / 2, nd_size - vec2(1)), 0).a;
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let nd_size = vec2<i32>(textureDimensions(nd_tex));
    let size = vec2<i32>(textureDimensions(source_tex));
    let scale = i32(settings.toggles.w);
    let px = vec2<i32>(in.position.xy);
    let center_depth = nd_depth_at(px, nd_size, scale);
    if (is_background(center_depth)) {
        return vec4(1.0);
    }
    let radius = settings.declutter.w;
    // NVIDIA HBAO 同款：g_BlurFalloff = 1 / (2σ²)，σ = (R + 1) / 2
    let sigma = (radius + 1.0) * 0.5;
    let falloff = 1.0 / (2.0 * sigma * sigma);
    let sharpness = settings.ao2.w;
#ifdef SGL_BLUR_Y
    let step = vec2(0, 1);
#else
    let step = vec2(1, 0);
#endif
    var total = 0.0;
    var wsum = 0.0;
    for (var i = -16; i <= 16; i++) {
        let fi = f32(i);
        if (abs(fi) > radius) {
            continue;
        }
        let spx = clamp(px + step * i, vec2(0), size - vec2(1));
        let d = nd_depth_at(spx, nd_size, scale);
        if (is_background(d)) {
            continue;
        }
        let dz = (d - center_depth) * sharpness;
        let w = exp(-fi * fi * falloff) * exp(-dz * dz);
        total += textureLoad(source_tex, spx, 0).r * w;
        wsum += w;
    }
    let v = select(1.0, total / wsum, wsum > 0.0);
    return vec4(vec3(v), 1.0);
}

#endif

// ---------------------------------------------------------------------------
#ifdef SGL_HLR

@group(0) @binding(0) var<uniform> settings: SglLookPost;
@group(0) @binding(1) var nd_tex: texture_2d<f32>;

fn nd_at(px: vec2<i32>, size: vec2<i32>) -> vec4<f32> {
    return textureLoad(nd_tex, clamp(px, vec2(0), size - vec2(1)), 0);
}

// 「有标记」= 是几何且参与边线（sglDx11 法线纹理 .w bit0）
fn flagged(s: vec4<f32>) -> bool {
    return !is_background(s.a) && edge_participant(s.rgb);
}

// 一个方向的判边：next = 右 / 下，prev = 左 / 上，h = 该轴的梯度步长·像素步长。true = 边
fn edge_along(c: vec4<f32>, next: vec4<f32>, prev: vec4<f32>, h: f32) -> bool {
    let flag_c = flagged(c);
    let flag_n = flagged(next);
    // ① 中心无标记：邻居有标记 且（中心是背景 或 dC − dN > 阈值）
    if (!flag_c) {
        return flag_n && (is_background(c.a) || c.a - next.a > settings.hlr.x);
    }
    // ② 邻居无标记：邻居是背景 或 dC − dN < −阈值（中心更近）
    if (!flag_n) {
        return is_background(next.a) || c.a - next.a < -settings.hlr.x;
    }
    // ③ 深度台阶 + 梯度方向变化（平面无论多陡两向量反向平行、|dot| = 1，只有真台阶才判边）
    if (abs(c.a - next.a) > settings.hlr.x && flagged(prev)) {
        let v1 = normalize(vec2(c.a - prev.a, h));
        let v2 = normalize(vec2(c.a - next.a, -h));
        if (abs(dot(v1, v2)) < settings.hlr.z) {
            return true;
        }
    }
    // ④ 法线折痕
    return abs(dot(normalize(c.rgb), normalize(next.rgb))) < settings.hlr.y;
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(nd_tex));
    let px = vec2<i32>(in.position.xy);
    let c = nd_at(px, size);
    // 邻居 1 个屏幕像素（超采样时 = step 个子像素）；h = 梯度步长常数 × 屏幕像素步长（sglDx11 的 cb1[0] × 1000），
    // 不随超采样变
    let step = max(i32(settings.toggles.w), 1);
    let h = settings.hlr.w * f32(step) / vec2<f32>(size);
    let right = nd_at(px + vec2(step, 0), size);
    let left = nd_at(px - vec2(step, 0), size);
    let down = nd_at(px + vec2(0, step), size);
    let up = nd_at(px - vec2(0, step), size);
    let edge = edge_along(c, right, left, h.x) || edge_along(c, down, up, h.y);
    return vec4(vec3(select(1.0, 0.0, edge)), 1.0);
}

#endif

// ---------------------------------------------------------------------------
#ifdef SGL_DECLUTTER

@group(0) @binding(0) var<uniform> settings: SglLookPost;
@group(0) @binding(1) var hlr_tex: texture_2d<f32>;

// 一个屏幕像素的边线值：对它的 s×s 个 HLR 子像素取平均（= sglDx11 MSAA 版 HLR 的 Σ/N，来源的 hlrResolved）；
// 越界夹到边缘像素。
fn edge_at(px: vec2<i32>, screen: vec2<i32>, s: i32) -> f32 {
    let hlr_size = vec2<i32>(textureDimensions(hlr_tex));
    let base = clamp(px, vec2(0), screen - vec2(1)) * s;
    var acc = 0.0;
    for (var j = 0; j < 4; j++) {
        if (j >= s) {
            break;
        }
        for (var i = 0; i < 4; i++) {
            if (i >= s) {
                break;
            }
            acc += textureLoad(hlr_tex, min(base + vec2(i, j), hlr_size - vec2(1)), 0).r;
        }
    }
    return acc / f32(s * s);
}

// 跑在屏幕分辨率上，窗口与 E3D 一样是 2×2 个屏幕像素：本像素、左、上、左上，权重 1/9；
// t = saturate((sum − min) / (max − min))；out = lerp(1, c, t)
@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let s = max(i32(settings.toggles.w), 1);
    let screen = (vec2<i32>(textureDimensions(hlr_tex)) + vec2(s - 1)) / s;
    let px = vec2<i32>(in.position.xy);
    let c = edge_at(px, screen, s);
    let sum = (c + edge_at(px + vec2(-1, 0), screen, s) + edge_at(px + vec2(0, -1), screen, s)
        + edge_at(px + vec2(-1, -1), screen, s)) * settings.declutter.z;
    let t = clamp(
        (sum - settings.declutter.x) / max(1.0e-6, settings.declutter.y - settings.declutter.x),
        0.0,
        1.0,
    );
    return vec4(vec3(mix(1.0, c, t)), 1.0);
}

#endif

// ---------------------------------------------------------------------------
#ifdef SGL_COMPOSITE

@group(0) @binding(0) var<uniform> settings: SglLookPost;
@group(0) @binding(1) var colour_tex: texture_2d<f32>;
@group(0) @binding(2) var edge_tex: texture_2d<f32>;
@group(0) @binding(3) var ao_tex: texture_2d<f32>;
// 下面三张只给调试直出读：ND、去杂前的 HLR、prepass 法线纹理（都是 ND 分辨率）
@group(0) @binding(4) var nd_tex: texture_2d<f32>;
@group(0) @binding(5) var hlr_tex: texture_2d<f32>;
@group(0) @binding(6) var normal_tex: texture_2d<f32>;

// 调试直出（`?look_debug=…` / `--look-debug=…`，Rust 侧 `SglLookDebugView`）：把后处理链的中间量当颜色画出来，
// 查两端画面差异用。px 是 ND 分辨率的像素，block 是它所在的屏幕像素；背景一律黑。两样效果都关时 ND / HLR 没跑、
// 中间纹理只是 1×1 占位，直出一片黑。
fn debug_colour(mode: u32, px: vec2<i32>, block: vec2<i32>) -> vec3<f32> {
    let nd = textureLoad(nd_tex, px, 0);
    let depth = decode_linear_depth(textureLoad(normal_tex, px, 0).rgb);
    if (is_background(depth)) {
        return vec3(0.0);
    }
    switch mode {
        // 1 depth：prepass 里的线性深度，10 单位（1 m）一个锯齿周期
        case 1u: {
            return vec3(fract(depth / 10.0));
        }
        // 2 depth_error：ND 纹理存回的深度相对 prepass 30 bit 深度的误差，按边线深度阈值归一（绿 0 → 红 ≥ 1 个阈值）
        case 2u: {
            let e = clamp(abs(nd.a - depth) / settings.hlr.x, 0.0, 1.0);
            return vec3(e, 1.0 - e, 0.0);
        }
        // 3 normal：ND 眼空间面法线
        case 3u: {
            return normalize(nd.rgb) * 0.5 + 0.5;
        }
        // 4 flags：参与标记——边线红、伪阴影绿、都参与黄、都不参与白
        case 4u: {
            let code = effect_code(nd.rgb);
            return select(
                vec3(f32(code == 1 || code == 3), f32(code >= 2), 0.0),
                vec3(1.0),
                code == 0,
            );
        }
        // 5 edges：去杂前的 HLR 边线（0 = 边）
        case 5u: {
            return vec3(textureLoad(hlr_tex, px, 0).r);
        }
        // 6 ao：模糊后的伪阴影（屏幕分辨率）
        default: {
            let ao_size = vec2<i32>(textureDimensions(ao_tex));
            return vec3(textureLoad(ao_tex, min(block, ao_size - vec2(1)), 0).r);
        }
    }
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(max(c, vec3(0.0)), vec3(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3(0.0031308));
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((max(c, vec3(0.0)) + 0.055) / 1.055, vec3(2.4));
    return select(hi, lo, c <= vec3(0.04045));
}

// E3D 在 8-bit UNORM 目标的字节域里合成：背景渐变、半透明叠加、× 边线 × 伪阴影都按字节算，超采样时再对
// s×s 个子像素在字节域取平均（= MSAA 解析）。colour 是在透明黑底上画出来的（线性、预乘），先除回直通色再转字节。
// 输出不透明，且同一屏幕像素的 s×s 个子像素写同一个值：egui 线性采样缩回视口时，相同值的平均还是它自己，
// 屏幕上就是字节域的结果；egui 不再参与混合，也就不会在线性空间里把边线压浅、对预乘色再乘一次 alpha。
@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let s = max(i32(settings.toggles.w), 1);
    let size = vec2<i32>(textureDimensions(colour_tex));
    let screen = (size + vec2(s - 1)) / s;
    let px = vec2<i32>(in.position.xy);
    let block = px / s;
    if (settings.debug.x != 0u) {
        return vec4(debug_colour(settings.debug.x, px, block), 1.0);
    }
    // 背景：视口顶 → 底在字节域里线性插值（同 E3D 的 D2D 渐变），按本屏幕像素的行取
    let t = (f32(block.y) + 0.5) / f32(screen.y);
    let bg = mix(settings.bg_top.rgb, settings.bg_bottom.rgb, t);
    var col = vec3(0.0);
    for (var j = 0; j < 4; j++) {
        if (j >= s) {
            break;
        }
        for (var i = 0; i < 4; i++) {
            if (i >= s) {
                break;
            }
            let c = textureLoad(colour_tex, min(block * s + vec2(i, j), size - vec2(1)), 0);
            let a = clamp(c.a, 0.0, 1.0);
            let straight = select(vec3(0.0), c.rgb / max(a, 1.0e-6), a > 0.0);
            col += mix(bg, linear_to_srgb(clamp(straight, vec3(0.0), vec3(1.0))), a);
        }
    }
    col /= f32(s * s);
    var hlr = 1.0;
    if (settings.toggles.x != 0u) {
        let edge_size = vec2<i32>(textureDimensions(edge_tex));
        hlr = textureLoad(edge_tex, min(block, edge_size - vec2(1)), 0).r;
    }
    var ao = 1.0;
    if (settings.toggles.y != 0u) {
        let ao_size = vec2<i32>(textureDimensions(ao_tex));
        ao = textureLoad(ao_tex, min(block, ao_size - vec2(1)), 0).r;
    }
    let bytes = col * hlr * ao;
    return vec4(srgb_to_linear(bytes), 1.0);
}

#endif
