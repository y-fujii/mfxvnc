use miniz_oxide::deflate;
use packed_simd::FromCast;


pub struct TightEncoder {
	buffer: Vec<u8>,
	compressor: deflate::core::CompressorOxide,
	first: bool,
}

impl TightEncoder {
	pub fn new( pixels: usize ) -> Self {
		TightEncoder{
			buffer: Vec::with_capacity( pixels * 3 + 1 ),
			compressor: deflate::core::CompressorOxide::new( 1 | deflate::core::deflate_flags::TDEFL_GREEDY_PARSING_FLAG ),
			first: true,
		}
	}

	pub fn encode( &mut self, out: &mut Vec<u8>, screen: &[u8], w: usize, h: usize, x0: usize, y0: usize, x1: usize, y1: usize ) {
		use packed_simd::{ u8x4, i16x4 };
		assert!( x0 < x1 && x1 <= w );
		assert!( y0 < y1 && y1 <= h );
		assert!( screen.len() == w * h * 4 );
		assert!( self.buffer.capacity() >= (x1 - x0) * (y1 - y0) * 3 + 1 );

		/*
		self.buffer.clear();
		unsafe {
			let screen_u8x4 = screen.as_ptr() as *const u8x4;
			for y in y0 .. y1 {
				for x in x0 .. x1 {
					let dst = *screen_u8x4.add( w * y + x );
					self.buffer.push( dst.extract( 2 ) );
					self.buffer.push( dst.extract( 1 ) );
					self.buffer.push( dst.extract( 0 ) );
				}
			}
		}
		*/
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

		out.push( 0b0100_0000 ); // compression control.
		out.push( 2 ); // filter type: gradient.
		/*
		out.push( 0 ); // compression control.
		*/
		if self.buffer.len() < 12 {
			out.extend_from_slice( &self.buffer );
		}
		else {
			let len0_index = out.len();
			out.push( 0 );
			let len1_index = out.len();
			out.push( 0 );
			let len2_index = out.len();
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
				&mut self.compressor, &self.buffer, &mut out[defl_index..], deflate::core::TDEFLFlush::Sync
			);
			unsafe { out.set_len( defl_index + defl_len ) };

			let zlib_len = out.len() - zlib_index;
			assert!( zlib_len < 1 << 22 );
			out[len0_index] = 0x80 | ( zlib_len        & 0x7f) as u8;
			out[len1_index] = 0x80 | ((zlib_len >>  7) & 0x7f) as u8;
			out[len2_index] =         (zlib_len >> 14)         as u8;
		}
	}
}
