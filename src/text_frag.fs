#version 100
varying lowp vec2 tex_uv;
uniform sampler2D tex;
uniform lowp vec3 color;
void main() {
    //value = smoothstep(0.4, 0.6, texture2D(tex, tex_uv));
    lowp float value = texture2D(tex, tex_uv).a;
    gl_FragColor = vec4(color, value);
}
