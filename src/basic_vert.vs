#version 100
attribute vec2 local_pos;
uniform vec2 offset;
uniform vec2 world_offset;
uniform float angle;
uniform vec2 scale;
void main() {
    //Counterclockwise rotation (mat2 takes columns)
    mat2 rotation = mat2(cos(angle),sin(angle),
                    -sin(angle),cos(angle));
    gl_Position = vec4(((rotation*local_pos)+offset+world_offset)*scale, 0, 1);
}