#version 100
varying lowp vec2 tex_uv;
uniform sampler2D tex;
uniform lowp float opacity;
void main() {
    lowp vec4 texel = texture2D(tex, tex_uv);
    gl_FragColor = vec4(texel.rgb, texel.a*opacity);
}