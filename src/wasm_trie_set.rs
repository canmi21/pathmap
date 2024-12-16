use wasm_bindgen::prelude::*;
use crate::trie_map::*;


#[wasm_bindgen]
struct BytesTrieSet {
  btm: BytesTrieMap<()>
}

#[wasm_bindgen]
pub fn range_be_u32(start: u32, stop: u32, step: u32) -> BytesTrieSet {
  BytesTrieSet { btm: BytesTrieMap::<()>::range::<true, u32>(start, stop, step, ()) }
}

#[wasm_bindgen]
pub fn contains(m: &BytesTrieSet, k: &[u8]) -> bool {
  m.btm.contains(k)
}
