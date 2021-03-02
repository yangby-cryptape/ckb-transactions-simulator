use ckb_types::{core, packed, prelude::*, H256};

#[derive(Debug, Clone)]
pub(crate) struct CellInfo {
    pub(crate) capacity: core::Capacity,
    pub(crate) lock_hash: H256,
}

impl CellInfo {
    pub(crate) fn new(capacity: core::Capacity, lock_hash: H256) -> Self {
        Self {
            capacity,
            lock_hash,
        }
    }

    pub(crate) fn to_vec(&self) -> Vec<u8> {
        let mut output = [0u8; 8 + 32];
        let cap: packed::Uint64 = self.capacity.pack();
        let hash: packed::Byte32 = self.lock_hash.pack();
        (&mut output[0..8]).copy_from_slice(cap.as_slice());
        (&mut output[8..40]).copy_from_slice(hash.as_slice());
        output.to_vec()
    }

    pub(crate) fn from_slice(slice: &[u8]) -> Self {
        let cap: core::Capacity =
            packed::Uint64::new_unchecked((&slice[0..8]).to_vec().into()).unpack();
        let hash: H256 = packed::Byte32::new_unchecked((&slice[8..40]).to_vec().into()).unpack();
        Self::new(cap, hash)
    }
}
