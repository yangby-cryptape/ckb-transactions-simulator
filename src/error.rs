use std::{fmt, result};

use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("config error: {0}")]
    Config(String),
    #[error("runtime error: {0}")]
    Runtime(String),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("client error: {0}")]
    Client(String),

    #[error("url error: {0}")]
    Url(#[from] url::ParseError),
    #[error("crypto error: {0}")]
    Crypto(String),
    #[error("db error: {0}")]
    Db(#[from] rocksdb::Error),
}

pub type Result<T> = result::Result<T, Error>;

impl Error {
    pub(crate) fn config<T: fmt::Display>(inner: T) -> Self {
        Self::Config(inner.to_string())
    }
    pub(crate) fn runtime<T: fmt::Display>(inner: T) -> Self {
        Self::Runtime(inner.to_string())
    }
    pub(crate) fn storage<T: fmt::Display>(inner: T) -> Self {
        Self::Storage(inner.to_string())
    }
    pub(crate) fn client<T: fmt::Display>(inner: T) -> Self {
        Self::Client(inner.to_string())
    }
    pub(crate) fn argument_should_exist(name: &str) -> Self {
        Self::Config(format!("argument {} should exist", name))
    }
}

impl From<ckb_crypto::secp::Error> for Error {
    fn from(error: ckb_crypto::secp::Error) -> Self {
        Error::Crypto(error.to_string())
    }
}
