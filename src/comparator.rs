use std::*;
use packed_simd;


pub trait Comparator {
	const BSIZE_W: usize;
	const BSIZE_H: usize;
	fn compare<F: FnMut( usize, usize, usize, usize )>( &mut [u8], &[u8], usize, usize, usize, F );
}

pub struct BlockComparator;

impl Comparator for BlockComparator {
	const BSIZE_W: usize = 64;
	const BSIZE_H: usize = 64;

	fn compare<F: FnMut( usize, usize, usize, usize )>( prev: &mut [u8], next: &[u8], stride: usize, w: usize, h: usize, mut callback: F ) {
		let mut by = 0;
		while by < h {
			let mut bx = 0;
			while bx < w {
				let mut upper = packed_simd::u64x2::new( bx as u64, by as u64 );
				let mut lower = upper + packed_simd::u64x2::new( Self::BSIZE_W as u64, Self::BSIZE_H as u64 );
				for y in by .. cmp::min( by + Self::BSIZE_H, h ) {
					for x in bx .. cmp::min( bx + Self::BSIZE_W, w ) {
						let p = unsafe { *(prev.as_ptr() as *const u32).add( stride * y + x ) };
						let q = unsafe { *(next.as_ptr() as *const u32).add( stride * y + x ) } & 0x00ffffff;
						if p != q {
							let xy = packed_simd::u64x2::new( x as u64, y as u64 );
							lower = lower.min( xy );
							upper = upper.max( xy + 1 );
							unsafe { *(prev.as_mut_ptr() as *mut u32).add( stride * y + x ) = q };
						}
					}
				}
				if lower.lt( upper ).all() {
					let x0 = lower.extract( 0 ) as usize;
					let y0 = lower.extract( 1 ) as usize;
					let x1 = upper.extract( 0 ) as usize;
					let y1 = upper.extract( 1 ) as usize;
					callback( x0, y0, x1, y1 );
				}
				bx += Self::BSIZE_W;
			}
			by += Self::BSIZE_H;
		}
	}
}

pub struct StripComparator;

impl Comparator for StripComparator {
	const BSIZE_W: usize =  64;
	const BSIZE_H: usize = 128;

	fn compare<F: FnMut( usize, usize, usize, usize )>( prev: &mut [u8], next: &[u8], stride: usize, w: usize, h: usize, mut callback: F ) {
		let mut bx = 0;
		while bx < w {
			let mut y = 0;
			while y < h {
				'exit: while y < h {
					let s_prev = unsafe { (prev.as_ptr() as *const u32).add( stride * y ) };
					let s_next = unsafe { (next.as_ptr() as *const u32).add( stride * y ) };
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
					let s_prev = unsafe { (prev.as_ptr() as *mut   u32).add( stride * y ) };
					let s_next = unsafe { (next.as_ptr() as *const u32).add( stride * y ) };
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
					callback( x0, y0, x1, y1 );
				}
			}
			bx += Self::BSIZE_W;
		}
	}
}
