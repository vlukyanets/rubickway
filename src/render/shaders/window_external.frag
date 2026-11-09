#version 300 es
// Samples a GPU image. The driver converts any format, video included, and returns channels in
// order, with alpha 1 for formats without alpha. Content is premultiplied.
#extension GL_OES_EGL_image_external_essl3 : require
precision mediump float;

uniform samplerExternalOES u_texture;
uniform float u_swizzle;   // 1.0 when drawing into a mipmap copy, which window.frag samples

in vec2 v_uv;
out vec4 color;

void main() {
    vec4 t = texture(u_texture, v_uv);
    color = u_swizzle > 0.5 ? t.bgra : t;
}
