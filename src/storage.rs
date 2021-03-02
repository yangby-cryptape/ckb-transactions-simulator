use std::{path::Path, str::FromStr};

use ckb_types::{core, packed, prelude::*};

use crate::{
    error::{Error, Result},
    types::{CellInfo, InputInfo, MetaData},
};

const KEY_METADATA: &[u8] = b"metadata";
const KEY_NEXT_BLOCK_NUMBER: &[u8] = b"next-block-number";

pub(crate) struct Storage {
    db: rocksdb::DB,
}

impl Storage {
    const CF_CACHE: &'static str = "cache";
    const CF_CELLS: &'static str = "cells";

    const CF_NAMES: &'static [&'static str] = &[Self::CF_CACHE, Self::CF_CELLS];

    pub(crate) fn init<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        if path.exists() {
            let errmsg = format!("the directory [{}] alreay exists", path.display());
            Err(Error::storage(errmsg))
        } else {
            Self::open(path, true)
        }
    }

    pub(crate) fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        if !path.exists() || !path.is_dir() {
            let errmsg = format!("the directory [{}] doesn't exists", path.display());
            return Err(Error::storage(errmsg));
        }
        Self::open(path, false)
    }

    fn open<P: AsRef<Path>>(path: P, create: bool) -> Result<Self> {
        let opts = Self::default_dboptions(create);
        let cfs = Self::default_column_family_descriptors();
        let db = rocksdb::DB::open_cf_descriptors(&opts, &path, cfs)?;
        Ok(Self { db })
    }

    fn default_dboptions(create: bool) -> rocksdb::Options {
        let mut opts = rocksdb::Options::default();
        if create {
            opts.create_if_missing(true);
            opts.create_missing_column_families(true);
        } else {
            opts.create_if_missing(false);
            opts.create_missing_column_families(false);
        }
        // DBOptions
        opts.set_bytes_per_sync(1 << 20);
        opts.set_max_background_jobs(4);
        opts.set_max_total_wal_size((1 << 20) * 64);
        opts.set_keep_log_file_num(64);
        opts.set_max_open_files(64);
        // CFOptions "default"
        opts.set_level_compaction_dynamic_level_bytes(true);
        opts.set_write_buffer_size((1 << 20) * 8);
        opts.set_min_write_buffer_number_to_merge(1);
        opts.set_max_write_buffer_number(2);
        opts.set_max_write_buffer_size_to_maintain(-1);
        // [TableOptions/BlockBasedTable "default"]
        let block_opts = {
            let mut block_opts = rocksdb::BlockBasedOptions::default();
            block_opts.set_cache_index_and_filter_blocks(true);
            block_opts.set_pin_l0_filter_and_index_blocks_in_cache(true);
            block_opts
        };

        opts.set_block_based_table_factory(&block_opts);

        opts
    }

    fn default_cfoptions() -> rocksdb::Options {
        let mut opts = rocksdb::Options::default();
        opts.set_level_compaction_dynamic_level_bytes(true);
        opts.set_write_buffer_size((1 << 20) * 8);
        opts.set_min_write_buffer_number_to_merge(1);
        opts.set_max_write_buffer_number(2);
        opts.set_max_write_buffer_size_to_maintain(-1);
        opts
    }

    fn default_column_family_descriptors() -> Vec<rocksdb::ColumnFamilyDescriptor> {
        let cfopts = Self::default_cfoptions();
        Self::CF_NAMES
            .iter()
            .map(|name| rocksdb::ColumnFamilyDescriptor::new(name.to_owned(), cfopts.clone()))
            .collect()
    }
}

impl Storage {
    fn cf_handle(&self, cf_name: &str) -> Result<&rocksdb::ColumnFamily> {
        self.db.cf_handle(cf_name).ok_or_else(|| {
            let errmsg = format!("column family {} should exists", cf_name);
            Error::storage(errmsg)
        })
    }

    pub(crate) fn put_metadata(&self, metadata: &MetaData) -> Result<()> {
        self.db
            .put(KEY_METADATA, metadata.to_string().as_bytes())
            .map_err(Into::into)
    }

    pub(crate) fn get_metadata(&self) -> Result<MetaData> {
        self.db
            .get(KEY_METADATA)
            .map_err::<Error, _>(Into::into)?
            .map(|slice| String::from_utf8(slice).map_err(Error::storage))
            .transpose()?
            .map(|s| FromStr::from_str(&s).map_err(Error::storage))
            .transpose()?
            .ok_or_else(|| Error::storage("can not found the metadata"))
    }

    pub(crate) fn put_prev_number(&self, number: core::BlockNumber) -> Result<()> {
        let number_be = number.to_be_bytes();
        self.db
            .put(KEY_NEXT_BLOCK_NUMBER, &number_be)
            .map_err(Into::into)
    }

    pub(crate) fn get_next_number(&self) -> Result<Option<core::BlockNumber>> {
        self.db
            .get(KEY_NEXT_BLOCK_NUMBER)
            .map_err::<Error, _>(Into::into)
            .map(|opt| {
                opt.map(|slice| {
                    let mut number_be = [0u8; 8];
                    number_be.copy_from_slice(&slice[0..8]);
                    core::BlockNumber::from_be_bytes(number_be) + 1
                })
            })
    }

    pub(crate) fn add_cell(&self, op: packed::OutPoint, info: CellInfo) -> Result<()> {
        let cf_cells = self.cf_handle(Self::CF_CELLS)?;
        self.db
            .put_cf(cf_cells, op.as_slice(), info.to_vec().as_slice())
            .map_err(Into::into)
    }

    pub(crate) fn spend_cell(&self, op: packed::OutPoint) -> Result<()> {
        let cf_cells = self.cf_handle(Self::CF_CELLS)?;
        let cf_cache = self.cf_handle(Self::CF_CACHE)?;
        let cap = self.db.get_cf(cf_cells, op.as_slice())?.ok_or_else(|| {
            let errmsg = format!("cell {} should exists", op);
            Error::storage(errmsg)
        })?;
        self.db.delete_cf(cf_cells, op.as_slice())?;
        self.db
            .put_cf(cf_cache, op.as_slice(), cap.as_slice())
            .map_err(Into::into)
    }

    pub(crate) fn rm_cell(&self, op: packed::OutPoint) -> Result<()> {
        let cf_cells = self.cf_handle(Self::CF_CELLS)?;
        let cf_cache = self.cf_handle(Self::CF_CACHE)?;
        self.db.delete_cf(cf_cells, op.as_slice())?;
        self.db.delete_cf(cf_cache, op.as_slice())?;
        Ok(())
    }

    pub(crate) fn load_cells(&self) -> Result<Vec<InputInfo>> {
        let cf_cells = self.cf_handle(Self::CF_CELLS)?;
        self.db
            .full_iterator_cf(cf_cells, rocksdb::IteratorMode::Start)
            .map(|(key, value)| {
                let op = packed::OutPoint::from_slice(&key).map_err(Error::storage)?;
                let info = CellInfo::from_slice(&value);
                Ok(InputInfo::new(op, info))
            })
            .collect()
    }
}
