use std::fs::File;
use std::io::Read;
use divan::{Divan, Bencher, black_box};
use pathmap::PathMap;
use pathmap::path_serialization::{deserialize_paths_, serialize_paths_};

#[divan::bench()]
fn big_logic_serialize_paths(bencher: Bencher) {
  let file_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("benches").join("big_logic.metta.paths");
  let mut file = File::open(file_path).unwrap();
  let mut map = PathMap::new();
  let mut in_buffer = vec![];
  file.read_to_end(&mut in_buffer).unwrap();
  deserialize_paths_(map.write_zipper(), &in_buffer[..], ()).expect("deserialization error");
  // don't write directly to file, we want to avoid disk and caching funny business
  let mut out_buffer = Vec::with_capacity(in_buffer.len());
  bencher.bench_local(|| {
    let rz = map.read_zipper();
    unsafe { out_buffer.set_len(0) }
    let pathmap::path_serialization::SerializationStats { path_count : total_paths , .. }=
      serialize_paths_(rz, &mut out_buffer).expect("serialization error");
    assert_eq!(total_paths, 91692);
  });
  assert_eq!(in_buffer, out_buffer);
  black_box(out_buffer);
}

#[divan::bench()]
fn big_logic_deserialize_paths(bencher: Bencher) {
  let mut map = PathMap::new();
  let file_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("benches").join("big_logic.metta.paths");
  let mut file = File::open(file_path).unwrap();
  // don't read directly from file, we want to avoid disk and caching funny business
  let mut in_buffer = vec![];
  file.read_to_end(&mut in_buffer).unwrap();
  bencher.bench_local(|| {
    let wz = map.write_zipper();
    let pathmap::path_serialization::DeserializationStats { path_count : total_paths , .. }=
      deserialize_paths_(wz, &in_buffer[..], ()).expect("deserialization error");
    assert_eq!(total_paths, 91692);
  });
  assert_eq!(map.val_count(), 91692);
  black_box(map);
}

fn main() {
  // Run registered benchmarks.
  let divan = Divan::from_args()
    .sample_count(5);

  divan.main();
}
