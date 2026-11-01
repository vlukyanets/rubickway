#version 300 es
// Adaptive canvas grid. All inputs are physical pixels with the origin at the top-left corner;
// see render/grid.rs for how they are computed.
precision highp float;

uniform float u_height;   // buffer height, to flip gl_FragCoord.y
uniform float u_minor;    // minor spacing
uniform vec2 u_offset;    // position of a super-major line (every 16 minor lines), per axis
uniform float u_fade;     // minor line fade, 0..1
uniform vec2 u_axis;      // origin axis positions
uniform vec2 u_axis_on;   // 1.0 when the axis is on screen
uniform vec3 u_background;
uniform vec3 u_alpha;     // minor, major, axis opacity

out vec4 color;

// Coverage of a 1-pixel-wide line whose pixel starts at the line position, sampled at a pixel
// centre `d` pixels away from the line's centre.
float coverage(float d) {
    return clamp(1.0 - abs(d), 0.0, 1.0);
}

float grid_alpha(float p, float offset) {
    float rel = p - 0.5 - offset;
    float n = floor(rel / u_minor + 0.5);
    float d = rel - n * u_minor;
    float a;
    if (mod(n, 16.0) < 0.5) {
        a = u_alpha.y;
    } else if (mod(n, 4.0) < 0.5) {
        a = mix(u_alpha.x, u_alpha.y, u_fade);
    } else {
        a = u_alpha.x * u_fade;
    }
    return a * coverage(d);
}

void main() {
    vec2 p = vec2(gl_FragCoord.x, u_height - gl_FragCoord.y);
    float a = max(grid_alpha(p.x, u_offset.x), grid_alpha(p.y, u_offset.y));
    if (u_axis_on.x > 0.5) {
        a = max(a, u_alpha.z * coverage(p.x - 0.5 - u_axis.x));
    }
    if (u_axis_on.y > 0.5) {
        a = max(a, u_alpha.z * coverage(p.y - 0.5 - u_axis.y));
    }
    color = vec4(mix(u_background, vec3(1.0), a), 1.0);
}
