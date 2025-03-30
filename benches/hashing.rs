use std::fs::File;
use std::io::Read;
use divan::{Divan, Bencher, black_box};
use pathmap::trie_map::BytesTrieMap;
use pathmap::path_serialization::{deserialize_paths_, serialize_paths_};

#[divan::bench()]
fn big_logic_hash(bencher: Bencher) {
  let mut map = BytesTrieMap::new();
  let file_path = std::path::PathBuf::from(file!()).parent().unwrap().join("big_logic.metta.paths");
  let mut file = File::open(file_path).unwrap();
  // don't read directly from file, we want to avoid disk and caching funny business
  let mut in_buffer = vec![];
  file.read_to_end(&mut in_buffer).unwrap();
  let wz = map.write_zipper();
  let pathmap::path_serialization::DeserializationStats { path_count : total_paths , .. }=
    deserialize_paths_(wz, &in_buffer[..], ()).expect("deserialization error");
  assert_eq!(total_paths, 91692);
  assert_eq!(map.val_count(), 91692);

  bencher.bench_local(|| {
    black_box(map.hash2(|_| 0));
    // black_box(map.reference_equiv(map, |x, y| x == y))
  });

  black_box(map);
}

fn main() {
  // Run registered benchmarks.
  let divan = Divan::from_args()
    .sample_count(5);

  divan.main();
}
