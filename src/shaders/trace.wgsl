[[builtin(vertex_index)]]
var<in> in_vertex_index: u32;
[[builtin(position)]]
var<out> out_pos: vec4<f32>;

[[block]] struct Params {
    value_change_count: u32;
    trace_start: vec2<u32>; // (left, bottom)
    trace_height: u32; // in pixels
};

// Try just having a bunch of commands (width, height) for each value change being rendered.
// I think these will have to be recomputed if the window is resized.
[[block]] struct ValueChange {
    width: u32; // in pixels
    height: f32; // [0, 1]
};

[[block]] struct Trace {
    value_changes: [[stride(8)]] array<ValueChange>;
};

[[group(0), binding(0)]] var<uniform> params: Params;
[[group(0), binding(1)]] var<storage> trace: Trace;

[[stage(vertex)]]
fn vs_main() {
    var x: f32 = f32(i32(in_vertex_index) - 1);
    var y: f32 = f32(i32(in_vertex_index & 1) * 2 - 1);
    out_pos = vec4<f32>(x, y, 0.0, 1.0);
}

[[location(0)]]
var<out> out_color: vec4<f32>;

[[stage(fragment)]]
fn fs_main() {
    out_color = vec4<f32>(1.0, 0.0, 0.0, 1.0);
}

