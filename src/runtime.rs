use std::sync::atomic;

use tokio::runtime::{Builder, Runtime};

use crate::error::{Error, Result};

pub(crate) fn initialize() -> Result<Runtime> {
    Builder::new_multi_thread()
        .worker_threads(4)
        .max_threads(32)
        .enable_time()
        .enable_io()
        .thread_name_fn(|| {
            static ATOMIC_ID: atomic::AtomicUsize = atomic::AtomicUsize::new(0);
            let id = ATOMIC_ID.fetch_add(1, atomic::Ordering::SeqCst);
            format!("Runtime{}", id)
        })
        .build()
        .map_err(Error::runtime)
}
