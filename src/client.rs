use std::sync::Arc;

use ckb_jsonrpc_types as rpc;
use ckb_types::{core, H256};
use futures::compat::Future01CompatExt;
use jsonrpc_core::futures::Future as _;
use jsonrpc_core_client::transports::http;
use jsonrpc_derive::rpc;
use parking_lot::RwLock;
use tokio::{runtime, sync::oneshot};
use tokio01::runtime as runtime01;
use url::Url;

use crate::error::{Error, Result};

pub struct Client {
    client: gen_client::Client,
    runtime: Arc<RwLock<runtime::Runtime>>,
    _runtime01: Arc<RwLock<runtime01::Runtime>>,
}

#[rpc(client)]
trait CkbRpc {
    #[rpc(name = "get_tip_block_number")]
    fn get_tip_block_number(&self) -> Result<rpc::BlockNumber>;

    #[rpc(name = "get_header_by_number")]
    fn get_header_by_number(
        &self,
        block_number: rpc::BlockNumber,
    ) -> Result<Option<rpc::HeaderView>>;

    #[rpc(name = "get_block_by_number")]
    fn get_block_by_number(&self, block_number: rpc::BlockNumber)
        -> Result<Option<rpc::BlockView>>;

    #[rpc(name = "send_transaction")]
    fn send_transaction(
        &self,
        tx: rpc::Transaction,
        outputs_validator: Option<rpc::OutputsValidator>,
    ) -> Result<H256>;
}

fn initialize(rt: runtime::Runtime, url: &Url) -> Result<Client> {
    log::trace!("initialize a JSON-RPC client ...");
    let fut_client_conn = http::connect::<gen_client::Client>(url.as_str());

    let (tx1, mut rx1) = oneshot::channel();
    let (tx2, mut rx2) = oneshot::channel();

    log::trace!("run a legacy runtime to connect");
    let mut rt01 = runtime01::Builder::new()
        .core_threads(4)
        .blocking_threads(4)
        .name_prefix("LegacyRT")
        .build()
        .map_err(|err| {
            Error::runtime(format!("failed to create a legacy runtime since {}", err))
        })?;
    rt01.spawn(
        fut_client_conn
            .map(|client| {
                log::trace!("connect successfully");
                if tx1.send(client).is_err() {
                    log::error!("failed to send ok to main thread");
                }
            })
            .map_err(|error| {
                log::trace!("connect unsuccessfully");
                if tx2.send(error).is_err() {
                    log::error!("failed to send err to main thread");
                }
            }),
    );
    log::trace!("waiting for the client");
    let client = rt.block_on(async {
        loop {
            tokio::select! {
                Ok(client) = (&mut rx1) => {
                log::trace!("select client successfully");
                    return Ok(client);
                },
                Ok(error) = (&mut rx2) => {
                log::trace!("select client unsuccessfully");
                    return Err(Error::client(error));
                },
                else => {
                    log::error!("failed to select the result of the client connection");
                    return Err(Error::client("no result for the connection"));
                }
            }
        }
    })?;
    Ok(Client {
        client,
        runtime: Arc::new(RwLock::new(rt)),
        _runtime01: Arc::new(RwLock::new(rt01)),
    })
}

impl Client {
    pub fn new(url: &Url) -> Result<Client> {
        let rt = crate::runtime::initialize()?;
        initialize(rt, url)
    }

    pub fn get_tip_block_number(&self) -> Result<core::BlockNumber> {
        let fut = self.client.get_tip_block_number();
        self.runtime
            .write()
            .block_on(fut.compat())
            .map_err(Error::client)
            .map(Into::into)
    }

    pub fn get_header_by_number(
        &self,
        block_number: core::BlockNumber,
    ) -> Result<Option<rpc::HeaderView>> {
        let fut = self.client.get_header_by_number(block_number.into());
        self.runtime
            .write()
            .block_on(fut.compat())
            .map_err(Error::client)
    }

    pub fn get_block_by_number(
        &self,
        block_number: core::BlockNumber,
    ) -> Result<Option<rpc::BlockView>> {
        let fut = self.client.get_block_by_number(block_number.into());
        self.runtime
            .write()
            .block_on(fut.compat())
            .map_err(Error::client)
    }

    pub fn send_transaction(&self, tx: rpc::Transaction) -> Result<H256> {
        let fut = self.client.send_transaction(tx, None);
        self.runtime
            .write()
            .block_on(fut.compat())
            .map_err(Error::client)
    }
}
