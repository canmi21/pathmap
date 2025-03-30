
use crate::ring::*;

/// A 256-bit type containing a bit for every possible value in a byte
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct ByteMask(pub [u64; 4]);

impl ByteMask {
    pub const EMPTY: ByteMask = Self(empty_mask());

    /// Create a new empty ByteMask
    #[inline]
    pub const fn new() -> Self {
        Self(empty_mask())
    }
    /// Unwraps the `ByteMask` type to yield the inner array
    #[inline]
    pub fn into_inner(self) -> [u64; 4] {
        self.0
    }
    /// Load a new bytemask from bytes
    #[inline]
    pub fn from_bytes(arr: &[u8; 32]) -> Self { unsafe { ByteMask(std::mem::transmute_copy(arr)) } }
    /// Unwraps the `ByteMask` type to yield the inner byte array
    #[inline]
    pub fn as_bytes(self) -> [u8; 32] { unsafe { std::mem::transmute(self.0) } }
    /// Create an iterator over every byte, in ascending order
    #[inline]
    pub fn iter(&self) -> ByteMaskIter {
        self.byte_mask_iter()
    }
    #[inline]
    pub fn xor(&self, other: &ByteMask) -> ByteMask {
        ByteMask([self.0[0] ^ other.0[0], self.0[1] ^ other.0[1], self.0[2] ^ other.0[2], self.0[3] ^ other.0[3]])
    }
}

impl core::fmt::Debug for ByteMask {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}

impl BitMask for ByteMask {
    #[inline]
    fn count_bits(&self) -> usize { self.0.count_bits() }
    #[inline]
    fn is_empty_mask(&self) -> bool { self.0.is_empty_mask() }
    #[inline]
    fn test_bit(&self, k: u8) -> bool { self.0.test_bit(k) }
    #[inline]
    fn set_bit(&mut self, k: u8) { self.0.set_bit(k) }
    #[inline]
    fn clear_bit(&mut self, k: u8) { self.0.clear_bit(k) }
    #[inline]
    fn make_empty(&mut self) {self.0.make_empty() }
}

impl From<u8> for ByteMask {
    #[inline]
    fn from(singleton_byte: u8) -> Self {
        let mut new_mask = Self::new();
        new_mask.set_bit(singleton_byte);
        new_mask
    }
}

impl From<[u64; 4]> for ByteMask {
    #[inline]
    fn from(mask: [u64; 4]) -> Self {
        Self(mask)
    }
}

impl From<ByteMask> for [u64; 4] {
    #[inline]
    fn from(mask: ByteMask) -> Self {
        mask.0
    }
}

impl IntoByteMaskIter for ByteMask {
    #[inline]
    fn byte_mask_iter(self) -> ByteMaskIter {
        self.0.byte_mask_iter()
    }
}

impl FromIterator<u8> for ByteMask {
    #[inline]
    fn from_iter<I: IntoIterator<Item=u8>>(iter: I) -> Self {
        let mut result = Self::new();
        for byte in iter.into_iter() {
            result.set_bit(byte);
        }
        result
    }
}

impl PartialEq<ByteMask> for [u64; 4] {
    #[inline]
    fn eq(&self, other: &ByteMask) -> bool {
        *self == other.0
    }
}

impl PartialEq<[u64; 4]> for ByteMask {
    #[inline]
    fn eq(&self, other: &[u64; 4]) -> bool {
        self.0 == *other
    }
}

impl Lattice for ByteMask {
    #[inline]
    fn pjoin(&self, other: &Self) -> AlgebraicResult<Self> {
        self.0.pjoin(&other.0).map(|mask| Self(mask))
    }
    #[inline]
    fn pmeet(&self, other: &Self) -> AlgebraicResult<Self> {
        self.0.pmeet(&other.0).map(|mask| Self(mask))
    }
    #[inline]
    fn bottom() -> Self {
        Self::new()
    }
}

impl DistributiveLattice for ByteMask {
    #[inline]
    fn psubtract(&self, other: &Self) -> AlgebraicResult<Self> where Self: Sized {
        self.0.psubtract(&other.0).map(|mask| Self(mask))
    }
}

//GOAT, below here is functionality implemented on arrays of u64, which ought to be generalized to additional widths

/// Some useful bit-twiddling methods for working with the mask you might get from [child_mask](crate::zipper::Zipper::child_mask)
pub trait BitMask {
    /// Returns the number of set bits in `mask`
    fn count_bits(&self) -> usize;

    /// Returns `true` if all bits in `mask` are clear, otherwise returns `false`
    fn is_empty_mask(&self) -> bool;

    /// Returns `true` if the `k`th bit in `mask` is set, otherwise returns `false`
    fn test_bit(&self, k: u8) -> bool;

    /// Sets the `k`th bit in mask
    fn set_bit(&mut self, k: u8);

    /// Clears the `k`th bit in mask
    fn clear_bit(&mut self, k: u8);

    /// Clears all bits in the mask, restoring it to an empty mask
    fn make_empty(&mut self);
}

impl BitMask for [u64; 4] {
    #[inline]
    fn count_bits(&self) -> usize {
        return (self[0].count_ones() + self[1].count_ones() + self[2].count_ones() + self[3].count_ones()) as usize;
    }
    #[inline]
    fn is_empty_mask(&self) -> bool {
        self[0] == 0 && self[1] == 0 && self[2] == 0 && self[3] == 0
    }
    #[inline]
    fn test_bit(&self, k: u8) -> bool {
        let idx = ((k & 0b11000000) >> 6) as usize;
        let bit_i = k & 0b00111111;
        debug_assert!(idx < 4);
        self[idx] & (1 << bit_i) > 0
    }
    #[inline]
    fn set_bit(&mut self, k: u8) {
        let idx = (k / 64) as usize;
        self[idx] |= 1 << (k % 64);
    }
    #[inline]
    fn clear_bit(&mut self, k: u8) {
        let idx = (k / 64) as usize;
        self[idx] ^= 1 << (k % 64);
    }
    #[inline]
    fn make_empty(&mut self) {
        *self = [0; 4];
    }
}

/// An iterator to visit each byte in a byte mask in ascending order.  Useful for working with the mask
/// as you might get from [child_mask](crate::zipper::Zipper::child_mask)
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
    pub fn new(mask: [u64; 4]) -> Self {
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

//GOAT, This needs to be generalized to bit sets of other widths
impl Lattice for [u64; 4] {
    #[inline]
    fn pjoin(&self, other: &Self) -> AlgebraicResult<Self> {
        let result = [self[0] | other[0], self[1] | other[1], self[2] | other[2], self[3] | other[3]];
        bitmask_algebraic_result(result, self, other)
    }
    #[inline]
    fn pmeet(&self, other: &Self) -> AlgebraicResult<Self> {
        let result = [self[0] & other[0], self[1] & other[1], self[2] & other[2], self[3] & other[3]];
        bitmask_algebraic_result(result, self, other)
    }
    #[inline]
    fn bottom() -> Self {
        empty_mask()
    }
}

//GOAT, This should be generalized to bit sets of other widths
impl DistributiveLattice for [u64; 4] {
    #[inline]
    fn psubtract(&self, other: &Self) -> AlgebraicResult<Self> where Self: Sized {
        let result = [self[0] & !other[0], self[1] & !other[1], self[2] & !other[2], self[3] & !other[3]];
        bitmask_algebraic_result(result, self, other)
    }
}

/// Internal function to compose AlgebraicResult after algebraic operation
#[inline]
fn bitmask_algebraic_result(result: [u64; 4], self_mask: &[u64; 4], other_mask: &[u64; 4]) -> AlgebraicResult<[u64; 4]> {
    if result.is_empty() {
        return AlgebraicResult::None
    }
    let mut mask = 0;
    if result == *self_mask {
        mask  = SELF_IDENT;
    }
    if result == *other_mask {
        mask |= COUNTER_IDENT;
    }
    if mask > 0 {
        return AlgebraicResult::Identity(mask)
    } else {
        AlgebraicResult::Element(result)
    }
}

/// Returns a new empty mask
#[inline]
pub const fn empty_mask() -> [u64; 4] {
    [0; 4]
}

#[test]
fn bit_utils_test() {
    let mut mask = empty_mask();
    assert_eq!(mask.count_bits(), 0);
    assert_eq!(mask.is_empty_mask(), true);

    mask.set_bit(b'C');
    mask.set_bit(b'a');
    mask.set_bit(b't');
    assert_eq!(mask.is_empty_mask(), false);
    assert_eq!(mask.count_bits(), 3);

    mask.set_bit(b'C');
    mask.set_bit(b'a');
    mask.set_bit(b'n');
    assert_eq!(mask.count_bits(), 4);

    mask.clear_bit(b't');
    assert_eq!(mask.test_bit(b'n'), true);
    assert_eq!(mask.test_bit(b't'), false);
}
