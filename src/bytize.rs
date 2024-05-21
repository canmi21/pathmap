use std::{mem, ptr};

pub fn prefix_key(k: &u64) -> &[u8] {
    let bs = (8 - k.leading_zeros()/8) as u8;
    let kp: *const u64 = k;
    unsafe { std::slice::from_raw_parts(kp as *const _, (bs as usize).max(1)) }
}

// pub fn prefix_key_be(k: u64) -> Vec<u8> {
//     if k == 0 { vec![0] }
//     else { k.to_be_bytes()[(k.leading_zeros()/8) as usize..].to_vec() }
// }

pub fn prefix_key_be(k: &u64) -> &[u8] {
    let bs = (8 - k.trailing_zeros()/8) as usize;
    let l = bs.max(1);
    let kp: *const u64 = k;
    unsafe { std::slice::from_raw_parts((kp as *const u8).byte_offset((8 - l) as isize), l) }
}

pub fn from_prefix_key(k: Vec<u8>) -> u64 {
    let kp = unsafe { k.as_ptr() } as *const u64;
    let shift = 64usize.saturating_sub(k.len()*8);
    unsafe { (*kp) & (!0u64 >> shift) }
}
