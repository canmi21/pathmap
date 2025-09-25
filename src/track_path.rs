use crate::{
    utils::ByteMask,
    zipper::{
        Zipper, ZipperAbsolutePath, ZipperMoving, ZipperIteration,
        ZipperPath, ZipperPathBuffer, ZipperValues,
        ZipperReadOnlyValues, ZipperReadOnlyConditionalValues,
    },
};

/// Wrapper for blind zippers that allows tracking path
///
/// This allows having nested virtual zippers that don't maintain their
/// own path buffer, such that they don't repeat the work of copying paths.
pub struct TrackPath<Z> {
    zipper: Z,
    path: Vec<u8>,
    origin_len: usize,
}

impl<Z: ZipperMoving> TrackPath<Z> {
    pub fn new(mut zipper: Z) -> Self {
        zipper.reset();
        Self {
            zipper,
            path: Vec::new(),
            origin_len: 0,
        }
    }
    pub fn with_origin(mut zipper: Z, origin: &[u8]) -> Self {
        zipper.reset();
        Self {
            zipper,
            path: origin.to_vec(),
            origin_len: origin.len(),
        }
    }
}

impl<Z: Zipper> Zipper for TrackPath<Z> {
    #[inline] fn path_exists(&self) -> bool { self.zipper.path_exists() }
    #[inline] fn is_val(&self) -> bool { self.zipper.is_val() }
    #[inline] fn child_count(&self) -> usize { self.zipper.child_count() }
    #[inline] fn child_mask(&self) -> ByteMask { self.zipper.child_mask() }
}
impl<Z: ZipperMoving> ZipperMoving for TrackPath<Z> {
    #[inline] fn at_root(&self) -> bool { self.zipper.at_root() }
    fn reset(&mut self) {
        self.zipper.reset();
        self.path.truncate(self.origin_len);
    }
    fn val_count(&self) -> usize { todo!() }
    fn descend_to<K: AsRef<[u8]>>(&mut self, path: K) -> bool {
        let path = path.as_ref();
        self.path.extend_from_slice(path);
        self.zipper.descend_to(path)
    }
    fn descend_to_existing<K: AsRef<[u8]>>(&mut self, path: K) -> usize {
        let path = path.as_ref();
        let descended = self.zipper.descend_to_existing(path);
        self.path.extend_from_slice(&path[..descended]);
        descended
    }
    fn descend_to_val<K: AsRef<[u8]>>(&mut self, path: K) -> usize {
        let path = path.as_ref();
        let descended = self.zipper.descend_to_val(path);
        self.path.extend_from_slice(&path[..descended]);
        descended
    }
    fn descend_to_byte(&mut self, k: u8) -> bool {
        self.path.push(k);
        self.zipper.descend_to_byte(k)
    }
    fn ascend(&mut self, steps: usize) -> Result<(), usize> {
        let rv = self.zipper.ascend(steps);
        let ascended = match rv {
            Ok(()) => steps,
            Err(remaining) => steps - remaining,
        };
        let orig_len = self.path.len();
        self.path.truncate(orig_len - ascended);
        rv
    }
    fn ascend_byte(&mut self) -> bool {
        if !self.zipper.ascend_byte() {
            return false;
        }
        self.path.pop();
        true
    }
    fn ascend_until(&mut self) -> Option<usize> {
        let ascended = self.zipper.ascend_until()?;
        let orig_len = self.path.len();
        self.path.truncate(orig_len - ascended);
        Some(ascended)
    }
    fn ascend_until_branch(&mut self) -> Option<usize> {
        let ascended = self.zipper.ascend_until_branch()?;
        let orig_len = self.path.len();
        self.path.truncate(orig_len - ascended);
        Some(ascended)
    }
    fn to_next_sibling_byte(&mut self) -> Option<u8> {
        let byte = self.zipper.to_next_sibling_byte()?;
        let last = self.path.last_mut().expect("path must not be empty");
        *last = byte;
        Some(byte)
    }
    fn to_prev_sibling_byte(&mut self) -> Option<u8> {
        let byte = self.zipper.to_prev_sibling_byte()?;
        let last = self.path.last_mut().expect("path must not be empty");
        *last = byte;
        Some(byte)
    }
    fn descend_indexed_byte(&mut self, child_idx: usize) -> Option<u8> {
        let byte = self.zipper.descend_indexed_byte(child_idx)?;
        self.path.push(byte);
        Some(byte)
    }
    fn descend_first_byte(&mut self) -> Option<u8> {
        let byte = self.zipper.descend_first_byte()?;
        self.path.push(byte);
        Some(byte)
    }
    fn descend_until(&mut self, dst: Option<&mut Vec<u8>>) -> bool {
        let orig_len = self.path.len();
        let descended = self.zipper.descend_until(Some(&mut self.path));
        if let Some(dst) = dst {
            dst.extend_from_slice(&self.path[orig_len..]);
        }
        descended
    }
    // TODO: using default impl. re-using zipper's own `to_next_step` implementation
    // would require changing the API such that path can be updated.
    // fn to_next_step(&mut self) -> bool;
}

impl<Z: ZipperMoving> ZipperIteration for TrackPath<Z> { }

impl<Z: ZipperMoving> ZipperPath for TrackPath<Z> {
    fn path(&self) -> &[u8] { &self.path[self.origin_len..] }
}

impl<Z: ZipperMoving> ZipperAbsolutePath for TrackPath<Z> {
    fn origin_path(&self) -> &[u8] { &self.path }
    fn root_prefix_path(&self) -> &[u8] { &self.path[..self.origin_len] }
}

impl<Z: ZipperValues<V>, V> ZipperValues<V> for TrackPath<Z> {
    fn val(&self) -> Option<&V> { self.zipper.val() }
}

impl<'a, Z: ZipperReadOnlyValues<'a, V>, V> ZipperReadOnlyValues<'a, V> for TrackPath<Z> {
    fn get_val(&self) -> Option<&'a V> { self.zipper.get_val() }
}

impl<'a, Z: ZipperReadOnlyConditionalValues<'a, V>, V> ZipperReadOnlyConditionalValues<'a, V> for TrackPath<Z> {
    type WitnessT = Z::WitnessT;
    fn witness<'w>(&self) -> Self::WitnessT { self.zipper.witness() }
    fn get_val_with_witness<'w>(&self, witness: &'w Self::WitnessT) -> Option<&'w V> where 'a: 'w {
        self.zipper.get_val_with_witness(witness)
    }
}

impl<Z: ZipperMoving> ZipperPathBuffer for TrackPath<Z> {
    unsafe fn origin_path_assert_len(&self, len: usize) -> &[u8] {
        let ptr = self.path.as_ptr();
        unsafe { core::slice::from_raw_parts(ptr, len) }
    }
    fn prepare_buffers(&mut self) { }
    fn reserve_buffers(&mut self, path_len: usize, _stack: usize) {
        self.path.reserve(path_len);
    }
}

#[cfg(test)]
mod tests {
    use super::{TrackPath};
    use crate::{
        PathMap,
        zipper::{zipper_iteration_tests, zipper_moving_tests},
    };

    zipper_moving_tests::zipper_moving_tests!(track_path,
        |keys: &[&[u8]]| {
            keys.into_iter().map(|k| (k, ())).collect::<PathMap<()>>()
        },
        |trie: &mut PathMap<()>, path: &[u8]| {
            TrackPath::with_origin(trie.read_zipper_at_path(path), path)
        }
    );

    zipper_iteration_tests::zipper_iteration_tests!(track_path,
        |keys: &[&[u8]]| {
            keys.into_iter().map(|k| (k, ())).collect::<PathMap<()>>()
        },
        |trie: &mut PathMap<()>, path: &[u8]| {
            TrackPath::with_origin(trie.read_zipper_at_path(path), path)
        }
    );
}
