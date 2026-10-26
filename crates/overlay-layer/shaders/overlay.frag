#version 450
// The overlay image is premultiplied RGBA; blending is ONE, ONE_MINUS_SRC_ALPHA.
layout(constant_id = 0) const bool HDR10 = false;
layout(set = 0, binding = 0) uniform sampler2D overlay;
layout(location = 0) in vec2 uv;
layout(location = 0) out vec4 color;

// HDR10 swapchains hold BT.2020 primaries encoded with the ST 2084 (PQ) curve. The overlay is
// sRGB, so it is decoded, moved to BT.2020 and placed at the SDR reference white (203 nits)
// before PQ encoding; otherwise it would show dark, washed out and with shifted hues.
const float SDR_WHITE_NITS = 203.0;

vec3 srgb_to_linear(vec3 c) {
    return mix(c / 12.92, pow((c + 0.055) / 1.055, vec3(2.4)), greaterThan(c, vec3(0.04045)));
}

vec3 pq_encode(vec3 nits) {
    const float m1 = 2610.0 / 16384.0;
    const float m2 = 2523.0 / 4096.0 * 128.0;
    const float c1 = 3424.0 / 4096.0;
    const float c2 = 2413.0 / 4096.0 * 32.0;
    const float c3 = 2392.0 / 4096.0 * 32.0;
    vec3 p = pow(clamp(nits / 10000.0, 0.0, 1.0), vec3(m1));
    return pow((c1 + c2 * p) / (1.0 + c3 * p), vec3(m2));
}

void main() {
    vec4 texel = texture(overlay, uv);
    if (!HDR10) {
        color = texel;
        return;
    }
    vec3 straight = texel.a > 0.0 ? texel.rgb / texel.a : vec3(0.0);
    // BT.709 to BT.2020 primaries (ITU-R BT.2087), column-major.
    const mat3 to_2020 = mat3(
        0.6274, 0.0691, 0.0164,
        0.3293, 0.9195, 0.0880,
        0.0433, 0.0114, 0.8956);
    vec3 nits = to_2020 * srgb_to_linear(straight) * SDR_WHITE_NITS;
    color = vec4(pq_encode(nits) * texel.a, texel.a);
}
