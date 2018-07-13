use flate2;


pub struct TightEncoder {
	buffer: Vec<u8>,
	compressor: flate2::Compress,
}

impl TightEncoder {
	pub fn new( pixels: usize ) -> Self {
		TightEncoder{
			buffer: Vec::with_capacity( pixels * 3 ),
			compressor: flate2::Compress::new( flate2::Compression::fast(), true ),
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
				/* x == x0 */ {
					let v00 = *screen_u8x4.add( w * (y - 0) + x0 );
					let v01 = *screen_u8x4.add( w * (y - 1) + x0 );
					let dst = v00 - v01;
					self.buffer.push( dst.extract( 2 ) );
					self.buffer.push( dst.extract( 1 ) );
					self.buffer.push( dst.extract( 0 ) );
				}
				for x in x0 + 1 .. x1 {
					let v00 = *screen_u8x4.add( w * (y - 0) + (x - 0) );
					let v01 = *screen_u8x4.add( w * (y - 1) + (x - 0) );
					let v10 = *screen_u8x4.add( w * (y - 0) + (x - 1) );
					let v11 = *screen_u8x4.add( w * (y - 1) + (x - 1) );
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
			self.compressor.compress_vec( &self.buffer, out, flate2::FlushCompress::Sync ).unwrap();
			let size = out.len() - data_index;
			assert!( size < 1 << 22 );
			out[len0_index] = 0x80 | ( size        & 0x7f) as u8;
			out[len1_index] = 0x80 | ((size >>  7) & 0x7f) as u8;
			out[len2_index] =         (size >> 14)         as u8;
		}
	}
}
