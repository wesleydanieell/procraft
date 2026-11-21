struct CameraMapping {
    x: vec4<f32>,
    y: vec4<f32>,
};
@group(0) @binding(0) var pixels: texture_2d<f32>;
@group(0) @binding(1) var bilinear: sampler;
@group(0) @binding(2) var<uniform> camera: CameraMapping;

@vertex fn vertex(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    var positions = array<vec2<f32>,3>(vec2<f32>(-1.,-1.), vec2<f32>(3.,-1.), vec2<f32>(-1.,3.));
    return vec4<f32>(positions[i],0.,1.);
}
@fragment fn fragment(@builtin(position) screen: vec4<f32>) -> @location(0) vec4<f32> {
    let p = vec3<f32>(screen.xy,1.);
    let uv = vec2<f32>(dot(camera.x.xyz,p),dot(camera.y.xyz,p));
    if any(uv < vec2<f32>(0.)) || any(uv >= vec2<f32>(1.)) { discard; }
    return textureSample(pixels,bilinear,uv);
}
