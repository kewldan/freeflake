// Shaders for the snow overlay. All positions are desktop points; the
// viewport uniform maps the part of the desktop covered by one surface to
// clip space. Colours are written premultiplied.

struct Viewport {
    origin: vec2<f32>,
    size: vec2<f32>,
    // Physical pixels per point, for anti-aliasing.
    scale: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
};

@group(0) @binding(0) var<uniform> viewport: Viewport;

fn to_clip(p: vec2<f32>) -> vec4<f32> {
    let n = (p - viewport.origin) / viewport.size;
    return vec4<f32>(n.x * 2.0 - 1.0, 1.0 - n.y * 2.0, 0.0, 1.0);
}

// ---------------------------------------------------------------------------
// Triangle meshes (snow piles, the tree).

struct MeshOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn mesh_vs(@location(0) pos: vec2<f32>, @location(1) color: vec4<f32>) -> MeshOut {
    var out: MeshOut;
    out.pos = to_clip(pos);
    out.color = vec4<f32>(color.rgb * color.a, color.a);
    return out;
}

@fragment
fn mesh_fs(in: MeshOut) -> @location(0) vec4<f32> {
    return in.color;
}

// ---------------------------------------------------------------------------
// Instanced discs (flakes, chunks, snowballs, ornaments).

struct DiscOut {
    @builtin(position) pos: vec4<f32>,
    // Position inside the disc, in radii.
    @location(0) local: vec2<f32>,
    @location(1) color: vec4<f32>,
    // x: radius in pixels, y: outline strength, z: softness.
    @location(2) params: vec3<f32>,
};

const QUAD = array<vec2<f32>, 6>(
    vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
    vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
);

// How far the quad reaches beyond the disc, in radii, to fit the outline.
const REACH: f32 = 1.7;

@vertex
fn disc_vs(
    @builtin(vertex_index) vi: u32,
    @location(0) center: vec2<f32>,
    @location(1) radius: f32,
    @location(2) outline: f32,
    @location(3) color: vec4<f32>,
    @location(4) softness: f32,
) -> DiscOut {
    var quad = QUAD;
    let corner = quad[vi] * REACH;
    var out: DiscOut;
    out.pos = to_clip(center + corner * radius);
    out.local = corner;
    out.color = color;
    out.params = vec3<f32>(radius * viewport.scale, outline, softness);
    return out;
}

@fragment
fn disc_fs(in: DiscOut) -> @location(0) vec4<f32> {
    let d = length(in.local);
    // Anti-aliasing width in radii: about one pixel.
    let aa = 1.0 / max(in.params.x, 0.5);
    let edge = max(aa, in.params.z);
    let fill = 1.0 - smoothstep(1.0 - edge, 1.0 + aa, d);
    // A faint dark rim keeps white snow visible over white windows.
    let ring = (1.0 - smoothstep(1.15, REACH, d)) * (1.0 - fill) * in.params.y;
    let fill_a = fill * in.color.a;
    let ring_a = ring * 0.22 * in.color.a;
    let shade = vec3<f32>(0.25, 0.32, 0.42);
    let rgb = in.color.rgb * fill_a + shade * ring_a;
    return vec4<f32>(rgb, fill_a + ring_a * (1.0 - fill_a));
}
