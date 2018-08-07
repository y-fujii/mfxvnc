use std::*;
use miniz_oxide::deflate;
use packed_simd::{ u8x4, i8x4, i16x4, i32x4, FromCast, FromBits };


pub trait Encoder {
	fn new( usize, usize ) -> Self;
	fn encode( &mut self, &mut Vec<u8>, *const u32, usize, usize, usize );
}

pub struct RawEncoder;

impl Encoder for RawEncoder {
	fn new( _: usize, _: usize ) -> Self {
		RawEncoder
	}

	fn encode( &mut self, out: &mut Vec<u8>, screen: *const u32, stride: usize, w: usize, h: usize ) {
		out.extend( &[ 0, 0, 0, 0 ] ); // encoding type: RAW.
		let size = w * h * 4;
		out.reserve( size );
		unsafe {
			let mut src_u32 = screen;
			let mut dst_u32 = out.as_mut_ptr().add( out.len() ) as *mut u32;
			for _ in 0 .. h {
				ptr::copy_nonoverlapping( src_u32, dst_u32, w );
				src_u32 = src_u32.add( stride );
				dst_u32 = dst_u32.add( w );
			}
			let out_len = out.len();
			out.set_len( out_len + size );
		}
	}
}

pub struct TightCompressor {
	compressor: deflate::core::CompressorOxide,
	first: bool,
}

impl TightCompressor {
	pub fn new() -> Self {
		TightCompressor{
			compressor: deflate::core::CompressorOxide::new( 1 | deflate::core::deflate_flags::TDEFL_GREEDY_PARSING_FLAG ),
			first: true,
		}
	}

	pub fn compress( &mut self, src: &[u8], out: &mut Vec<u8>, stream: u8, filter: u8 ) {
		out.extend( &[
			0, 0, 0, 7, // encoding type: Tight.
			0b0100_0000 | (stream << 4), // compression control.
			filter, // filter type: gradient.
		] );

		if src.len() < 12 {
			out.extend_from_slice( src );
		}
		else {
			let len_index = out.len();
			out.extend( &[ 0, 0, 0 ] );

			let zlib_index = out.len();
			if self.first {
				out.extend( &[ 0x78, 0x01 ] );
				self.first = false;
			}

			let defl_index = out.len();
			let capacity = out.capacity();
			unsafe { out.set_len( capacity ) };
			let (_, _, defl_len) = deflate::core::compress(
				&mut self.compressor, src, &mut out[defl_index..], deflate::core::TDEFLFlush::Sync
			);
			unsafe { out.set_len( defl_index + defl_len ) };

			let zlib_len = out.len() - zlib_index;
			assert!( zlib_len < 1 << 22 );
			out[len_index + 0] = 0x80 | ( zlib_len        & 0x7f) as u8;
			out[len_index + 1] = 0x80 | ((zlib_len >>  7) & 0x7f) as u8;
			out[len_index + 2] =         (zlib_len >> 14)         as u8;
		}
	}
}

pub struct TightRawEncoder {
	buffer: Vec<u8>,
	compressor: TightCompressor,
}

impl Encoder for TightRawEncoder {
	fn new( w: usize, h: usize ) -> Self {
		TightRawEncoder{
			buffer: Vec::with_capacity( 3 * w * h + 1 ),
			compressor: TightCompressor::new(),
		}
	}

	fn encode( &mut self, out: &mut Vec<u8>, screen: *const u32, stride: usize, w: usize, h: usize ) {
		unsafe {
			let screen = screen as *const u8x4;
			self.buffer.set_len( w * h * 3 );
			let mut buffer_index = 0;
			for y in 0 .. h {
				let s00 = screen.add( stride * y );
				for x in 0 .. w {
					let dst = *s00.add( x );
					u8x4::write_to_slice_unaligned_unchecked( shuffle!( dst, [2, 1, 0, 3] ), &mut self.buffer[buffer_index..] );
					buffer_index += 3;
				}
			}
		}

		self.compressor.compress( &mut self.buffer, out, 0, 0 );
	}
}

pub struct TightGradientEncoder {
	buffer: Vec<u8>,
	compressor: TightCompressor,
}

impl Encoder for TightGradientEncoder {
	fn new( w: usize, h: usize ) -> Self {
		TightGradientEncoder{
			buffer: Vec::with_capacity( 3 * w * h + 1 ),
			compressor: TightCompressor::new(),
		}
	}

	fn encode( &mut self, out: &mut Vec<u8>, screen: *const u32, stride: usize, w: usize, h: usize ) {
		unsafe {
			let screen = screen as *const u8x4;
			self.buffer.set_len( w * h * 3 );
			let mut buffer_index = 0;
			/* y == y0 */ {
				let s00 = screen.offset(  0 );
				let s10 = screen.offset( -1 );
				/* x == x0 */ {
					let dst = *s00.add( 0 );
					u8x4::write_to_slice_unaligned_unchecked( shuffle!( dst, [2, 1, 0, 3] ), &mut self.buffer[buffer_index..] );
					buffer_index += 3;
				}
				for x in 1 .. w {
					let dst = *s00.add( x ) - *s10.add( x );
					u8x4::write_to_slice_unaligned_unchecked( shuffle!( dst, [2, 1, 0, 3] ), &mut self.buffer[buffer_index..] );
					buffer_index += 3;
				}
			}
			for y in 1 .. h {
				let s00 = screen.add( stride * y ).sub(          0 );
				let s01 = screen.add( stride * y ).sub( stride + 0 );
				let s10 = screen.add( stride * y ).sub(          1 );
				let s11 = screen.add( stride * y ).sub( stride + 1 );
				/* x == x0 */ {
					let dst = *s00.add( 0 ) - *s01.add( 0 );
					u8x4::write_to_slice_unaligned_unchecked( shuffle!( dst, [2, 1, 0, 3] ), &mut self.buffer[buffer_index..] );
					buffer_index += 3;
				}
				for x in 1 .. w {
					let w01 = i16x4::from( *s01.add( x ) );
					let w10 = i16x4::from( *s10.add( x ) );
					let w11 = i16x4::from( *s11.add( x ) );
					let prd = (w01 + w10 - w11).max( i16x4::splat( 0 ) ).min( i16x4::splat( 255 ) );
					let dst = *s00.add( x ) - u8x4::from_cast( prd );
					u8x4::write_to_slice_unaligned_unchecked( shuffle!( dst, [2, 1, 0, 3] ), &mut self.buffer[buffer_index..] );
					buffer_index += 3;
				}
			}
		}

		self.compressor.compress( &mut self.buffer, out, 0, 2 );
	}
}

pub struct TightAdaptiveEncoder {
	buffer_raw: Vec<u8>,
	buffer_lin: Vec<u8>,
	compressor_raw: TightCompressor,
	compressor_lin: TightCompressor,
}

impl Encoder for TightAdaptiveEncoder {
	fn new( w: usize, h: usize ) -> Self {
		TightAdaptiveEncoder{
			buffer_raw: Vec::with_capacity( 3 * w * h + 1 ),
			buffer_lin: Vec::with_capacity( 3 * w * h + 1 ),
			compressor_raw: TightCompressor::new(),
			compressor_lin: TightCompressor::new(),
		}
	}

	fn encode( &mut self, out: &mut Vec<u8>, screen: *const u32, stride: usize, w: usize, h: usize ) {
		let n_pixels = w * h;
		let mut sum_l1 = i32x4::splat( 0 );
		let mut n_matches = 0;
		unsafe {
			let screen = screen as *const u8x4;
			self.buffer_raw.set_len( 3 * n_pixels );
			self.buffer_lin.set_len( 3 * n_pixels );
			let mut buffer_index = 0;
			/* y == y0 */ {
				let s00 = screen.offset(  0 );
				let s10 = screen.offset( -1 );
				/* x == x0 */ {
					let v00 = *s00.add( 0 );
					let dst = v00;

					u8x4::write_to_slice_unaligned_unchecked( shuffle!( v00, [2, 1, 0, 3] ), &mut self.buffer_raw[buffer_index..] );
					u8x4::write_to_slice_unaligned_unchecked( shuffle!( dst, [2, 1, 0, 3] ), &mut self.buffer_lin[buffer_index..] );
					buffer_index += 3;
				}
				for x in 1 .. w {
					let v00 = *s00.add( x );
					let v10 = *s10.add( x );
					let dst = v00 - v10;

					u8x4::write_to_slice_unaligned_unchecked( shuffle!( v00, [2, 1, 0, 3] ), &mut self.buffer_raw[buffer_index..] );
					u8x4::write_to_slice_unaligned_unchecked( shuffle!( dst, [2, 1, 0, 3] ), &mut self.buffer_lin[buffer_index..] );
					buffer_index += 3;

					let idst = i8x4::from_bits( dst );
					sum_l1 += i32x4::from( i8x4::max( -idst, idst ) );
					if v00 == v10 {
						n_matches += 1;
					}
				}
			}
			for y in 1 .. h {
				let s00 = screen.add( stride * y ).sub(          0 );
				let s01 = screen.add( stride * y ).sub( stride + 0 );
				let s10 = screen.add( stride * y ).sub(          1 );
				let s11 = screen.add( stride * y ).sub( stride + 1 );
				/* x == x0 */ {
					let v00 = *s00.add( 0 );
					let v01 = *s01.add( 0 );
					let dst = v00 - v01;

					u8x4::write_to_slice_unaligned_unchecked( shuffle!( v00, [2, 1, 0, 3] ), &mut self.buffer_raw[buffer_index..] );
					u8x4::write_to_slice_unaligned_unchecked( shuffle!( dst, [2, 1, 0, 3] ), &mut self.buffer_lin[buffer_index..] );
					buffer_index += 3;

					let idst = i8x4::from_bits( dst );
					sum_l1 += i32x4::from( i8x4::max( -idst, idst ) );
					if v00 == v01 {
						n_matches += 1;
					}
				}
				for x in 1 .. w {
					let v00 = *s00.add( x );
					let v01 = *s01.add( x );
					let v10 = *s10.add( x );
					let v11 = *s11.add( x );
					let w01 = i16x4::from( v01 );
					let w10 = i16x4::from( v10 );
					let w11 = i16x4::from( v11 );
					let prd = (w01 + w10 - w11).max( i16x4::splat( 0 ) ).min( i16x4::splat( 255 ) );
					let dst = v00 - u8x4::from_cast( prd );

					u8x4::write_to_slice_unaligned_unchecked( shuffle!( v00, [2, 1, 0, 3] ), &mut self.buffer_raw[buffer_index..] );
					u8x4::write_to_slice_unaligned_unchecked( shuffle!( dst, [2, 1, 0, 3] ), &mut self.buffer_lin[buffer_index..] );
					buffer_index += 3;

					let idst = i8x4::from_bits( dst );
					sum_l1 += i32x4::from( i8x4::max( -idst, idst ) );
					if v00 == v01 || v00 == v10 {
						n_matches += 1;
					}
				}
			}
		}

		let raw_ratio = (n_pixels - n_matches) as f64 / n_pixels as f64;
		let m = sum_l1.extract( 0 ) as f64 + sum_l1.extract( 1 ) as f64 + sum_l1.extract( 2 ) as f64;
		let lin_ratio = if m == 0.0 {
			0.0
		}
		else {
			(1.0 / f64::ln( 2.0 ) + 1.0) / 8.0 + (1.0 / 8.0) * f64::log2( m / (3 * n_pixels) as f64 )
		};

		if raw_ratio < lin_ratio {
			self.compressor_raw.compress( &mut self.buffer_raw, out, 0, 0 );
		}
		else {
			self.compressor_lin.compress( &mut self.buffer_lin, out, 1, 2 );
		}
	}
}

/*
#[link( name = "turbojpeg" )]
extern "C" {
	fn tjInitCompress() -> *mut os::raw::c_void;
	fn tjDestroy( _: *mut os::raw::c_void ) -> i32;
	fn tjCompress2( _: *mut os::raw::c_void, _: *const u8, _: i32, _: i32, _: i32, _: i32, _: *const *mut u8, _: *mut usize, _: i32, _: i32, _: i32 ) -> i32;
}

pub struct TightJpegEncoder {
	tj_handle: *mut os::raw::c_void,
}

impl Drop for TightJpegEncoder {
	fn drop( &mut self ) {
		unsafe { tjDestroy( self.tj_handle ) };
	}
}

impl Encoder for TightJpegEncoder {
	fn new( _: usize, _: usize ) -> Self {
		let handle = unsafe { tjInitCompress() };
		assert!( !handle.is_null() );
		TightJpegEncoder{
			tj_handle: handle,
		}
	}

	fn encode( &mut self, out: &mut Vec<u8>, screen: *const u32, stride: usize, w: usize, h: usize ) {
		out.extend( &[
			0, 0, 0, 7, // encoding type: Tight.
			0b1001_0000, // compression control: JPEG.
		] );

		let len_index = out.len();
		out.extend( &[ 0, 0, 0 ] );

		let jpeg_index = out.len();
		let mut jpeg_len = 0;
		unsafe {
			tjCompress2(
				self.tj_handle,
				screen as *const u8,
				w as i32,
				(4 * stride) as i32,
				h as i32,
				3, // TJPF_BGRX.
				&out.as_mut_ptr().add( jpeg_index ),
				&mut jpeg_len,
				0, // TJSAMP_444.
				92,
				1024, // TJFLAG_NOREALLOC.
			);
			out.set_len( jpeg_index + jpeg_len );
		}

		assert!( jpeg_len < 1 << 22 );
		out[len_index + 0] = 0x80 | ( jpeg_len        & 0x7f) as u8;
		out[len_index + 1] = 0x80 | ((jpeg_len >>  7) & 0x7f) as u8;
		out[len_index + 2] =         (jpeg_len >> 14)         as u8;
	}
}
*/
