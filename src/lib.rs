#![allow(warnings)]
pub mod ring;
pub mod bytize;
pub mod bytetrie;
pub mod zipper;


/// returns the position of the next/previous active bit in x
/// if there is no next/previous bit, returns the argument position
/// assumes that pos is active in x
fn bit_sibling(pos: u8, x: u64, next: bool) -> u8 {
    debug_assert_ne!((1u64 << pos) & x, 0);
    if next {
        if pos == 0 { return 0 } // resolves overflow in shift
        let succ = !0u64 >> (64 - pos);
        let m = x & succ;
        if m == 0u64 { pos }
        else { (63 - m.leading_zeros()) as u8 }
    } else {
        let prec = !(!0u64 >> (63 - pos));
        let m = x & prec;
        if m == 0u64 { pos }
        else { m.trailing_zeros() as u8 }
    }
}

// #[cfg(test)]
// mod tests {
//     use std::collections::HashSet;
//     use crate::bit_sibling;
//     use crate::bytetrie::BytesTrieMap;
//
//     #[test]
//     fn btm_prefix() {
//         // from https://en.wikipedia.org/wiki/Radix_tree#/media/File:Patricia_trie.svg
//         let mut btm = BytesTrieMap::new();
//         let rs: Vec<&str> = vec!["romane", "romanus", "romulus", "rubens", "ruber", "rubicon", "rubicundus", "rom'i"];
//         rs.iter().enumerate().for_each(|(i, r)| { btm.insert(r.as_bytes(), i); });
//         assert_eq!(btm.at("rom".as_bytes()).map(|m| m.items().collect::<HashSet<_>>()),
//                    Some(HashSet::from([("ane".as_bytes().to_vec(), 0), ("anus".as_bytes().to_vec(), 1), ("ulus".as_bytes().to_vec(), 2), ("'i".as_bytes().to_vec(), 7)])));
//
//         let mut rz = crate::zipper::ReadZipper::new(&btm);
//         rz.child('r' as u8); rz.child('o' as u8); rz.child('m' as u8); // focus = rom
//         assert!(rz.child('\'' as u8)); // focus = rom'  (' is the lowest byte)
//         assert!(rz.sibling(true)); // focus = roma  (a is the second byte)
//         assert_eq!(rz.focus.items().map(|(k, _)| k).collect::<Vec<_>>(), vec!['n' as u8]); // both follow-ups romane and romanus have n following a
//         assert!(rz.sibling(true)); // focus = romu  (u is the third byte)
//         assert_eq!(rz.focus.items().map(|(k, _)| k).collect::<Vec<_>>(), vec!['l' as u8]); // and romu is followed by lus
//         assert!(!rz.sibling(true)); // fails (u is the highest byte)
//         assert!(rz.sibling(false)); // focus = roma (we can step back)
//         assert_eq!(rz.focus.items().map(|(k, _)| k).collect::<Vec<_>>(), vec!['n' as u8]); // again
//         assert!(rz.parent()); // focus = rom
//         assert_eq!(rz.focus.items().map(|(k, _)| k).collect::<Vec<_>>(), vec!['\'' as u8, 'a' as u8, 'u' as u8]); // all three options we visited
//         assert!(rz.nth_child(0, true)); // focus = rom'
//         assert_eq!(rz.focus.items().map(|(k, _)| k).collect::<Vec<_>>(), vec!['i' as u8]);
//         assert!(rz.parent()); // focus = rom
//         assert!(rz.nth_child(1, true)); // focus = roma
//         assert_eq!(rz.focus.items().map(|(k, _)| k).collect::<Vec<_>>(), vec!['n' as u8]);
//         assert!(rz.parent());
//         assert!(rz.nth_child(2, true)); // focus = romu
//         assert_eq!(rz.focus.items().map(|(k, _)| k).collect::<Vec<_>>(), vec!['l' as u8]);
//         assert!(rz.parent());
//         assert!(rz.nth_child(1, false)); // focus = roma
//         assert_eq!(rz.focus.items().map(|(k, _)| k).collect::<Vec<_>>(), vec!['n' as u8]);
//         assert!(rz.parent());
//         assert!(rz.nth_child(2, false)); // focus = rom'
//         assert_eq!(rz.focus.items().map(|(k, _)| k).collect::<Vec<_>>(), vec!['i' as u8]);
//         // ' < a < u
//         // 39 105 117
//     }
//
//     #[test]
//     fn bit_siblings() {
//         let x = 0b0000000000000000000000000000000000000100001001100000000000000010u64;
//         let i = 0b0000000000000000000000000000000000000000000001000000000000000000u64;
//         let p = 0b0000000000000000000000000000000000000000001000000000000000000000u64;
//         let n = 0b0000000000000000000000000000000000000000000000100000000000000000u64;
//         let f = 0b0000000000000000000000000000000000000100000000000000000000000000u64;
//         let l = 0b0000000000000000000000000000000000000000000000000000000000000010u64;
//         let bit_i = 18;
//         let bit_i_onehot = 1u64 << bit_i;
//         assert_eq!(i, bit_i_onehot);
//         assert_ne!(bit_i_onehot & x, 0);
//         assert_eq!(p, 1u64 << bit_sibling(bit_i, x, false));
//         assert_eq!(n, 1u64 << bit_sibling(bit_i, x, true));
//         assert_eq!(f, 1u64 << bit_sibling(f.trailing_zeros() as u8, x, false));
//         assert_eq!(l, 1u64 << bit_sibling(l.trailing_zeros() as u8, x, true));
//         assert_eq!(0, bit_sibling(0, 1, false));
//         assert_eq!(0, bit_sibling(0, 1, true));
//         assert_eq!(63, bit_sibling(63, 1u64 << 63, false));
//         assert_eq!(63, bit_sibling(63, 1u64 << 63, true));
//     }
// }
