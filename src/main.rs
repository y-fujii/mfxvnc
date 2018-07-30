#[macro_use]
extern crate packed_simd;
extern crate byteorder;
extern crate miniz_oxide;
extern crate scrap;
mod tight;
use std::*;
use std::io::{ Read, Write };
use byteorder::{ ByteOrder, ReadBytesExt, WriteBytesExt, BigEndian };


struct VncServer;

impl VncServer {
	const BSIZE_W: usize =  64;
	const BSIZE_H: usize = 128;

	fn listen<A: std::net::ToSocketAddrs>( addr: A ) -> io::Result<()> {
		let listener = net::TcpListener::bind( addr )?;
		for stream in listener.incoming() {
			let mut stream = stream?;
			stream.set_nodelay( true )?;
			if let Err( _ ) = Self::shake_hands( &mut stream ) {
				continue;
			}

			let reader = {
				let stream = stream.try_clone()?;
				thread::spawn( move || Self::read_loop( stream ) )
			};
			let w_result = Self::write_loop( stream );
			let r_result = reader.join().unwrap();
			w_result.and( r_result )?
		}
		Ok( () )
	}

	fn shake_hands( stream: &mut net::TcpStream ) -> io::Result<()> {
		// => protocol version.
		stream.write_all( b"RFB 003.008\n" )?;
		// <= protocol version.
		let mut buf = [0; 12];
		stream.read_exact( &mut buf )?;
		if buf != *b"RFB 003.008\n" {
			stream.write_all( b"\x00\x00\x00\x06error." )?;
			return Err( io::Error::new( io::ErrorKind::Other, "protocol version" ) );
		}

		// => security types.
		stream.write_all( &[1, 1] )?;
		// <= security type.
		if stream.read_u8()? != 1 {
			stream.write_all( b"\x00\x00\x00\x06error." )?;
			return Err( io::Error::new( io::ErrorKind::Other, "security type" ) );
		}

		// security result.
		stream.write_u32::<BigEndian>( 0 )?;

		// client init.
		stream.read_u8()?;

		// a server init message will be sent in write_loop().

		Ok( () )
	}

	fn read_loop( mut stream: net::TcpStream ) -> io::Result<()> {
		let mut buf = [0; 4096];
		loop {
			stream.read( &mut buf )?;
		}
	}

	fn write_loop( mut stream: net::TcpStream ) -> io::Result<()> {
		let mut encoder = tight::TightEncoder::new( Self::BSIZE_W * Self::BSIZE_H );
		let mut cap = scrap::Capturer::new( scrap::Display::primary()? )?;
		let w = cap.width();
		let h = cap.height();
		let mut buf = Vec::with_capacity( w * h * 4 );

		/* send a server init message. */ {
			buf.write_u16::<BigEndian>( w as u16 )?;
			buf.write_u16::<BigEndian>( h as u16 )?;
			buf.write_u8( 32 )?; // bits per pixel.
			buf.write_u8( 24 )?; // depth.
			buf.write_u8( 0 )?; // big endian flag.
			buf.write_u8( 1 )?; // true colour flag.
			buf.write_u16::<BigEndian>( 255 )?; // R max.
			buf.write_u16::<BigEndian>( 255 )?; // G max.
			buf.write_u16::<BigEndian>( 255 )?; // B max.
			buf.write_u8(  0 )?; // R shift.
			buf.write_u8(  8 )?; // G shift.
			buf.write_u8( 16 )?; // B shift.
			buf.write_all( &[0; 3] )?; // padding.
			let name = b"mfxvnc";
			buf.write_u32::<BigEndian>( name.len() as u32 )?;
			buf.write_all( name )?;
			stream.write_all( &buf )?;
			buf.clear();
		}

		let mut screen_prev = vec![ 0; w * h * 4 ];
		loop {
			// capture.
			let screen_next = match cap.frame() {
				Ok ( buf ) => buf,
				Err( err ) =>
					if err.kind() == std::io::ErrorKind::WouldBlock {
						continue;
					}
					else {
						return Err( err.into() );
					},
			};
			assert!( screen_next.len() == w * h * 4 );

			// framebuffer update header.
			buf.write_u8( 0 )?; // message type: framebuffer update.
			buf.write_u8( 0 )?; // padding.
			let n_rects_index = buf.len();
			buf.write_u16::<BigEndian>( 0 )?; // # of rectangles.

			// search & encode update region.
			let timer = time::SystemTime::now();
			let n_rects = Self::update_strip( &mut buf, &mut encoder, &mut screen_prev, &screen_next, w, h )?;
			let elapsed = timer.elapsed().unwrap();
			eprintln!( "encode: {:>3} ms, {:>4} KiB.",
				elapsed.as_secs() * 1000 + elapsed.subsec_nanos() as u64 / 1000000,
				buf.len() / 1024,
			);

			// rewrite # of rectangles.
			BigEndian::write_u16( &mut buf[n_rects_index ..], n_rects );

			// send messages.
			if n_rects > 0 {
				stream.write_all( &buf )?;
			}
			buf.clear();
		}
	}

	#[allow( dead_code )]
	fn update_block( buf: &mut Vec<u8>, encoder: &mut tight::TightEncoder, prev: &mut [u8], next: &[u8], w: usize, h: usize ) -> io::Result<u16> {
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
					buf.write_u16::<BigEndian>( x0 as u16 )?;
					buf.write_u16::<BigEndian>( y0 as u16 )?;
					buf.write_u16::<BigEndian>( (x1 - x0) as u16 )?;
					buf.write_u16::<BigEndian>( (y1 - y0) as u16 )?;
					buf.write_u32::<BigEndian>( 7 )?; // encoding: tight.
					encoder.encode( buf, next, w, h, x0, y0, x1, y1 );
					//buf.write_u32::<BigEndian>( 0 )?; // encoding: raw.
					//Self::encode_raw( &mut buf, &next, w, h, x0, y0, x1, y1 );
					n_rects += 1;
				}
				bx += Self::BSIZE_W;
			}
			by += Self::BSIZE_H;
		}
		Ok( n_rects )
	}

	#[allow( dead_code )]
	fn update_strip( buf: &mut Vec<u8>, encoder: &mut tight::TightEncoder, prev: &mut [u8], next: &[u8], w: usize, h: usize ) -> io::Result<u16> {
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
					buf.write_u16::<BigEndian>( x0 as u16 )?;
					buf.write_u16::<BigEndian>( y0 as u16 )?;
					buf.write_u16::<BigEndian>( (x1 - x0) as u16 )?;
					buf.write_u16::<BigEndian>( (y1 - y0) as u16 )?;
					buf.write_u32::<BigEndian>( 7 )?; // encoding: tight.
					encoder.encode( buf, next, w, h, x0, y0, x1, y1 );
					n_rects += 1;
				}
			}
			bx += Self::BSIZE_W;
		}
		Ok( n_rects )
	}

	#[allow( dead_code )]
	fn encode_raw( out: &mut Vec<u8>, screen: &[u8], w: usize, _h: usize, x0: usize, y0: usize, x1: usize, y1: usize ) {
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

fn main() -> Result<(), Box<std::error::Error>> {
	VncServer::listen( "0.0.0.0:5900" )?;
	Ok( () )
}
