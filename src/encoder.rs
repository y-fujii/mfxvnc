use miniz_oxide::deflate;
use packed_simd::{i16x4, i32x4, i8x4, shuffle, u8x4, FromBits, FromCast};
use rand;
use std::*;

pub trait Encoder {
    fn new() -> Self;
    fn encode(&mut self, _: &mut Vec<u8>, _: &[u32], _: usize, _: usize, _: usize);
}

pub struct RandomColorEncoder;

impl Encoder for RandomColorEncoder {
    fn new() -> Self {
        RandomColorEncoder
    }

    fn encode(&mut self, out: &mut Vec<u8>, _: &[u32], _: usize, _: usize, _: usize) {
        out.extend(&[
            0,
            0,
            0,
            7,           // encoding type: Tight.
            0b1000_0000, // compression control: fill.
            rand::random::<u8>(),
            rand::random::<u8>(),
            rand::random::<u8>(),
        ]);
    }
}

pub struct RawEncoder;

impl Encoder for RawEncoder {
    fn new() -> Self {
        RawEncoder
    }

    fn encode(&mut self, out: &mut Vec<u8>, screen: &[u32], stride: usize, w: usize, h: usize) {
        out.extend(&[0, 0, 0, 0]); // encoding type: RAW.
        let size = 4 * w * h;
        out.reserve(size);
        unsafe {
            let mut src_u32 = screen.as_ptr();
            let mut dst_u32 = out.as_mut_ptr().add(out.len()) as *mut u32;
            for _ in 0..h {
                ptr::copy_nonoverlapping(src_u32, dst_u32, w);
                src_u32 = src_u32.add(stride);
                dst_u32 = dst_u32.add(w);
            }
            let out_len = out.len();
            out.set_len(out_len + size);
        }
    }
}

pub struct TightCompressor {
    compressor: deflate::core::CompressorOxide,
    first: bool,
}

impl TightCompressor {
    pub fn new() -> Self {
        TightCompressor {
            compressor: deflate::core::CompressorOxide::new(
                1 | deflate::core::deflate_flags::TDEFL_GREEDY_PARSING_FLAG,
            ),
            first: true,
        }
    }

    pub fn compress(&mut self, src: &[u8], out: &mut Vec<u8>, stream: u8, filter: u8) {
        out.extend(&[
            0,
            0,
            0,
            7,                           // encoding type: Tight.
            0b0100_0000 | (stream << 4), // compression control.
            filter,                      // filter type: gradient.
        ]);

        if src.len() < 12 {
            out.extend_from_slice(src);
        } else {
            let len_index = out.len();
            out.extend(&[0, 0, 0]);

            let zlib_index = out.len();
            if self.first {
                out.extend(&[0x78, 0x01]);
                self.first = false;
            }

            let defl_index = out.len();
            let capacity = out.capacity();
            unsafe { out.set_len(capacity) };
            let (_, _, defl_len) = deflate::core::compress(
                &mut self.compressor,
                src,
                &mut out[defl_index..],
                deflate::core::TDEFLFlush::Sync,
            );
            unsafe { out.set_len(defl_index + defl_len) };

            let zlib_len = out.len() - zlib_index;
            assert!(zlib_len < 1 << 22);
            out[len_index + 0] = 0x80 | (zlib_len & 0x7f) as u8;
            out[len_index + 1] = 0x80 | ((zlib_len >> 7) & 0x7f) as u8;
            out[len_index + 2] = (zlib_len >> 14) as u8;
        }
    }
}

pub struct TightRawEncoder {
    buffer: Vec<u8>,
    compressor: TightCompressor,
}

impl Encoder for TightRawEncoder {
    fn new() -> Self {
        TightRawEncoder {
            buffer: Vec::new(),
            compressor: TightCompressor::new(),
        }
    }

    fn encode(&mut self, out: &mut Vec<u8>, screen: &[u32], stride: usize, w: usize, h: usize) {
        let len = 3 * w * h;
        if self.buffer.capacity() < len + 1 {
            self.buffer = Vec::with_capacity(len + 1);
        }

        let screen = screen.as_ptr() as *const u8x4;
        unsafe {
            self.buffer.set_len(len);
            let mut buffer = self.buffer.as_mut_ptr();
            for sy in (0..stride * h).step_by(stride) {
                let s00 = screen.add(sy);
                for x in 0..w {
                    let dst = *s00.add(x);
                    ptr::write_unaligned(buffer as *mut u8x4, shuffle!(dst, [2, 1, 0, 3]));
                    buffer = buffer.add(3);
                }
            }
        }

        self.compressor.compress(&self.buffer, out, 0, 0);
    }
}

pub struct TightGradientEncoder {
    buffer: Vec<u8>,
    compressor: TightCompressor,
}

impl Encoder for TightGradientEncoder {
    fn new() -> Self {
        TightGradientEncoder {
            buffer: Vec::new(),
            compressor: TightCompressor::new(),
        }
    }

    fn encode(&mut self, out: &mut Vec<u8>, screen: &[u32], stride: usize, w: usize, h: usize) {
        let len = 3 * w * h;
        if self.buffer.capacity() < len + 1 {
            self.buffer = Vec::with_capacity(len + 1);
        }

        let screen = screen.as_ptr() as *const u8x4;
        unsafe {
            self.buffer.set_len(len);
            let mut buffer = self.buffer.as_mut_ptr();
            let s00 = screen;
            let s01 = screen.sub(stride);
            /* y == y0 */
            {
                let mut v10 = u8x4::splat(0);
                for x in 0..w {
                    let v00 = *s00.add(x);
                    let dst = v00 - v10;
                    ptr::write_unaligned(buffer as *mut u8x4, shuffle!(dst, [2, 1, 0, 3]));
                    buffer = buffer.add(3);
                    v10 = v00;
                }
            }
            for sy in (stride..stride * h).step_by(stride) {
                let s00 = s00.add(sy);
                let s01 = s01.add(sy);
                let mut dwy = i16x4::splat(0);
                for x in 0..w {
                    let v00 = *s00.add(x);
                    let v01 = *s01.add(x);
                    let w00 = i16x4::from(v00);
                    let w01 = i16x4::from(v01);
                    let prd = (w01 + dwy).max(i16x4::splat(0)).min(i16x4::splat(255));
                    let dst = v00 - u8x4::from_cast(prd);
                    ptr::write_unaligned(buffer as *mut u8x4, shuffle!(dst, [2, 1, 0, 3]));
                    buffer = buffer.add(3);
                    dwy = w00 - w01;
                }
            }
        }

        self.compressor.compress(&mut self.buffer, out, 0, 2);
    }
}

pub struct TightAdaptiveEncoder {
    buffer_raw: Vec<u8>,
    buffer_lin: Vec<u8>,
    compressor_raw: TightCompressor,
    compressor_lin: TightCompressor,
}

impl Encoder for TightAdaptiveEncoder {
    fn new() -> Self {
        TightAdaptiveEncoder {
            buffer_raw: Vec::new(),
            buffer_lin: Vec::new(),
            compressor_raw: TightCompressor::new(),
            compressor_lin: TightCompressor::new(),
        }
    }

    fn encode(&mut self, out: &mut Vec<u8>, screen: &[u32], stride: usize, w: usize, h: usize) {
        let len = 3 * w * h;
        if self.buffer_raw.capacity() < len + 1 {
            self.buffer_raw = Vec::with_capacity(len + 1);
        }
        if self.buffer_lin.capacity() < len + 1 {
            self.buffer_lin = Vec::with_capacity(len + 1);
        }

        let screen = screen.as_ptr() as *const u8x4;
        let mut sum_l1 = i32x4::splat(0);
        let mut n_matches = 0;
        unsafe {
            self.buffer_raw.set_len(len);
            self.buffer_lin.set_len(len);
            let mut buffer_raw = self.buffer_raw.as_mut_ptr();
            let mut buffer_lin = self.buffer_lin.as_mut_ptr();
            let s00 = screen;
            let s01 = screen.sub(stride);

            /* y == y0 */
            {
                let mut v10 = u8x4::splat(0);
                for x in 0..w {
                    let v00 = *s00.add(x);
                    let dst = v00 - v10;

                    ptr::write_unaligned(buffer_raw as *mut u8x4, shuffle!(v00, [2, 1, 0, 3]));
                    ptr::write_unaligned(buffer_lin as *mut u8x4, shuffle!(dst, [2, 1, 0, 3]));
                    buffer_raw = buffer_raw.add(3);
                    buffer_lin = buffer_lin.add(3);

                    let idst = i8x4::from_bits(dst);
                    sum_l1 += i32x4::from(i8x4::max(-idst, idst));
                    if v00 == v10 {
                        n_matches += 1;
                    }

                    v10 = v00;
                }
            }

            for sy in (stride..stride * h).step_by(stride) {
                let s00 = s00.add(sy);
                let s01 = s01.add(sy);
                let mut w10 = i16x4::splat(0);
                let mut w11 = i16x4::splat(0);
                for x in 0..w {
                    let v00 = *s00.add(x);
                    let v01 = *s01.add(x);
                    let w00 = i16x4::from(v00);
                    let w01 = i16x4::from(v01);
                    let prd = (w01 + w10 - w11).max(i16x4::splat(0)).min(i16x4::splat(255));
                    let dst = v00 - u8x4::from_cast(prd);

                    ptr::write_unaligned(buffer_raw as *mut u8x4, shuffle!(v00, [2, 1, 0, 3]));
                    ptr::write_unaligned(buffer_lin as *mut u8x4, shuffle!(dst, [2, 1, 0, 3]));
                    buffer_raw = buffer_raw.add(3);
                    buffer_lin = buffer_lin.add(3);

                    let idst = i8x4::from_bits(dst);
                    sum_l1 += i32x4::from(i8x4::max(-idst, idst));
                    if w00 == w01 || w00 == w10 {
                        n_matches += 1;
                    }

                    w10 = w00;
                    w11 = w01;
                }
            }
        }

        let n_pixels = w * h;
        let raw_ratio = (n_pixels - n_matches) as f64 / n_pixels as f64;
        let m = sum_l1.extract(0) as f64 + sum_l1.extract(1) as f64 + sum_l1.extract(2) as f64;
        let lin_ratio = if m == 0.0 {
            0.0
        } else {
            (1.0 / f64::ln(2.0) + 1.0) / 8.0 + (1.0 / 8.0) * f64::log2(m / (3 * n_pixels) as f64)
        };

        if raw_ratio < lin_ratio {
            self.compressor_raw.compress(&self.buffer_raw, out, 0, 0);
        } else {
            self.compressor_lin.compress(&self.buffer_lin, out, 1, 2);
        }
    }
}

pub struct TightJpegEncoder {
    compressor: *mut ffi::c_void,
}

impl Drop for TightJpegEncoder {
    fn drop(&mut self) {
        unsafe { jpeg_compressor_destroy(self.compressor) };
    }
}

impl Encoder for TightJpegEncoder {
    fn new() -> Self {
        let compressor = unsafe { jpeg_compressor_create() };
        TightJpegEncoder { compressor: compressor }
    }

    fn encode(&mut self, out: &mut Vec<u8>, screen: &[u32], stride: usize, w: usize, h: usize) {
        out.extend(&[
            0,
            0,
            0,
            7,           // encoding type: Tight.
            0b1001_0000, // compression control: JPEG.
        ]);

        let len_index = out.len();
        out.extend(&[0, 0, 0]);

        let jpeg_index = out.len();
        let jpeg_len;
        unsafe {
            jpeg_len = jpeg_compressor_compress(
                self.compressor,
                out.as_mut_ptr().add(jpeg_index),
                out.capacity() - jpeg_index,
                screen.as_ptr(),
                stride,
                w,
                h,
            );
            out.set_len(jpeg_index + jpeg_len);
        }

        assert!(jpeg_len < 1 << 22);
        out[len_index + 0] = 0x80 | (jpeg_len & 0x7f) as u8;
        out[len_index + 1] = 0x80 | ((jpeg_len >> 7) & 0x7f) as u8;
        out[len_index + 2] = (jpeg_len >> 14) as u8;
    }
}

extern {
    fn jpeg_compressor_create() -> *mut ffi::c_void;
    fn jpeg_compressor_destroy(this: *mut ffi::c_void);
    fn jpeg_compressor_compress(
        this: *mut ffi::c_void,
        dst: *mut u8,
        dst_size: usize,
        src: *const u32,
        stride: usize,
        w: usize,
        h: usize,
    ) -> usize;
}
