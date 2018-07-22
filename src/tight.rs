use miniz_oxide::deflate;


pub struct TightEncoder {
	buffer: Vec<u8>,
	compressor: deflate::core::CompressorOxide,
}

impl TightEncoder {
	pub fn new( pixels: usize ) -> Self {
		TightEncoder{
			buffer: Vec::with_capacity( pixels * 3 ),
			compressor: deflate::core::CompressorOxide::new(
				deflate::core::deflate_flags::TDEFL_WRITE_ZLIB_HEADER | deflate::core::deflate_flags::TDEFL_GREEDY_PARSING_FLAG | 1,
			),
		}
	}

	pub fn encode( &mut self, out: &mut Vec<u8>, screen: &[u8], w: usize, h: usize, x0: usize, y0: usize, x1: usize, y1: usize ) {
		use std::simd::{ u8x4, i16x4 };
		assert!( x0 < x1 && x1 <= w );
		assert!( y0 < y1 && y1 <= h );
		assert!( screen.len() == w * h * 4 );
		assert!( self.buffer.capacity() >= (x1 - x0) * (y1 - y0) * 3 );

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
		self.buffer.clear();
		unsafe {
			let screen_u8x4 = screen.as_ptr() as *const u8x4;
			/* y == y0 */ {
				/* x == x0 */ {
					let dst = *screen_u8x4.add( w * y0 + x0 );
					self.buffer.push( dst.extract( 2 ) );
					self.buffer.push( dst.extract( 1 ) );
					self.buffer.push( dst.extract( 0 ) );
				}
				for x in x0 + 1 .. x1 {
					let v00 = *screen_u8x4.add( w * y0 + (x - 0) );
					let v10 = *screen_u8x4.add( w * y0 + (x - 1) );
					let dst = v00 - v10;
					self.buffer.push( dst.extract( 2 ) );
					self.buffer.push( dst.extract( 1 ) );
					self.buffer.push( dst.extract( 0 ) );
				}
			}
			for y in y0 + 1 .. y1 {
				let s00 = screen_u8x4.add( w * (y - 0) - 0 );
				let s01 = screen_u8x4.add( w * (y - 1) - 0 );
				let s10 = screen_u8x4.add( w * (y - 0) - 1 );
				let s11 = screen_u8x4.add( w * (y - 1) - 1 );
				/* x == x0 */ {
					let v00 = *s00.add( x0 );
					let v01 = *s01.add( x0 );
					let dst = v00 - v01;
					self.buffer.push( dst.extract( 2 ) );
					self.buffer.push( dst.extract( 1 ) );
					self.buffer.push( dst.extract( 0 ) );
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
					let dst = v00 - u8x4::from( prd );
					self.buffer.push( dst.extract( 2 ) );
					self.buffer.push( dst.extract( 1 ) );
					self.buffer.push( dst.extract( 0 ) );
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
			let data_index = out.len();
			let capacity = out.capacity();
			unsafe { out.set_len( capacity ) };
			let (_, _, size) = deflate::core::compress(
				&mut self.compressor, &self.buffer, &mut out[data_index..], deflate::core::TDEFLFlush::Sync
			);
			unsafe { out.set_len( data_index + size ) };
			assert!( size < 1 << 22 );
			out[len0_index] = 0x80 | ( size        & 0x7f) as u8;
			out[len1_index] = 0x80 | ((size >>  7) & 0x7f) as u8;
			out[len2_index] =         (size >> 14)         as u8;
		}
	}
}
