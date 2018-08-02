use std::*;
use miniz_oxide::deflate;
use packed_simd::{ u8x4, i8x4, i16x4, i32x4, FromCast, FromBits };


pub trait Encoder {
	const ID: u32;
	fn new( usize ) -> Self;
	fn encode( &mut self, &mut Vec<u8>, &[u8], usize, usize, usize, usize, usize, usize );
}

pub struct RawEncoder;

impl Encoder for RawEncoder {
	const ID: u32 = 0;

	fn new( _: usize ) -> Self {
		RawEncoder
	}

	fn encode( &mut self, out: &mut Vec<u8>, screen: &[u8], w: usize, _h: usize, x0: usize, y0: usize, x1: usize, y1: usize ) {
		let size = (x1 - x0) * (y1 - y0) * 4;
		out.reserve( size );
		unsafe {
			let mut src_u32 = (screen.as_ptr() as *const u32).add( w * y0 + x0 );
			let mut dst_u32 = out.as_mut_ptr().add( out.len() ) as *mut u32;
			for _ in y0 .. y1 {
				ptr::copy_nonoverlapping( src_u32, dst_u32, x1 - x0 );
				src_u32 = src_u32.add( w );
				dst_u32 = dst_u32.add( x1 - x0 );
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
	const ID: u32 = 7;

	pub fn new() -> Self {
		TightCompressor{
			compressor: deflate::core::CompressorOxide::new( 1 | deflate::core::deflate_flags::TDEFL_GREEDY_PARSING_FLAG ),
			first: true,
		}
	}

	pub fn compress( &mut self, src: &[u8], out: &mut Vec<u8>, stream: u8, filter: u8 ) {
		out.push( 0b0100_0000 | (stream << 4) ); // compression control.
		out.push( filter ); // filter type: gradient.

		if src.len() < 12 {
			out.extend_from_slice( src );
		}
		else {
			let len_index = out.len();
			out.push( 0 );
			out.push( 0 );
			out.push( 0 );

			let zlib_index = out.len();
			if self.first {
				out.push( 0x78 );
				out.push( 0x01 );
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
	const ID: u32 = 7;

	fn new( pixels: usize ) -> Self {
		TightRawEncoder{
			buffer: Vec::with_capacity( pixels * 3 + 1 ),
			compressor: TightCompressor::new(),
		}
	}

	fn encode( &mut self, out: &mut Vec<u8>, screen: &[u8], w: usize, h: usize, x0: usize, y0: usize, x1: usize, y1: usize ) {
		assert!( x0 < x1 && x1 <= w );
		assert!( y0 < y1 && y1 <= h );
		assert!( screen.len() == w * h * 4 );
		assert!( self.buffer.capacity() >= (x1 - x0) * (y1 - y0) * 3 + 1 );

		unsafe {
			let screen_u8x4 = screen.as_ptr() as *const u8x4;
			self.buffer.set_len( (x1 - x0) * (y1 - y0) * 3 );
			let mut buffer_index = 0;
			let mut sy = w * y0;
			while sy < w * y1 {
				let s00 = screen_u8x4.add( sy );
				for x in x0 .. x1 {
					let dst = *s00.add( x );
					u8x4::write_to_slice_unaligned_unchecked( shuffle!( dst, [2, 1, 0, 3] ), &mut self.buffer[buffer_index..] );
					buffer_index += 3;
				}
				sy += w;
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
	const ID: u32 = 7;

	fn new( pixels: usize ) -> Self {
		TightGradientEncoder{
			buffer: Vec::with_capacity( pixels * 3 + 1 ),
			compressor: TightCompressor::new(),
		}
	}

	fn encode( &mut self, out: &mut Vec<u8>, screen: &[u8], w: usize, h: usize, x0: usize, y0: usize, x1: usize, y1: usize ) {
		assert!( x0 < x1 && x1 <= w );
		assert!( y0 < y1 && y1 <= h );
		assert!( screen.len() == w * h * 4 );
		assert!( self.buffer.capacity() >= (x1 - x0) * (y1 - y0) * 3 + 1 );

		unsafe {
			let screen_u8x4 = screen.as_ptr() as *const u8x4;
			self.buffer.set_len( (x1 - x0) * (y1 - y0) * 3 );
			let mut buffer_index = 0;
			/* y == y0 */ {
				let s00 = screen_u8x4.add( w * y0 - 0 );
				let s10 = screen_u8x4.add( w * y0 - 1 );
				/* x == x0 */ {
					let dst = *s00.add( x0 );
					u8x4::write_to_slice_unaligned_unchecked( shuffle!( dst, [2, 1, 0, 3] ), &mut self.buffer[buffer_index..] );
					buffer_index += 3;
				}
				for x in x0 + 1 .. x1 {
					let dst = *s00.add( x ) - *s10.add( x );
					u8x4::write_to_slice_unaligned_unchecked( shuffle!( dst, [2, 1, 0, 3] ), &mut self.buffer[buffer_index..] );
					buffer_index += 3;
				}
			}
			for y in y0 + 1 .. y1 {
				let s00 = screen_u8x4.add( w * y - (0 + 0) );
				let s01 = screen_u8x4.add( w * y - (w + 0) );
				let s10 = screen_u8x4.add( w * y - (0 + 1) );
				let s11 = screen_u8x4.add( w * y - (w + 1) );
				/* x == x0 */ {
					let dst = *s00.add( x0 ) - *s01.add( x0 );
					u8x4::write_to_slice_unaligned_unchecked( shuffle!( dst, [2, 1, 0, 3] ), &mut self.buffer[buffer_index..] );
					buffer_index += 3;
				}
				for x in x0 + 1 .. x1 {
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
	const ID: u32 = 7;

	fn new( pixels: usize ) -> Self {
		TightAdaptiveEncoder{
			buffer_raw: Vec::with_capacity( pixels * 3 + 1 ),
			buffer_lin: Vec::with_capacity( pixels * 3 + 1 ),
			compressor_raw: TightCompressor::new(),
			compressor_lin: TightCompressor::new(),
		}
	}

	fn encode( &mut self, out: &mut Vec<u8>, screen: &[u8], w: usize, h: usize, x0: usize, y0: usize, x1: usize, y1: usize ) {
		let n_pixels = (x1 - x0) * (y1 - y0);
		assert!( x0 < x1 && x1 <= w );
		assert!( y0 < y1 && y1 <= h );
		assert!( screen.len() == w * h * 4 );
		assert!( self.buffer_raw.capacity() >= n_pixels * 3 + 1 );
		assert!( self.buffer_lin.capacity() >= n_pixels * 3 + 1 );

		let mut sum_l1 = i32x4::splat( 0 );
		let mut n_matches = 0;
		unsafe {
			let screen_u8x4 = screen.as_ptr() as *const u8x4;
			self.buffer_raw.set_len( 3 * n_pixels );
			self.buffer_lin.set_len( 3 * n_pixels );
			let mut buffer_index = 0;
			/* y == y0 */ {
				let s00 = screen_u8x4.add( w * y0 - 0 );
				let s10 = screen_u8x4.add( w * y0 - 1 );
				/* x == x0 */ {
					let v00 = *s00.add( x0 );
					let dst = v00;

					u8x4::write_to_slice_unaligned_unchecked( shuffle!( v00, [2, 1, 0, 3] ), &mut self.buffer_raw[buffer_index..] );
					u8x4::write_to_slice_unaligned_unchecked( shuffle!( dst, [2, 1, 0, 3] ), &mut self.buffer_lin[buffer_index..] );
					buffer_index += 3;
				}
				for x in x0 + 1 .. x1 {
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
			for y in y0 + 1 .. y1 {
				let s00 = screen_u8x4.add( w * y - (0 + 0) );
				let s01 = screen_u8x4.add( w * y - (w + 0) );
				let s10 = screen_u8x4.add( w * y - (0 + 1) );
				let s11 = screen_u8x4.add( w * y - (w + 1) );
				/* x == x0 */ {
					let v00 = *s00.add( x0 );
					let v01 = *s01.add( x0 );
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
				for x in x0 + 1 .. x1 {
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
