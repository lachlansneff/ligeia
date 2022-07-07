
struct Uniforms {
    scale: vec2<f32>,
    feather_fraction: f32,
    line_width: f32,
    window_start: f32,
    height: f32,
}

@group(0)
@binding(0)
var<uniform> uniforms: Uniforms;

@group(0)
@binding(1)
var<storage, read> timestamps: array<f32>;

@group(0)
@binding(2)
var<storage, read> level_bits: array<u32>;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) offset: f32,
}

fn generate_line(vertex: vec2<f32>, from: vec2<f32> to: vec2<f32>) -> vec4<f32> {
    // the vector parallel to the line
    let x_basis: vec2<f32> = point_b - point_a;
    // a unit vector normal to the line
    let y_basis: vec2<f32> = normalize(vec2<f32>(-x_basis.y, x_basis.x));
    let the_point: vec2<f32> = point_a + x_basis * vertex.x + y_basis * uniforms.line_width * vertex.y;

    return vec4<f32>(the_point * uniforms.scale, 0.0, 1.0);
}

@vertex
fn vs_vertical_lines(
    @builtin(instance_index) instance_index: u32,
    @location(0) vertex: vec2<f32>,
) -> VertexOutput {
    let point_a: vec2<f32> = vec2<f32>(timestamps[uniforms.window_start + instance_index]);
    let point_b: vec2<f32> = vec2<f32>(point_a.x, point_a.y + uniforms.height);

    var result: VertexOutput;
    result.position = generate_line(vertex, point_a, point_b);
    result.offset = vertex.y * 2f;
    return result;
}

@vertex
fn vs_horizontal_lines(
    @builtin(instance_index) instance_index: u32,
    @location(0) vertex: vec2<f32>,
) -> VertexOutput {
    let is_high: bool = extractBits(level_bits[instance_index / 32], instance_index % 32, 1) != 0u;
    let offset: f32 = select(0.0, uniforms.height, level);

    let point_a: vec2<f32> = vec2<f32>(timestamps[instance_index], offset);
    let point_b: vec2<f32> = vec2<f32>(timestamps[instance_index + 1u], offset);

    var result: VertexOutput;
    result.position = generate_line(vertex, point_a, point_b);
    result.offset = vertex.y * 2f;
    return result;
}

@fragment
fn fs_shared(
    input: VertexOutput,
) -> @location(0) vec4<f32> {
    let dist: f32 = abs(input.offset);

    var alpha: f32;
    if dist > 1f - uniforms.feather_fraction {
        alpha = (1f - dist) / uniforms.feather_fraction;
    } else {
        alpha = 1f;
    }

    return vec4<f32>(0.0, 0.0, 0.0, alpha);
}
