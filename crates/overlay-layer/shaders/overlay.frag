#version 450
// The overlay image is premultiplied RGBA; blending is ONE, ONE_MINUS_SRC_ALPHA.
layout(set = 0, binding = 0) uniform sampler2D overlay;
layout(location = 0) in vec2 uv;
layout(location = 0) out vec4 color;

void main() {
    color = texture(overlay, uv);
}
