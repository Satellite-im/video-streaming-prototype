from bottle import route, run, static_file, template

@route('/static/letter-f.yuv')
def letter_f():
	return static_file("letter-f.yuv", root="./static")

@route('/static/style.css')
def get_style():
	return static_file("style.css", root='./static')

@route('/static/webgl-yuv.js')
def get_webgl_yuv():
	return static_file("webgl_yuv.js", root='./static')

@route('/static/webgl-rgb.js')
def get_webgl_rgb():
	return static_file("webgl_rgb.js", root='./static')

@route('/')
def hello():
	return template('index.html')

@route('/yuv-image')
def yuv_image():
	return template('views/yuv-image.html')

@route('/yuv-stream')
def yuv_stream():
	return template('views/yuv-stream.html')

@route('/rgb-stream')
def rgb_stream():
	return template('views/rgb-stream.html')

run(host='localhost', port=8080, debug=True)
