# Video Viewer
This python web app serves as a prototype for video streaming. It serves a web page that receives AV1 encoded video via a websocket connection and uses webgl2/glsl to render the video in a canvas. if this works, the javascript and GLSL can be re-used in a Dioxus desktop app.

Run as follows: `python3 server.py`

# YUV Image
This page tests that a single image can be decoded. Use the rust program found in src/bin/image_converter to create a properly formatted yuv image. The program expects the image to be a 512x512 pixel image. use ffmpeg to resize an image as follows: `ffmpeg -i image.jpg -s <width>x<height> image-resized.jpg`. Changing the file extension will convert between formats too. 

The test uses a file called `letter-f.yuv` but you can rename it. the letter F was used in the example because you will know if the image is rotated. 