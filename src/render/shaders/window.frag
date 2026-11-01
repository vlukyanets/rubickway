#version 300 es
// Samples a window texture. Buffers are little-endian ARGB8888 / XRGB8888 uploaded as RGBA
// bytes, so the channels arrive as B, G, R, A and are swizzled here. Content is premultiplied.
precision mediump float;

uniform sampler2D u_texture;
uniform float u_opaque;   // 1.0 for XRGB8888: ignore the alpha byte

in vec2 v_uv;
out vec4 color;

void main() {
    vec4 t = texture(u_texture, v_uv).bgra;
    color = u_opaque > 0.5 ? vec4(t.rgb, 1.0) : t;
}
