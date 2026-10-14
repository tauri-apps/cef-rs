// Vertex shader
struct VertexInput {
    @location(0) pos: vec4<f32>,
    @location(1) tex: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) pos: vec4<f32>,
    @location(0) tex: vec2<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.pos = input.pos;
    output.tex = input.tex;
    return output;
}

// Fragment shader
@group(0) @binding(0) var tex0: texture_2d<f32>;
@group(0) @binding(1) var samp0: sampler;

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(tex0, samp0, input.tex);
}

