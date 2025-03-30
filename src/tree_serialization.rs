use std::cell::UnsafeCell;
use std::io::Write;
use std::ptr::slice_from_raw_parts;
use crate::{zipper, TrieValue};
use crate::morphisms::{Catamorphism, new_map_from_ana, TrieBuilder, new_map_from_ana_jumping};
use crate::trie_map::BytesTrieMap;
use crate::utils::{BitMask, ByteMask};
use crate::write_zipper::ZipperWriting;
use crate::zipper::ZipperAbsolutePath;

/// WIP
pub fn serialize_fork<V : TrieValue, RZ : Catamorphism<V>, F: FnMut(usize, &[u8], &V) -> ()>(mut rz: RZ, target: &mut Vec<u8>, mut fv: F) -> std::io::Result<(usize)> {
    unsafe {
    thread_local! {
        static written: UnsafeCell<usize> = UnsafeCell::new(0)
    }
    written.with(|w| {
        rz.into_cata_jumping_side_effect_fallible(|bm: &ByteMask, ws: &mut [usize], jump, ov: Option<&V>, path: &[u8]| {
            // let bs = bm.iter().collect::<Vec<u8>>();
            // println!("at {path:?} #{} jump {} {:?}", bm.count_bits(), jump, &path[path.len()-jump..]);

            let w0 = *w.get();
            // println!("child ptrs {:?}, w0 {}, targetlen {}", ws, w0, target.len());
            let mut l = 0;
            target.write(jump.to_le_bytes().as_slice())?; l += 8;
            target.write(&path[path.len()-jump..])?; l += jump;
            target.write(slice_from_raw_parts(bm.0.as_ptr() as *const u8, 32).as_ref().unwrap())?; l += 32;
            target.write(slice_from_raw_parts(ws.as_ptr() as *const u8, 8*ws.len()).as_ref().unwrap())?; l += 8*ws.len();
            *w.get() = w0 + l;

            Ok(w0)
        })
    })
    }
}

/// WIP
pub fn deserialize_fork<V : TrieValue, WZ : ZipperWriting<V> + zipper::ZipperMoving, F: Fn(usize, &[u8]) -> V>(node: usize, wz: &mut WZ, source: &[u8], fv: F) -> std::io::Result<(usize)> {
    unsafe {
    // let mut recovered = 0;
    new_map_from_ana_jumping(wz, node, |n: usize, path: &[u8]| {
        // println!("n {}", n);
        let jump = usize::from_le_bytes((&source[n..n+8]).try_into().unwrap());
        let jump_path = &source[n+8..n+8+jump];
        let bm = ByteMask(std::ptr::read(&source[n+8+jump] as *const u8 as *const _));
        let ws = slice_from_raw_parts(&source[n+8+jump+32] as *const u8 as *const usize, bm.count_bits()).as_ref().unwrap();
        // println!("offset {}", n+8+jump+32);
        // println!("at {path:?} #{} jump {} {:?}", bm.count_bits(), jump, jump_path);
        // println!("child ptrs {:?}", ws);

        // recovered += 1;
        (jump_path, bm, ws.into_iter().cloned(), Some(fv(0, path)))
    });
    Ok(1)
    }
}

#[cfg(test)]
mod tests {
    use crate::tree_serialization::*;
    use crate::trie_map::BytesTrieMap;
    use crate::zipper::{ZipperAccess, ZipperIteration, ZipperMoving, ZipperReadOnly};

    #[test]
    fn tree_serde_2() {
        let keys = [vec![12, 13, 14], vec![12, 13, 14, 100, 101]];
        let btm: BytesTrieMap<usize> = keys.into_iter().enumerate().map(|(i, k)| (k, i)).collect();

        let mut v = vec![];
        let Ok(top_node) = serialize_fork(btm.read_zipper(), &mut v, |_1, _2, _3| {}) else { unreachable!() };
        let mut recovered = BytesTrieMap::new();
        deserialize_fork(top_node, &mut recovered.write_zipper(), &v[..], |_, _p| ()).unwrap();
        assert_eq!(btm.hash(|_i| 0), recovered.hash(|_i| 0));
    }

    #[test]
    fn patterns() {
        // let mut map = BytesTrieMap::<u64>::new();
        // map.insert(b"start:0000:hello", 0);
        // map.insert(b"start:0001:hello", 1);
        // map.insert(b"start:0002:hello", 2);
        // map.insert(b"start:0003:hello", 3);
        // map.insert(b"Foo:0000", 0);
        // map.insert(b"Foo:0001", 1);
        // map.insert(b"Bar:0000", 2);
        // map.insert(b"Bar:0001", 3);
        // map.insert(b"Foo:x:0000", 0);
        // map.insert(b"Foo:y:0001", 1);
        // map.insert(b"Bar:z:0000", 2);
        // map.insert(b"Bar:w:0001", 3);
        let rs = ["arrow", "bow", "cannon", "roman", "romane", "romanus", "romulus", "rubens", "ruber", "rubicon", "rubicundus", "rom'i"];
        let map: BytesTrieMap<u64> = rs.into_iter().enumerate().map(|(i, k)| (k, i as u64)).collect();

        let mut patterns = BytesTrieMap::new();

        let mut rz = map.read_zipper();
        loop {
            let mut srz = rz.fork_read_zipper();
            while let Some(ov) = srz.to_next_val() {
                let mut pwz = patterns.write_zipper_at_path(srz.path());
                let v = pwz.get_value_or_insert(BytesTrieMap::new());
                v.insert(rz.path(), *ov);
            }
            drop(srz);
            // patterns_wz.join(&rz.clone().make_map().unwrap().map_values(|ov| BytesTrieMap::from_iter(std::iter::once((path, *ov)))).read_zipper());
            if !rz.to_next_step() { break }
        }

        let mut results = vec![];
        let result_depth = 2;
        let mut prz = patterns.read_zipper();
        prz.descend_first_k_path(result_depth);
        loop {
            if let Some(sm) = prz.get_value() {
                if sm.at_least(2) {
                    results.push((String::from_utf8(prz.path().to_vec()).unwrap(), sm.iter().map(|(p, v)| (String::from_utf8(p).unwrap(), *v)).collect::<Vec<_>>()));
                    // println!("shared ending {:?} between {:?}", std::str::from_utf8(prz.path()).unwrap(), sm.iter().map(|(p, v)| (String::from_utf8(p).unwrap(), v)).collect::<Vec<_>>());
                }
            }
            if !prz.to_next_k_path(result_depth) { break }
        }
        assert_eq!(results, vec![
            ("on".into(), vec![("cann".into(), 2), ("rubic".into(), 9)]),
            ("ow".into(), vec![("arr".into(), 0), ("b".into(), 1)]),
            ("us".into(), vec![("roman".into(), 5), ("romul".into(), 6), ("rubicund".into(), 10)])
        ])
    }
}