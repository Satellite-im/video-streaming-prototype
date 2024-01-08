
// note that webgl2 seems to expect the textures side length be a power of 2. 
// 

// glsl scripts and webgl2 adapted from this blog post: https://medium.com/docler-engineering/webgl-video-manipulation-8d0892b565b6
// chatgpt "helped" a little here too 

var refs = {};
// the opengl texture, which is bound using gl.bindTexture() and then assigned to using the uniforms
var textures = {};
var attributes = {};
var buffers = {};
var uniforms = {};

const vertexShaderSource = `#version 300 es
in vec2 position;
in vec2 a_texCoord;
out vec2 texCoord;
uniform vec2 u_resolution;

void main() {
    // convert the position from pixels to 0.0 to 1.0
    vec2 zeroToOne = position / u_resolution;

    // convert from 0->1 to 0->2
    vec2 zeroToTwo = zeroToOne * 2.0;

    // convert from 0->2 to -1->+1 (clipspace)
    vec2 clipSpace = zeroToTwo - 1.0;

    gl_Position = vec4(clipSpace * vec2(1, -1), 0, 1);
   
    texCoord = (a_texCoord);
}
`;

const fragmentShaderSource = `#version 300 es
precision highp float;

in vec2 texCoord;
// these uniforms are 1d textures with GL_R8 internal format
uniform sampler2D y_plane;
uniform sampler2D u_plane;
uniform sampler2D v_plane;
out vec4 fragColor;

// https://gist.github.com/crearo/798412489698e749c17e572e74496e1b
void main() {
    float r, g, b, y, u, v;
    y = texture(y_plane, texCoord)[0];
    // wtf - why did this work? 
    // the test program converted from RGB to Y/Cb/Cr with a range of 0-255 but subtracting 0.5 
    // makes it look like the scale was from 0.0 to 1.0. 
    u = texture(u_plane, texCoord)[0] - 0.5; // 128.0;
    v = texture(v_plane, texCoord)[0] - 0.5; // 128.0;


    r = y + 1.4*v;
    g = y-0.343*u-0.711*v;
    b = y+1.765*u;

    //r = clamp(r, 0.0, 255.0);
    //g = clamp(g, 0.0, 255.0);
    //b = clamp(b, 0.0, 255.0);

    fragColor =  vec4( r, g, b, 1.0)  ;
}
`;

function createShader(gl, type, source) {
    const shader = gl.createShader(type);
    gl.shaderSource(shader, source);
    gl.compileShader(shader);

    if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
        console.error(`Shader compilation error: ${gl.getShaderInfoLog(shader)}`);
        gl.deleteShader(shader);
        return null;
    }

    return shader;
}

function createProgram(gl, vertexShaderSource, fragmentShaderSource) {
    const vertexShader = createShader(gl, gl.VERTEX_SHADER, vertexShaderSource);
    const fragmentShader = createShader(gl, gl.FRAGMENT_SHADER, fragmentShaderSource);

    if (!vertexShader || !fragmentShader) {
        return null;
    }

    const program = gl.createProgram();
    gl.attachShader(program, vertexShader);
    gl.attachShader(program, fragmentShader);
    gl.linkProgram(program);

    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
        console.error(`Program linking error: ${gl.getProgramInfoLog(program)}`);
        gl.deleteProgram(program);
        return null;
    }

    return program;
}

function init() {
    const canvas = document.querySelector("#canvas") || document.createElement("canvas");
    canvas.width = 512;
    canvas.height = 512;

    const gl = canvas.getContext("webgl2");
    if (!gl) {
        console.error("Unable to initialize WebGL. Your browser may not support it.");
        return;
    }
   
    const program = createProgram(gl, vertexShaderSource, fragmentShaderSource);
    gl.useProgram(program);

    // Create a vertex array object (attribute state)
    refs.vao = gl.createVertexArray();
    // and make it the one we're currently working with
    gl.bindVertexArray(refs.vao);

    const positionBuffer = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, positionBuffer);
    var x1 = 0;
    var x2 = canvas.width;
    var y1 = 0;
    var y2 = canvas.height;
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([
        x1, y1,
        x2, y1,
        x1, y2,
        x1, y2,
        x2, y1,
        x2, y2,
    ]), gl.STATIC_DRAW);

    const positionLocation = gl.getAttribLocation(program, "position");
    gl.enableVertexAttribArray(positionLocation);
    // Tell the attribute how to get data out of positionBuffer (ARRAY_BUFFER)
    var size = 2;          // 2 components per iteration
    var type = gl.FLOAT;   // the data is 32bit floats
    var normalize = false; // don't normalize the data
    var stride = 0;        // 0 = move forward size * sizeof(type) each iteration to get the next position
    var offset = 0;        // start at the beginning of the buffer
    gl.vertexAttribPointer(
        positionLocation, size, type, normalize, stride, offset);

    const texLocation = gl.getAttribLocation(program, "a_texCoord");
    // provide texture coordinates for the rectangle.
    var texCoordBuffer = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, texCoordBuffer);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([
        0.0,  0.0,
        1.0,  0.0,
        0.0,  1.0,
        0.0,  1.0,
        1.0,  0.0,
        1.0,  1.0,
    ]), gl.STATIC_DRAW);

    // Turn on the attribute
    gl.enableVertexAttribArray(texLocation);
    // Tell the attribute how to get data out of texcoordBuffer (ARRAY_BUFFER)
    var size = 2;          // 2 components per iteration
    var type = gl.FLOAT;   // the data is 32bit floats
    var normalize = false; // don't normalize the data
    var stride = 0;        // 0 = move forward size * sizeof(type) each iteration to get the next position
    var offset = 0;        // start at the beginning of the buffer
    gl.vertexAttribPointer(
        texLocation, size, type, normalize, stride, offset);

    // set u_resolution
    const resolutionLocation = gl.getUniformLocation(program, "u_resolution");
    gl.uniform2f(resolutionLocation, canvas.width, canvas.height);

    refs.program = program;
    refs.gl = gl;
    refs.canvas = canvas;
}

function render(gl, program, yData, uData, vData, width, height) {
    const yTexture = createTexture(gl, yData, width, height);
    const uTexture = createTexture(gl, uData, width / 2, height / 2);
    const vTexture = createTexture(gl, vData, width / 2, height / 2);

    gl.uniform1i(gl.getUniformLocation(program, "y_plane"), 0);
    gl.uniform1i(gl.getUniformLocation(program, "u_plane"), 1);
    gl.uniform1i(gl.getUniformLocation(program, "v_plane"), 2);

    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, yTexture);

    gl.activeTexture(gl.TEXTURE1);
    gl.bindTexture(gl.TEXTURE_2D, uTexture);

    gl.activeTexture(gl.TEXTURE2);
    gl.bindTexture(gl.TEXTURE_2D, vTexture);

    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.drawArrays(gl.TRIANGLES, 0, 6);

    // Release the textures
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, null);
    gl.deleteTexture(yTexture);

    gl.activeTexture(gl.TEXTURE1);
    gl.bindTexture(gl.TEXTURE_2D, null);
    gl.deleteTexture(uTexture);

    gl.activeTexture(gl.TEXTURE2);
    gl.bindTexture(gl.TEXTURE_2D, null);
    gl.deleteTexture(vTexture);
}

function createTexture(gl, data, width, height) {
    const texture = gl.createTexture();
    gl.bindTexture(gl.TEXTURE_2D, texture);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    gl.texImage2D(
        gl.TEXTURE_2D, 
        0, 
        gl.LUMINANCE, 
        width, 
        height, 
        0, 
        gl.LUMINANCE, 
        gl.UNSIGNED_BYTE, 
        new Uint8Array(data),
    );
    return texture;
}

init();

const socket = new WebSocket("ws://127.0.0.1:8081");
// Connection opened event
socket.addEventListener('open', (event) => {
    console.log('WebSocket connection opened:', event);
});

// Connection closed event
socket.addEventListener('close', (event) => {
    console.log('WebSocket connection closed:', event);
});

// Error event
socket.addEventListener('error', (event) => {
    console.error('WebSocket error:', event);
});

var y = [];
var u = [];
var v = [];

var width = 512;
var height = 512;
const y_len = width * height;
const uv_len = (width / 2) * (height / 2);

// Message received event
socket.addEventListener('message', (event) => {
    Promise.all([event.data.arrayBuffer()])
    .then(
        ([data]) => {
            render(
                refs.gl, 
                refs.program, 
                data.slice(0, y_len), 
                data.slice(y_len, y_len + uv_len), 
                data.slice(y_len + uv_len, y_len + uv_len + uv_len), 
                width, 
                height
            );
        },
        (error) => {
            console.log(error);
        }
    );
    
});

/*const xhr=new XMLHttpRequest();
xhr.open('GET','/static/letter-f.yuv');
xhr.responseType='arraybuffer';
xhr.onload=()=>{
    console.log('got image');
    const data=xhr.response;
    var len = data.byteLength;
    var width = 512;
    var height = 512;
    var y_len = width * height;
    var u_len = y_len / 4;
    var y = data.slice(0, y_len);
    var u = data.slice(y_len, y_len + u_len);
    var v = data.slice(y_len + u_len, y_len + u_len + u_len);

    console.log("len: " + len + " y_len: " + y_len + " u_len: " + u_len);
    render(refs.gl, refs.program, y, u, v, width, height);
}
xhr.send();*/
