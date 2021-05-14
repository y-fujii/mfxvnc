use packed_simd;
use std::*;

pub trait Comparator {
    fn compare<F: FnMut(usize, usize, usize, usize)>(_: &mut [u32], _: &[u32], _: usize, _: usize, _: usize, _: F);
}

pub struct BlockComparator;

impl BlockComparator {
    const BSIZE_W: usize = 64;
    const BSIZE_H: usize = 64;
}

impl Comparator for BlockComparator {
    fn compare<F: FnMut(usize, usize, usize, usize)>(
        prev: &mut [u32],
        next: &[u32],
        stride: usize,
        w: usize,
        h: usize,
        mut callback: F,
    ) {
        let prev = prev.as_mut_ptr();
        let next = next.as_ptr();
        let mut by = 0;
        while by < h {
            let mut bx = 0;
            while bx < w {
                let mut upper = packed_simd::u64x2::new(bx as u64, by as u64);
                let mut lower = upper + packed_simd::u64x2::new(Self::BSIZE_W as u64, Self::BSIZE_H as u64);
                for y in by..cmp::min(by + Self::BSIZE_H, h) {
                    for x in bx..cmp::min(bx + Self::BSIZE_W, w) {
                        let p = unsafe { *prev.add(stride * y).add(x) };
                        let q = unsafe { *next.add(stride * y).add(x) } & 0x00ffffff;
                        if p != q {
                            let xy = packed_simd::u64x2::new(x as u64, y as u64);
                            lower = lower.min(xy);
                            upper = upper.max(xy + 1);
                            unsafe { *prev.add(stride * y).add(x) = q };
                        }
                    }
                }
                if lower.lt(upper).all() {
                    let x0 = lower.extract(0) as usize;
                    let y0 = lower.extract(1) as usize;
                    let x1 = upper.extract(0) as usize;
                    let y1 = upper.extract(1) as usize;
                    callback(x0, y0, x1, y1);
                }
                bx += Self::BSIZE_W;
            }
            by += Self::BSIZE_H;
        }
    }
}

pub struct StripComparator;

impl StripComparator {
    const BSIZE_W: usize = 64;
    const BSIZE_H: usize = 128;
}

impl Comparator for StripComparator {
    fn compare<F: FnMut(usize, usize, usize, usize)>(
        prev: &mut [u32],
        next: &[u32],
        stride: usize,
        w: usize,
        h: usize,
        mut callback: F,
    ) {
        let prev = prev.as_mut_ptr();
        let next = next.as_ptr();
        let mut bx = 0;
        while bx < w {
            let mut y = 0;
            while y < h {
                'exit: while y < h {
                    let prev = unsafe { prev.add(stride * y) };
                    let next = unsafe { next.add(stride * y) };
                    for x in bx..cmp::min(bx + Self::BSIZE_W, w) {
                        let p = unsafe { *prev.add(x) };
                        let q = unsafe { *next.add(x) } & 0x00ffffff;
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
                while y < cmp::min(y0 + Self::BSIZE_H, h) {
                    let mut unchanged = true;
                    let prev = unsafe { prev.add(stride * y) };
                    let next = unsafe { next.add(stride * y) };
                    for x in bx..cmp::min(bx + Self::BSIZE_W, w) {
                        let p = unsafe { *prev.add(x) };
                        let q = unsafe { *next.add(x) } & 0x00ffffff;
                        if p != q {
                            unchanged = false;
                            x0 = cmp::min(x0, x);
                            x1 = cmp::max(x1, x + 1);
                            unsafe { *prev.add(x) = q };
                        }
                    }
                    if unchanged {
                        if n >= 8 {
                            break;
                        }
                        n += 1;
                    } else {
                        n = 0;
                    }
                    y += 1;
                }
                let y1 = y - n;

                if y0 < y1 {
                    callback(x0, y0, x1, y1);
                }
            }
            bx += Self::BSIZE_W;
        }
    }
}

pub struct QuadtreeComparator;

impl QuadtreeComparator {
    const TPIXELS: usize = 1024;
    const ALIGN_W: usize = 8;
    const ALIGN_H: usize = 8;
}

impl Comparator for QuadtreeComparator {
    fn compare<F: FnMut(usize, usize, usize, usize)>(
        prev: &mut [u32],
        next: &[u32],
        stride: usize,
        w: usize,
        h: usize,
        mut callback: F,
    ) {
        if let Some(a) = Self::compare_rec(prev, next, stride, 0, 0, w, h, &mut callback) {
            callback(a.0, a.1, a.2, a.3)
        }
    }
}

impl QuadtreeComparator {
    fn compare_rec<F: FnMut(usize, usize, usize, usize)>(
        prev: &mut [u32],
        next: &[u32],
        stride: usize,
        x0: usize,
        y0: usize,
        x1: usize,
        y1: usize,
        callback: &mut F,
    ) -> Option<(usize, usize, usize, usize)> {
        let w = x1 - x0;
        let h = y1 - y0;
        if w * h <= Self::TPIXELS {
            Self::compare_block(prev, next, stride, x0, y0, x1, y1)
        } else {
            let (ax1, ay1, bx0, by0) = if h <= w {
                let m = (x0 + x1 + Self::ALIGN_W) / (2 * Self::ALIGN_W) * Self::ALIGN_W;
                (m, y1, m, y0)
            } else {
                let m = (y0 + y1 + Self::ALIGN_H) / (2 * Self::ALIGN_H) * Self::ALIGN_H;
                (x1, m, x0, m)
            };
            let a_rect = Self::compare_rec(prev, next, stride, x0, y0, ax1, ay1, callback);
            let b_rect = Self::compare_rec(prev, next, stride, bx0, by0, x1, y1, callback);

            match (a_rect, b_rect) {
                (None, None) => None,
                (Some(_), None) => a_rect,
                (None, Some(_)) => b_rect,
                (Some(a), Some(b)) => {
                    let x0 = cmp::min(a.0, b.0);
                    let y0 = cmp::min(a.1, b.1);
                    let x1 = cmp::max(a.2, b.2);
                    let y1 = cmp::max(a.3, b.3);
                    let a_area = (a.2 - a.0) * (a.3 - a.1);
                    let b_area = (b.2 - b.0) * (b.3 - b.1);
                    let m_area = (x1 - x0) * (y1 - y0);
                    if x1 - x0 <= 2048 && m_area <= (2 << 22) / 3 && // The approx limits of Tight encoding.
					   (m_area <= a_area + b_area + Self::TPIXELS || 15 * m_area <= 16 * (a_area + b_area))
                    {
                        Some((x0, y0, x1, y1))
                    } else if a_area < b_area {
                        callback(a.0, a.1, a.2, a.3);
                        b_rect
                    } else {
                        callback(b.0, b.1, b.2, b.3);
                        a_rect
                    }
                }
            }
        }
    }

    fn compare_block(
        prev: &mut [u32],
        next: &[u32],
        stride: usize,
        x0: usize,
        y0: usize,
        x1: usize,
        y1: usize,
    ) -> Option<(usize, usize, usize, usize)> {
        let prev = prev.as_mut_ptr();
        let next = next.as_ptr();
        /*
        let mut lower = packed_simd::u64x2::splat( u64::MAX );
        let mut upper = packed_simd::u64x2::splat( 0 );
        for y in y0 .. y1 {
            let prev = unsafe { prev.add( stride * y ) };
            let next = unsafe { next.add( stride * y ) };
            for x in x0 .. x1 {
                let p = unsafe { *prev.add( x ) };
                let q = unsafe { *next.add( x ) } & 0x00ffffff;
                if p != q {
                    let xy = packed_simd::u64x2::new( x as u64, y as u64 );
                    lower = lower.min( xy );
                    upper = upper.max( xy + 1 );
                    unsafe { *prev.add( x ) = q };
                }
            }
        }
        if lower.lt( upper ).all() {
            let x0 = lower.extract( 0 ) as usize;
            let y0 = lower.extract( 1 ) as usize;
            let x1 = upper.extract( 0 ) as usize;
            let y1 = upper.extract( 1 ) as usize;
            Some( (x0, y0, x1, y1) )
        }
        else {
            None
        }
        */
        let mut x_min = usize::MAX;
        let mut y_min = usize::MAX;
        let mut x_max = 0;
        let mut y_max = 0;
        for y in y0..y1 {
            let prev = unsafe { prev.add(stride * y) };
            let next = unsafe { next.add(stride * y) };
            let mut x = x0;
            while x < x1 {
                let p = unsafe { *prev.add(x) };
                let q = unsafe { *next.add(x) } & 0x00ffffff;
                if p != q {
                    unsafe { *prev.add(x) = q };
                    x_min = cmp::min(x_min, x);
                    x_max = cmp::max(x_max, x + 1);
                    y_min = cmp::min(y_min, y);
                    y_max = cmp::max(y_max, y + 1);
                    x += 1;
                    break;
                }
                x += 1;
            }
            while x < x1 {
                let p = unsafe { *prev.add(x) };
                let q = unsafe { *next.add(x) } & 0x00ffffff;
                if p != q {
                    unsafe { *prev.add(x) = q };
                    x_max = cmp::max(x_max, x + 1);
                }
                x += 1;
            }
        }
        if x_min < x_max {
            Some((x_min, y_min, x_max, y_max))
        } else {
            None
        }
    }
}
