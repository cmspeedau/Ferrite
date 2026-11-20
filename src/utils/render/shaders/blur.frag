precision mediump float;
uniform sampler2D tex;
uniform vec2 size;
varying vec2 v_coords;
uniform float alpha;
uniform float radius;

void main() {
    vec2 texel = 1.0 / size;
    vec4 color = vec4(0.0);
    float total = 0.0;
    int r = int(radius);
    for (int x = -r; x <= r; x++) {
        for (int y = -r; y <= r; y++) {
            float w = exp(-(float(x * x + y * y)) / (2.0 * radius * radius + 0.0001));
            color += texture2D(tex, v_coords + vec2(float(x), float(y)) * texel) * w;
            total += w;
        }
    }
    gl_FragColor = (color / total) * alpha;
}
