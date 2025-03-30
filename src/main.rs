#![allow(warnings)]

use std::alloc::{alloc, dealloc, Layout};
use std::ptr;
use std::time::Instant;
use pathmap::ring::*;
use pathmap::trie_map::BytesTrieMap;

use std::fs::File;
use std::hint::black_box;
use std::io::Read;
use pathmap::path_serialization::{deserialize_paths_, serialize_paths_};

fn main() {
    let mut map = BytesTrieMap::new();
    let file_path = std::path::PathBuf::from(file!()).parent().unwrap().join("/home/adam/Projects/PathMap/benches/big_logic.metta.paths");
    let mut file = File::open(file_path).unwrap();
    // don't read directly from file, we want to avoid disk and caching funny business
    let mut in_buffer = vec![];
    file.read_to_end(&mut in_buffer).unwrap();
    let wz = map.write_zipper();
    let pathmap::path_serialization::DeserializationStats { path_count : total_paths , .. }=
      deserialize_paths_(wz, &in_buffer[..], ()).expect("deserialization error");
    assert_eq!(total_paths, 91692);
    // assert_eq!(map.val_count(), 91692);

    for _ in 0..100 {
        let t0 = Instant::now();
        println!("{} {} micros", map.hash(|_| 0), t0.elapsed().as_micros());
    }

    black_box(map);

}
