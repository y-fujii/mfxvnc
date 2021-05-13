use std::*;
use std::io::{ Read, Write };
use byteorder::{ ByteOrder, ReadBytesExt, WriteBytesExt, BigEndian };
use scrap;
use comparator;
use encoder;


pub struct VncServer<Comparator: comparator::Comparator, Encoder: encoder::Encoder> {
	_comparator: marker::PhantomData<Comparator>,
	_encoder: marker::PhantomData<Encoder>,
}

impl<Comparator: comparator::Comparator, Encoder: encoder::Encoder> VncServer<Comparator, Encoder> {
	pub fn listen<A: net::ToSocketAddrs>( addr: A ) -> io::Result<()> {
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
		let mut encoder = Encoder::new();
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
		}

		let mut prev_screen = Vec::new();
		loop {
			let prev_buf_len = buf.len();
			buf.clear();

			// framebuffer update header.
			buf.write_u8( 0 )?; // message type: framebuffer update.
			buf.write_u8( 0 )?; // padding.
			let n_rects_index = buf.len();
			buf.write_u16::<BigEndian>( 0 )?; // # of rectangles.

			let n_rects = {
				// capture.
				let next_screen = match cap.frame() {
					Ok ( buf ) => buf,
					Err( err ) =>
						if err.kind() == io::ErrorKind::WouldBlock {
							thread::sleep( time::Duration::from_secs( 1 ) / 120 );
							continue;
						}
						else {
							return Err( err.into() );
						},
				};
				let next_screen = unsafe { slice::from_raw_parts(
					next_screen.as_ptr() as *const u32, next_screen.len() / 4,
				) };
				if next_screen.len() != prev_screen.len() {
					prev_screen = vec![ 0; next_screen.len() ];
				}
				let stride = next_screen.len() / h;

				// search & encode update region.
				let timer = time::SystemTime::now();
				let mut n_rects = 0;
				Comparator::compare( &mut prev_screen, &next_screen, stride, w, h, |x0, y0, x1, y1| {
					buf.write_u16::<BigEndian>( x0 as u16 ).unwrap();
					buf.write_u16::<BigEndian>( y0 as u16 ).unwrap();
					buf.write_u16::<BigEndian>( (x1 - x0) as u16 ).unwrap();
					buf.write_u16::<BigEndian>( (y1 - y0) as u16 ).unwrap();
					encoder.encode( &mut buf, &next_screen[stride * y0 + x0..], stride, x1 - x0, y1 - y0 );
					n_rects += 1;
				} );
				let elapsed = timer.elapsed().unwrap();
				eprintln!( "  encode: {:>3} ms, {:>4} KiB.",
					elapsed.as_secs() * 1000 + elapsed.subsec_nanos() as u64 / 1000000, buf.len() / 1024,
				);

				n_rects
			};

			// rewrite # of rectangles.
			BigEndian::write_u16( &mut buf[n_rects_index ..], n_rects );

			// send messages.
			if n_rects > 0 {
				stream.write_all( &buf )?;
			}

			// throttle.
			#[cfg( unix )]
			{
				use libc;
				use os::unix::io::AsRawFd;

				let mut n = 0;
				while {
					let mut remaining: i32 = 0;
					unsafe { libc::ioctl( stream.as_raw_fd(), libc::TIOCOUTQ, &mut remaining ) };
					assert!( remaining >= 0 );
					remaining as usize >= prev_buf_len + buf.len()
				}
				{
					thread::sleep( time::Duration::from_secs( 1 ) / 120 );
					n += 1;
				}
				if n > 0 {
					eprintln!( "throttle: {:>3} ms", n * 1000 / 120 );
					//cap.frame().ok();
				}
			}
		}
	}
}
