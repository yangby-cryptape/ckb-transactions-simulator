use std::{collections::HashMap, fmt, result::Result as StdResult, str::FromStr};

use ckb_crypto::secp;
use ckb_hash::new_blake2b;
use ckb_jsonrpc_types as rpc;
use ckb_types::{bytes, core, packed, prelude::*, H256};
use serde::{Deserialize, Serialize};
use tiny_keccak::Hasher as _;

use super::LockInfo;
use crate::error::{Error, Result};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub(crate) struct MetaData {
    pub(crate) start_block: BlockMeta,
    pub(crate) lock_scripts: HashMap<LockScriptId, Script>,
    pub(crate) accounts: Vec<Account>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub(crate) struct BlockMeta {
    pub(crate) number: core::BlockNumber,
    pub(crate) hash: H256,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub(crate) struct Script {
    pub(crate) code_hash: H256,
    pub(crate) hash_type: ScriptHashType,
    pub(crate) cell_deps: Vec<CellDep>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub(crate) struct Account {
    pub(crate) secret_key: rpc::JsonBytes,
    pub(crate) lock_id: LockScriptId,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub(crate) struct CellDep {
    pub(crate) out_point: OutPoint,
    pub(crate) dep_type: DepType,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub(crate) struct OutPoint {
    pub tx_hash: H256,
    pub index: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ScriptHashType {
    Data,
    Type,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DepType {
    Code,
    DepGroup,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum LockScriptId {
    #[serde(rename = "secp256k1_blake160")]
    Secp256K1Blake160,
    #[serde(rename = "pwlock-k1-acpl")]
    PwLockK1Acpl,
}

impl FromStr for MetaData {
    type Err = serde_yaml::Error;
    fn from_str(s: &str) -> StdResult<Self, Self::Err> {
        serde_yaml::from_str(&s)
    }
}

impl fmt::Display for MetaData {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        serde_yaml::to_string(self)
            .map_err(|_| fmt::Error)
            .and_then(|s| write!(f, "{}", s))
    }
}

impl fmt::Display for LockScriptId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        serde_yaml::to_string(self)
            .map_err(|_| fmt::Error)
            .and_then(|s| write!(f, "{}", s))
    }
}

impl From<ScriptHashType> for core::ScriptHashType {
    fn from(input: ScriptHashType) -> core::ScriptHashType {
        match input {
            ScriptHashType::Data => core::ScriptHashType::Data,
            ScriptHashType::Type => core::ScriptHashType::Type,
        }
    }
}

impl From<DepType> for core::DepType {
    fn from(input: DepType) -> core::DepType {
        match input {
            DepType::Code => core::DepType::Code,
            DepType::DepGroup => core::DepType::DepGroup,
        }
    }
}
impl From<ScriptHashType> for packed::Byte {
    fn from(input: ScriptHashType) -> packed::Byte {
        Into::<core::ScriptHashType>::into(input).into()
    }
}

impl From<DepType> for packed::Byte {
    fn from(input: DepType) -> packed::Byte {
        Into::<core::DepType>::into(input).into()
    }
}

impl MetaData {
    pub(crate) fn accounts(&self) -> Result<HashMap<H256, LockInfo>> {
        self.accounts
            .iter()
            .map(|account| {
                let sk_bytes = account.clone().secret_key.into_bytes();
                let id = account.lock_id;
                let args = id.generate_args(&sk_bytes)?;
                let lock_script = self.lock_scripts.get(&id).ok_or_else(|| {
                    let errmsg = format!("lock scripts are not enough, requires {}", id);
                    Error::config(errmsg)
                })?;
                let hash_type: core::ScriptHashType = lock_script.hash_type.into();
                let script = packed::Script::new_builder()
                    .args(args.pack())
                    .code_hash(lock_script.code_hash.pack())
                    .hash_type(hash_type.into())
                    .build();
                let hash: H256 = script.calc_script_hash().unpack();
                let lock_info = LockInfo::new(id, script, sk_bytes);
                Ok((hash, lock_info))
            })
            .collect()
    }

    pub(crate) fn lock_deps_dict(&self) -> HashMap<LockScriptId, Vec<packed::CellDep>> {
        self.lock_scripts
            .iter()
            .map(|(key, value)| {
                let cell_deps = value.cell_deps.iter().map(Pack::pack).collect();
                (*key, cell_deps)
            })
            .collect()
    }
}

impl Pack<packed::OutPoint> for OutPoint {
    fn pack(&self) -> packed::OutPoint {
        packed::OutPoint::new_builder()
            .tx_hash(self.tx_hash.pack())
            .index(self.index.pack())
            .build()
    }
}

impl Pack<packed::CellDep> for CellDep {
    fn pack(&self) -> packed::CellDep {
        packed::CellDep::new_builder()
            .out_point(self.out_point.pack())
            .dep_type(self.dep_type.into())
            .build()
    }
}

impl LockScriptId {
    pub(crate) fn generate_args(self, sk_slice: &[u8]) -> Result<Vec<u8>> {
        let v = match self {
            Self::Secp256K1Blake160 => {
                let pk = secp::Privkey::from_slice(sk_slice).pubkey()?;
                let data = pk.serialize();
                {
                    let mut result = [0u8; 32];
                    let mut hasher = new_blake2b();
                    hasher.update(&data[..]);
                    hasher.finalize(&mut result);
                    (&result[..20]).to_vec()
                }
            }
            Self::PwLockK1Acpl => {
                let pk = secp::Privkey::from_slice(sk_slice).pubkey()?;
                let data = {
                    let mut temp = [4u8; 65];
                    temp[1..65].copy_from_slice(&pk.as_bytes());
                    let pk_raw =
                        secp256k1::PublicKey::from_slice(&temp).map_err(secp::Error::from)?;
                    Vec::from(&pk_raw.serialize_uncompressed()[1..])
                };
                {
                    let mut result = [0; 32];
                    let mut hasher = tiny_keccak::Keccak::v256();
                    hasher.update(&data);
                    hasher.finalize(&mut result);
                    (&result[12..]).to_vec()
                }
            }
        };
        Ok(v)
    }

    pub(crate) fn sign(self, sk_slice: &[u8], data: &[u8]) -> Result<Vec<u8>> {
        let signature = match self {
            Self::Secp256K1Blake160 => {
                let sk = secp::Privkey::from_slice(sk_slice);
                let message = {
                    let blank_signature = bytes::Bytes::from(vec![0u8; 65]);
                    let witness_blank = packed::WitnessArgs::new_builder()
                        .lock(Some(blank_signature).pack())
                        .build();
                    let witness_empty_len = witness_blank.as_bytes().len() as u64;

                    let mut message = [0u8; 32];
                    let mut hasher = new_blake2b();
                    hasher.update(data);
                    hasher.update(&witness_empty_len.to_le_bytes());
                    hasher.update(&witness_blank.as_bytes());
                    hasher.finalize(&mut message);
                    message
                };
                sk.sign_recoverable(&message.into())
                    .map(|sig| sig.serialize())?
            }
            Self::PwLockK1Acpl => {
                let sk = secp::Privkey::from_slice(sk_slice);
                let message_raw = {
                    let blank_signature = bytes::Bytes::from(vec![0u8; 65]);
                    let witness_blank = packed::WitnessArgs::new_builder()
                        .lock(Some(blank_signature).pack())
                        .build();
                    let witness_empty_len = witness_blank.as_bytes().len() as u64;

                    let mut result = [0u8; 32];
                    let mut hasher = tiny_keccak::Keccak::v256();
                    hasher.update(data);
                    hasher.update(&witness_empty_len.to_le_bytes());
                    hasher.update(&witness_blank.as_bytes());
                    hasher.finalize(&mut result);
                    result
                };
                let message = {
                    let mut result = [0u8; 32];
                    let mut hasher = tiny_keccak::Keccak::v256();
                    let prefix = format!("\x19Ethereum Signed Message:\n{}", result.len());
                    hasher.update(&prefix.as_bytes());
                    hasher.update(&message_raw);
                    hasher.finalize(&mut result);
                    result
                };
                sk.sign_recoverable(&message.into())
                    .map(|sig| sig.serialize())?
            }
        };
        Ok(signature)
    }
}
