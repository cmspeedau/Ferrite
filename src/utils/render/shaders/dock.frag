precision mediump float;
uniform vec2 size;
varying vec2 v_coords;
uniform float alpha;
uniform float radius;
uniform float slots;

void main() {
    vec2 px = v_coords * size;
    vec2 center = size / 2.0;
    vec2 half_size = size / 2.0;
    vec2 d = abs(px - center) - half_size + radius;
    float dist = length(max(d, 0.0)) - radius;
    float mask = smoothstep(1.0, 0.0, dist);

    vec3 top = vec3(0.157, 0.173, 0.204);
    vec3 bot = vec3(0.067, 0.075, 0.094);
    vec3 bg = mix(top, bot, v_coords.y);

    float border = smoothstep(1.5, 0.0, abs(dist));
    vec3 col = mix(bg, vec3(1.0), border * 0.08);

    // Hairline separators between the clickable slots.
    if (slots > 1.0) {
        float slot_w = size.x / slots;
        float fx = mod(px.x, slot_w);
        float dsep = min(fx, slot_w - fx);
        float sep = smoothstep(1.2, 0.0, dsep);
        float inner = step(1.0, px.x) * step(px.x, size.x - 1.0);
        col = mix(col, vec3(1.0), sep * inner * 0.10);
    }

    // Accent hairline along the top edge.
    float accent = smoothstep(2.0, 0.0, px.y);
    col = mix(col, vec3(1.0, 0.42, 0.0), accent * 0.85);

    gl_FragColor = vec4(col, mask * 0.92 * alpha);
}
