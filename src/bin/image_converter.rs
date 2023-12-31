/// image_converter
/// usage: ./image_converter path/to/input_file.ext path/to/output_file.yuv
/// the input file can be any extension handled by image::DynamicImage. the output file
/// will be in the YUV4:2:0 format
/// conversion equations taken from here: https://web.archive.org/web/20180423091842/http://www.equasys.de/colorconversion.html
use image::DynamicImage;

fn main() {
    let mut iter = std::env::args().skip(1);
    // Load a PNG image
    let img_path = iter.next().unwrap();
    let img = image::open(img_path).expect("Failed to open image");

    // Convert the image to YUV420 format
    let planes = convert_to_yuv420(&img);
    println!("plane lengths");
    println!("y: {}", planes.y.len());
    println!("u: {}", planes.u.len());
    println!("v: {}", planes.v.len());

    // Save the YUV420 data to a file
    let output_path = iter.next().unwrap();
    save_yuv420_to_file(&planes, &output_path);

    println!("Conversion complete. YUV data saved to: {}", output_path);
}

struct Planes {
    y: Vec<u8>,
    u: Vec<u8>,
    v: Vec<u8>,
}

fn rgb_to_yuv(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let r = r as f32;
    let g = g as f32;
    let b = b as f32;

    let mut y = 0.299 * r + 0.587 * g + 0.114 * b + 0.0;
    let mut u = -0.169 * r - 0.331 * g + 0.500 * b + 128.0;
    let mut v = 0.500 * r - 0.419 * g - 0.081 * b + 128.0;

    y = clamp(y, 0.0, 255.0);
    u = clamp(u, 0.0, 255.0);
    v = clamp(v, 0.0, 255.0);

    (y, u, v)
}

fn convert_to_yuv420(img: &DynamicImage) -> Planes {
    let img = img.to_rgba8();
    let (width, height) = img.dimensions();
    println!("image dimensions: {:?}", img.dimensions());
    let mut y_plane = Vec::with_capacity((width * height) as usize);
    let mut u_plane = Vec::with_capacity((width * height / 4) as usize);
    let mut v_plane = Vec::with_capacity((width * height / 4) as usize);

    for (_row, _col, pixel) in img.enumerate_pixels() {
        let [r, g, b] = [pixel[0], pixel[1], pixel[2]];

        let (y, _, _) = rgb_to_yuv(r, g, b);

        println!("{r}, {g}, {b}");

        y_plane.push(y as u8);
    }

    for row in 0..(height / 2) {
        for col in 0..(width / 2) {
            let r = row * 2;
            let c = col * 2;

            let pixel00 = img.get_pixel(c, r);
            let pixel01 = img.get_pixel(c, r + 1);
            let pixel10 = img.get_pixel(c + 1, r);
            let pixel11 = img.get_pixel(c + 1, r + 1);

            let (_y00, u00, v00) = rgb_to_yuv(pixel00[0], pixel00[1], pixel00[2]);
            let (_y01, u01, v01) = rgb_to_yuv(pixel01[0], pixel01[1], pixel01[2]);
            let (_y10, u10, v10) = rgb_to_yuv(pixel10[0], pixel10[1], pixel10[2]);
            let (_y11, u11, v11) = rgb_to_yuv(pixel11[0], pixel11[1], pixel11[2]);

            let u = (u00 + u01 + u10 + u11) / 4.0;
            let v = (v00 + v01 + v10 + v11) / 4.0;
            u_plane.push(u as u8);
            v_plane.push(v as u8);
        }
    }

    Planes {
        y: y_plane,
        u: u_plane,
        v: v_plane,
    }
}

fn clamp(x: f32, min: f32, max: f32) -> f32 {
    if x < min {
        min
    } else if x > max {
        max
    } else {
        x
    }
}

// Function to save YUV420 data to a file
fn save_yuv420_to_file(planes: &Planes, path: &str) {
    use std::fs::File;
    use std::io::Write;

    let mut file = File::create(path).expect("Failed to create output file");

    // Write Y plane
    file.write_all(&planes.y).expect("Failed to write Y plane");

    // Write U and V planes
    file.write_all(&planes.u).expect("Failed to write U plane");

    file.write_all(&planes.v).expect("Failed to write V plane");
}
