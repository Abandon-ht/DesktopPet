@group(0) @binding(0) var scene: texture_2d<f32>;
override straight_alpha: bool = true;

@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let points = array(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    return vec4(points[i], 0.0, 1.0);
}

@fragment fn fs(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let c = textureLoad(scene, vec2<i32>(p.xy), 0);
    if straight_alpha {
        if c.a < 0.00001 { return vec4(0.0); }
        return vec4(c.rgb / c.a, c.a);
    }
    return c;
}
