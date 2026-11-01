#version 300 es
// One window surface as a quad. Positions are physical pixels with the origin at the top-left
// corner; texture coordinates go through the viewport crop and the buffer transform.
precision highp float;

uniform vec2 u_viewport;   // framebuffer size
uniform vec4 u_dst;        // x, y, width, height in pixels
uniform vec4 u_src;        // crop in normalised surface coordinates: x, y, width, height
uniform int u_transform;   // wl_output.transform value, 0..7

out vec2 v_uv;

// Maps a point of the (cropped) surface, 0..1 with y down, to buffer texture coordinates.
vec2 to_buffer(vec2 s) {
    if (u_transform == 1) return vec2(s.y, 1.0 - s.x);        // 90
    if (u_transform == 2) return vec2(1.0 - s.x, 1.0 - s.y);  // 180
    if (u_transform == 3) return vec2(1.0 - s.y, s.x);        // 270
    if (u_transform == 4) return vec2(1.0 - s.x, s.y);        // flipped
    if (u_transform == 5) return vec2(s.y, s.x);              // flipped 90
    if (u_transform == 6) return vec2(s.x, 1.0 - s.y);        // flipped 180
    if (u_transform == 7) return vec2(1.0 - s.y, 1.0 - s.x);  // flipped 270
    return s;
}

void main() {
    // Triangle strip corners: (0,0), (1,0), (0,1), (1,1).
    vec2 corner = vec2(float(gl_VertexID & 1), float((gl_VertexID >> 1) & 1));
    vec2 px = u_dst.xy + corner * u_dst.zw;
    vec2 ndc = px / u_viewport * 2.0 - 1.0;
    gl_Position = vec4(ndc.x, -ndc.y, 0.0, 1.0);
    v_uv = to_buffer(u_src.xy + corner * u_src.zw);
}
