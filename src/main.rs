#[macro_use]
extern crate packed_simd;
extern crate byteorder;
extern crate rand;
extern crate miniz_oxide;
extern crate scrap;
mod comparator;
mod encoder;
mod server;
use std::*;


fn main() -> Result<(), Box<error::Error>> {
	//server::VncServer::<comparator::StripComparator, encoder::RandomColorEncoder>::listen( "0.0.0.0:5900" )?;
	//server::VncServer::<comparator::StripComparator, encoder::TightRawEncoder>::listen( "0.0.0.0:5900" )?;
	server::VncServer::<comparator::StripComparator, encoder::TightGradientEncoder>::listen( "0.0.0.0:5900" )?;
	//server::VncServer::<comparator::StripComparator, encoder::TightAdaptiveEncoder>::listen( "0.0.0.0:5900" )?;
	//server::VncServer::<comparator::StripComparator, encoder::TightJpegEncoder>::listen( "0.0.0.0:5900" )?;
	//server::VncServer::<comparator::QuadtreeComparator, encoder::RandomColorEncoder>::listen( "0.0.0.0:5900" )?;
	//server::VncServer::<comparator::QuadtreeComparator, encoder::TightRawEncoder>::listen( "0.0.0.0:5900" )?;
	//server::VncServer::<comparator::QuadtreeComparator, encoder::TightGradientEncoder>::listen( "0.0.0.0:5900" )?;
	//server::VncServer::<comparator::QuadtreeComparator, encoder::TightAdaptiveEncoder>::listen( "0.0.0.0:5900" )?;
	//server::VncServer::<comparator::QuadtreeComparator, encoder::TightJpegEncoder>::listen( "0.0.0.0:5900" )?;
	Ok( () )
}
