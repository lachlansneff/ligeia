
let half_width: f32 = 0.01;

@group(0)
@binding(0)
var<storage, read> points: array<vec2<f32>>;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) offset: f32,
}

@vertex
fn vs_main(
    @builtin(instance_index) instance_index: u32,
    @location(0) vertex: vec2<f32>,
) -> VertexOutput {
    let point_a: vec2<f32> = points[instance_index];
    let point_b: vec2<f32> = points[instance_index + 1u];

    // the vector parallel to the line
    let x_basis: vec2<f32> = point_b - point_a;
    // a unit vector normal to the line
    let y_basis: vec2<f32> = normalize(vec2<f32>(-x_basis.y, x_basis.x));
    let the_point: vec2<f32> = point_a + x_basis * vertex.x + y_basis * half_width * vertex.y;

    var result: VertexOutput;
    result.position = vec4<f32>(the_point, 0.0, 1.0);
    result.offset = vertex.y * 2f;
    return result;
}

let feather: f32 = 0.5;

@fragment
fn fs_main(
    input: VertexOutput,
) -> @location(0) vec4<f32> {
    let dist: f32 = abs(input.offset);

    var alpha: f32;
    if dist > 1f - feather {
        alpha = (1f - dist) / feather;
    } else {
        alpha = 1f;
    }

    return vec4<f32>(1.0, 1.0, 0.0, alpha);
}
