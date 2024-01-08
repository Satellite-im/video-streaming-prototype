// shamelessly stolen from here: https://github.com/hanguk0726/Avatar-Vision/blob/main/rust/src/tools/image_processing.rs

pub const Y_SCALE: [[f32; 3]; 3] = [
    [0.2578125, 0.50390625, 0.09765625],
    [0.299, 0.587, 0.114],
    [0.183, 0.614, 0.062],
];
pub const Y_OFFSET: [f32; 3] = [16.0, 0.0, 16.0];

pub const U_SCALE: [[f32; 3]; 3] = [
    [-0.1484375, -0.2890625, 0.4375],
    [-0.169, -0.331, 0.500],
    [-0.101, -0.339, 0.439],
];
const U_OFFSET: [f32; 3] = [128.0, 128.0, 128.0];

pub const V_SCALE: [[f32; 3]; 3] = [
    [0.4375, -0.3671875, -0.0703125],
    [0.500, -0.419, -0.081],
    [0.439, -0.399, -0.040],
];
const V_OFFSET: [f32; 3] = [128.0, 128.0, 128.0];

#[derive(Debug, Clone, Copy)]
pub enum ColorScale {
    // coeffecients taken from https://github.com/hanguk0726/Avatar-Vision/blob/main/rust/src/tools/image_processing.rs
    Av,
    // full scale: https://web.archive.org/web/20180423091842/http://www.equasys.de/colorconversion.html
    Full,
    // HdTv scale: // https://web.archive.org/web/20180423091842/http://www.equasys.de/colorconversion.html
    HdTv,
}

impl ColorScale {
    pub fn to_idx(self) -> usize {
        match self {
            ColorScale::Av => 0,
            ColorScale::Full => 1,
            ColorScale::HdTv => 2,
        }
    }
}

pub struct YUV420Buf {
    pub data: Vec<u8>,
    pub width: usize,
    pub height: usize,
}

impl av_data::frame::FrameBuffer for YUV420Buf {
    fn linesize(&self, idx: usize) -> Result<usize, av_data::frame::FrameError> {
        match idx {
            0 => Ok(self.width),
            1 | 2 => Ok(self.width / 2),
            _ => Err(av_data::frame::FrameError::InvalidIndex),
        }
    }

    fn count(&self) -> usize {
        3
    }

    fn as_slice_inner(&self, idx: usize) -> Result<&[u8], av_data::frame::FrameError> {
        let base_u = self.width * self.height;
        let base_v = base_u + (base_u / 4);
        match idx {
            0 => Ok(&self.data[0..self.width * self.height]),
            1 => Ok(&self.data[base_u..base_v]),
            2 => Ok(&self.data[base_v..]),
            _ => Err(av_data::frame::FrameError::InvalidIndex),
        }
    }

    fn as_mut_slice_inner(&mut self, idx: usize) -> Result<&mut [u8], av_data::frame::FrameError> {
        let base_u = self.width * self.height;
        let base_v = base_u + (base_u / 4);
        match idx {
            0 => Ok(&mut self.data[0..self.width * self.height]),
            1 => Ok(&mut self.data[base_u..base_v]),
            2 => Ok(&mut self.data[base_v..]),
            _ => Err(av_data::frame::FrameError::InvalidIndex),
        }
    }
}

// u and v are calculated by averaging a 4-pixel square
pub fn rgb_to_yuv420(rgb: &[u8],  width: usize, height: usize, input_width: usize, input_height: usize, color_scale: ColorScale) -> Vec<u8> {
    let size = (3 * width * height) / 2;
    let mut yuv = vec![0; size];

    let u_base = width * height;
    let v_base = u_base + u_base / 4;
    let half_width = width / 2;

    // assumes input height and width are >= output height and width
    let width_diff = input_width - width;
    let height_diff = input_height - height;
    let width_margin = width_diff / 2;
    let height_margin = height_diff / 2;

    // y is full size, u, v is quarter size
    let pixel = |x: usize, y: usize| -> (f32, f32, f32) {
        let x = x + width_margin;
        let y = y + height_margin;
        // two dim to single dim
        let base_pos = (x + y * input_width) * 3;
        (
            rgb[base_pos] as f32,
            rgb[base_pos + 1] as f32,
            rgb[base_pos + 2] as f32,
        )
    };

    let color_scale_idx = color_scale.to_idx();
    let y_scale: &[f32; 3] = &Y_SCALE[color_scale_idx];
    let u_scale: &[f32; 3] = &U_SCALE[color_scale_idx];
    let v_scale: &[f32; 3] = &V_SCALE[color_scale_idx];

    let y_offset = Y_OFFSET[color_scale_idx];
    let u_offset = U_OFFSET[color_scale_idx];
    let v_offset = V_OFFSET[color_scale_idx];

    let write_y = |yuv: &mut [u8], x: usize, y: usize, rgb: (f32, f32, f32)| {
        yuv[x + y * width] =
            (y_scale[0] * rgb.0 + y_scale[1] * rgb.1 + y_scale[2] * rgb.2 + y_offset) as u8;
    };

    let write_u = |yuv: &mut [u8], x: usize, y: usize, rgb: (f32, f32, f32)| {
        yuv[u_base + x + y * half_width] =
            (u_scale[0] * rgb.0 + u_scale[1] * rgb.1 + u_scale[2] * rgb.2 + u_offset) as u8;
    };

    let write_v = |yuv: &mut [u8], x: usize, y: usize, rgb: (f32, f32, f32)| {
        yuv[v_base + x + y * half_width] =
            (v_scale[0] * rgb.0 + v_scale[1] * rgb.1 + v_scale[2] * rgb.2 + v_offset) as u8;
    };
    for i in 0..width / 2 {
        for j in 0..height / 2 {
            let px = i * 2;
            let py = j * 2;
            let pix0x0 = pixel(px, py);
            let pix0x1 = pixel(px, py + 1);
            let pix1x0 = pixel(px + 1, py);
            let pix1x1 = pixel(px + 1, py + 1);
            let avg_pix = (
                (pix0x0.0 as u32 + pix0x1.0 as u32 + pix1x0.0 as u32 + pix1x1.0 as u32) as f32
                    / 4.0,
                (pix0x0.1 as u32 + pix0x1.1 as u32 + pix1x0.1 as u32 + pix1x1.1 as u32) as f32
                    / 4.0,
                (pix0x0.2 as u32 + pix0x1.2 as u32 + pix1x0.2 as u32 + pix1x1.2 as u32) as f32
                    / 4.0,
            );
            write_y(&mut yuv[..], px, py, pix0x0);
            write_y(&mut yuv[..], px, py + 1, pix0x1);
            write_y(&mut yuv[..], px + 1, py, pix1x0);
            write_y(&mut yuv[..], px + 1, py + 1, pix1x1);
            write_u(&mut yuv[..], i, j, avg_pix);
            write_v(&mut yuv[..], i, j, avg_pix);
        }
    }
    yuv
}
