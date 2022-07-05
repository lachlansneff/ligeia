

@group(0)
@binding(0)
var<storage, read> value_change_locations: array<f32>;

@group(0)
@binding(1)
var<storage, read> values: array<u32>;

// Each quad has 6 vertices, so `index` should be between 0 and 5, inclusive.
// Generates a quad (two triangles) that goes straight up from `start`.
fn index_to_quad(index: u32, start: vec2<f32>, length: f32, width: f32) -> vec2<f32> {
    
}

@vertex
fn vs_main(
    @builtin(vertex_index) in_vertex_index: u32,
    @builtin(instance_index) in_instance_index: u32,
) -> @builtin(position) vec4<f32> {

}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    
}
