use std::*;
use byteorder::{ WriteBytesExt, BigEndian };
use packed_simd;
use encoder;


pub trait Comparator<Encoder: encoder::Encoder> {
	const BSIZE_W: usize;
	const BSIZE_H: usize;
	fn update( &mut Vec<u8>, &mut Encoder, &mut [u8], &[u8], usize, usize ) -> io::Result<u16>;
}

pub struct BlockComparator<Encoder: encoder::Encoder> {
	phantom: marker::PhantomData<Encoder>,
}

impl<Encoder: encoder::Encoder> Comparator<Encoder> for BlockComparator<Encoder> {
	const BSIZE_W: usize = 64;
	const BSIZE_H: usize = 64;

	fn update( out: &mut Vec<u8>, encoder: &mut Encoder, prev: &mut [u8], next: &[u8], w: usize, h: usize ) -> io::Result<u16> {
		let mut n_rects = 0;
		let mut by = 0;
		while by < h {
			let mut bx = 0;
			while bx < w {
				let mut upper = packed_simd::u64x2::new( bx as u64, by as u64 );
				let mut lower = upper + packed_simd::u64x2::new( Self::BSIZE_W as u64, Self::BSIZE_H as u64 );
				for y in by .. cmp::min( by + Self::BSIZE_H, h ) {
					for x in bx .. cmp::min( bx + Self::BSIZE_W, w ) {
						let p = unsafe { *(prev.as_ptr() as *const u32).add( w * y + x ) };
						let q = unsafe { *(next.as_ptr() as *const u32).add( w * y + x ) } & 0x00ffffff;
						if p != q {
							let xy = packed_simd::u64x2::new( x as u64, y as u64 );
							lower = lower.min( xy );
							upper = upper.max( xy + 1 );
							unsafe { *(prev.as_mut_ptr() as *mut u32).add( w * y + x ) = q };
						}
					}
				}
				if lower.lt( upper ).all() {
					let x0 = lower.extract( 0 ) as usize;
					let y0 = lower.extract( 1 ) as usize;
					let x1 = upper.extract( 0 ) as usize;
					let y1 = upper.extract( 1 ) as usize;
					out.write_u16::<BigEndian>( x0 as u16 )?;
					out.write_u16::<BigEndian>( y0 as u16 )?;
					out.write_u16::<BigEndian>( (x1 - x0) as u16 )?;
					out.write_u16::<BigEndian>( (y1 - y0) as u16 )?;
					out.write_u32::<BigEndian>( Encoder::ID )?;
					encoder.encode( out, next, w, h, x0, y0, x1, y1 );
					n_rects += 1;
				}
				bx += Self::BSIZE_W;
			}
			by += Self::BSIZE_H;
		}
		Ok( n_rects )
	}
}

pub struct StripComparator<Encoder: encoder::Encoder> {
	phantom: marker::PhantomData<Encoder>,
}

impl<Encoder: encoder::Encoder> Comparator<Encoder> for StripComparator<Encoder> {
	const BSIZE_W: usize =  64;
	const BSIZE_H: usize = 128;

	fn update( out: &mut Vec<u8>, encoder: &mut Encoder, prev: &mut [u8], next: &[u8], w: usize, h: usize ) -> io::Result<u16> {
		let mut n_rects = 0;
		let mut bx = 0;
		while bx < w {
			let mut y = 0;
			while y < h {
				'exit: while y < h {
					let s_prev = unsafe { (prev.as_ptr() as *const u32).add( w * y ) };
					let s_next = unsafe { (next.as_ptr() as *const u32).add( w * y ) };
					for x in bx .. cmp::min( bx + Self::BSIZE_W, w ) {
						let p = unsafe { *s_prev.add( x ) };
						let q = unsafe { *s_next.add( x ) } & 0x00ffffff;
						if p != q {
							break 'exit;
						}
					}
					y += 1;
				}
				let y0 = y;

				let mut n = 0;
				let mut x0 = bx + Self::BSIZE_W;
				let mut x1 = bx;
				while y < cmp::min( y0 + Self::BSIZE_H, h ) {
					let mut unchanged = true;
					let s_prev = unsafe { (prev.as_ptr() as *mut   u32).add( w * y ) };
					let s_next = unsafe { (next.as_ptr() as *const u32).add( w * y ) };
					for x in bx .. cmp::min( bx + Self::BSIZE_W, w ) {
						let p = unsafe { *s_prev.add( x ) };
						let q = unsafe { *s_next.add( x ) } & 0x00ffffff;
						if p != q {
							unchanged = false;
							x0 = cmp::min( x0, x );
							x1 = cmp::max( x1, x + 1 );
							unsafe { *s_prev.add( x ) = q };
						}
					}
					if unchanged {
						if n >= 8 {
							break;
						}
						n += 1;
					}
					else {
						n = 0;
					}
					y += 1;
				}
				let y1 = y - n;

				if y0 < y1 {
					out.write_u16::<BigEndian>( x0 as u16 )?;
					out.write_u16::<BigEndian>( y0 as u16 )?;
					out.write_u16::<BigEndian>( (x1 - x0) as u16 )?;
					out.write_u16::<BigEndian>( (y1 - y0) as u16 )?;
					out.write_u32::<BigEndian>( Encoder::ID )?;
					encoder.encode( out, next, w, h, x0, y0, x1, y1 );
					n_rects += 1;
				}
			}
			bx += Self::BSIZE_W;
		}
		Ok( n_rects )
	}
}
