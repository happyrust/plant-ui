#import bevy_pbr::mesh_functions::{get_world_from_local, mesh_position_local_to_clip}
#import bevy_pbr::mesh_view_bindings as view_bindings

// Material bind groups are injected by Bevy (currently group 3).  Hard-coding
// group 2 collides with the mesh bind group and leaves this material without a
// render pipeline, so invalid TUBIs disappear entirely.
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> line_color: vec4<f32>;
// x = line width, y = dash, z = gap, w = antialias radius; all in physical pixels.
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var<uniform> line_params: vec4<f32>;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) side: f32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) @interpolate(linear) line_position_px: vec2<f32>,
    @location(1) @interpolate(flat) line_length_px: f32,
};

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    let world_from_local = get_world_from_local(vertex.instance_index);
    let start_clip = mesh_position_local_to_clip(world_from_local, vec4<f32>(0.0, 0.0, 0.0, 1.0));
    let end_clip = mesh_position_local_to_clip(world_from_local, vec4<f32>(0.0, 0.0, 1.0, 1.0));
    let viewport_size = max(view_bindings::view.viewport.zw, vec2<f32>(1.0));
    let start_ndc = start_clip.xy / start_clip.w;
    let end_ndc = end_clip.xy / end_clip.w;
    let projected_delta_px = (end_ndc - start_ndc) * vec2<f32>(0.5, -0.5) * viewport_size;
    let line_length_px = length(projected_delta_px);
    let direction_px = select(vec2<f32>(1.0, 0.0), projected_delta_px / line_length_px, line_length_px > 0.0001);
    let normal_px = vec2<f32>(-direction_px.y, direction_px.x);
    let endpoint = select(start_clip, end_clip, vertex.position.z > 0.5);
    let half_width_px = line_params.x * 0.5;
    let offset_ndc = normal_px * vertex.side * half_width_px * 2.0 / viewport_size;

    var out: VertexOutput;
    out.clip_position = endpoint;
    out.clip_position.xy += offset_ndc * endpoint.w;
    out.line_position_px = vec2<f32>(vertex.position.z * line_length_px, vertex.side * half_width_px);
    out.line_length_px = line_length_px;
    return out;
}

struct FragmentInput {
    @location(0) @interpolate(linear) line_position_px: vec2<f32>,
    @location(1) @interpolate(flat) line_length_px: f32,
};

@fragment
fn fragment(input: FragmentInput) -> @location(0) vec4<f32> {
    let width_px = line_params.x;
    let dash_px = line_params.y;
    let gap_px = line_params.z;
    let aa_px = line_params.w;
    let half_width_px = width_px * 0.5;

    let cross_aa = max(aa_px, fwidth(input.line_position_px.y));
    let cross_alpha = 1.0 - smoothstep(
        max(half_width_px - cross_aa, 0.0),
        half_width_px,
        abs(input.line_position_px.y),
    );

    let along_aa = max(aa_px, fwidth(input.line_position_px.x));
    var pattern_alpha = 1.0;
    let period_px = dash_px + gap_px;
    if input.line_length_px >= period_px {
        let phase_px = input.line_position_px.x - floor(input.line_position_px.x / period_px) * period_px;
        let dash_start = smoothstep(0.0, along_aa, phase_px);
        let dash_end = 1.0 - smoothstep(max(dash_px - along_aa, 0.0), dash_px, phase_px);
        pattern_alpha = min(dash_start, dash_end);
    } else {
        // A projected segment shorter than one 5+4 px period is a complete short line.
        let line_start = smoothstep(0.0, along_aa, input.line_position_px.x);
        let line_end = 1.0 - smoothstep(
            max(input.line_length_px - along_aa, 0.0),
            input.line_length_px,
            input.line_position_px.x,
        );
        pattern_alpha = min(line_start, line_end);
    }

    let alpha = cross_alpha * pattern_alpha * line_color.a;
    if alpha <= 0.001 {
        discard;
    }
    return vec4<f32>(line_color.rgb, alpha);
}
