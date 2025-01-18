#[inline]
pub fn byte_mask_subtract(x: [u64; 4], y: [u64; 4]) -> [u64; 4] {
    [x[0] & !y[0], x[1] & !y[1], x[2] & !y[2], x[3] & !y[3]]
}

#[inline]
pub fn byte_mask_meet(x: [u64; 4], y: [u64; 4]) -> [u64; 4] {
    [x[0] & y[0], x[1] & y[1], x[2] & y[2], x[3] & y[3]]
}

#[inline]
pub fn byte_mask_join(x: [u64; 4], y: [u64; 4]) -> [u64; 4] {
    [x[0] | y[0], x[1] | y[1], x[2] | y[2], x[3] | y[3]]
}

#[inline]
pub fn byte_mask_left(x: [u64; 4], pos: u8) -> u8 {
    if pos == 0 { return 0 }
    let mut c = 0u8;
    let m = !0u64 >> (63 - ((pos - 1) & 0b00111111));
    if pos > 0b01000000 { c += x[0].count_ones() as u8; }
    else { return c + (x[0] & m).count_ones() as u8 }
    if pos > 0b10000000 { c += x[1].count_ones() as u8; }
    else { return c + (x[1] & m).count_ones() as u8 }
    if pos > 0b11000000 { c += x[2].count_ones() as u8; }
    else { return c + (x[2] & m).count_ones() as u8 }
    c + (x[3] & m).count_ones() as u8
}

#[inline]
pub fn byte_mask_contains(x: [u64; 4], k: u8) -> bool {
    0 != (x[((k & 0b11000000) >> 6) as usize] & (1u64 << (k & 0b00111111)))
}

#[inline]
pub fn byte_mask_set(x: &mut [u64; 4], k: u8) -> () {
    x[((k & 0b11000000) >> 6) as usize] |= 1u64 << (k & 0b00111111);
}

#[inline]
pub fn byte_mask_clear(x: &mut [u64; 4], k: u8) -> () {
    x[((k & 0b11000000) >> 6) as usize] &= !(1u64 << (k & 0b00111111));
}

#[inline]
pub fn byte_mask_from_iter<T: Iterator<Item=u8>>(iter: T) -> [u64; 4] {
    let mut m = [0u64; 4];
    iter.for_each(|b| byte_mask_set(&mut m, b));
    m
}

/// An iterator to visit each byte in a byte mask, as you might get from [child_mask](crate::zipper::Zipper::child_mask)
pub struct ByteMaskIter {
    i: u8,
    mask: [u64; 4],
}

pub trait IntoByteMaskIter {
    fn byte_mask_iter(self) -> ByteMaskIter;
}

impl IntoByteMaskIter for [u64; 4] {
    fn byte_mask_iter(self) -> ByteMaskIter {
        ByteMaskIter::from(self)
    }
}

impl IntoByteMaskIter for &[u64; 4] {
    fn byte_mask_iter(self) -> ByteMaskIter {
        ByteMaskIter::from(*self)
    }
}

impl From<[u64; 4]> for ByteMaskIter {
    fn from(mask: [u64; 4]) -> Self {
        Self::new(mask)
    }
}

impl ByteMaskIter {
    /// Make a new `ByteMaskIter` from a mask, as you might get from [child_mask](crate::zipper::Zipper::child_mask)
    fn new(mask: [u64; 4]) -> Self {
        Self {
            i: 0,
            mask,
        }
    }
}

impl Iterator for ByteMaskIter {
    type Item = u8;

    fn next(&mut self) -> Option<u8> {
        loop {
            let w = &mut self.mask[self.i as usize];
            if *w != 0 {
                let wi = w.trailing_zeros() as u8;
                *w ^= 1u64 << wi;
                let index = self.i*64 + wi;
                return Some(index)
            } else if self.i < 3 {
                self.i += 1;
            } else {
                return None
            }
        }
    }
}