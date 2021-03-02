mod init;
mod run;
mod storage;

pub(crate) use init::*;
pub(crate) use run::*;
pub(crate) use storage::*;

use ckb_types::{bytes, packed};

use crate::error::Result;

#[derive(Debug, Clone)]
pub(crate) struct InputInfo {
    pub(crate) out_point: packed::OutPoint,
    pub(crate) cell_info: CellInfo,
}

#[derive(Debug, Clone)]
pub(crate) struct LockInfo {
    pub(crate) id: LockScriptId,
    pub(crate) script: packed::Script,
    pub(crate) secret_key: bytes::Bytes,
}

impl InputInfo {
    pub(crate) fn new(out_point: packed::OutPoint, cell_info: CellInfo) -> Self {
        Self {
            out_point,
            cell_info,
        }
    }
}

impl LockInfo {
    pub(crate) fn new(id: LockScriptId, script: packed::Script, secret_key: bytes::Bytes) -> Self {
        Self {
            id,
            script,
            secret_key,
        }
    }

    pub(crate) fn sign(&self, data: &[u8]) -> Result<Vec<u8>> {
        self.id.sign(&self.secret_key, data)
    }
}
