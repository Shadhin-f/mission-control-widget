//! The backend modules in `rust/src-tauri/src` (stats, network, AI bridges, cleaner...) are shared with
//! the Tauri edition. They only ever use three things from Tauri: an `AppHandle` to emit events, the
//! `Emitter` trait and `async_runtime::spawn`. The native edition depends on this crate under the name
//! `tauri`, so those files compile unchanged and both editions run the same backend code.
use once_cell::sync::OnceCell;
use serde_json::Value;
use std::sync::Arc;

type Sink = Arc<dyn Fn(&str, Value) + Send + Sync>;

static SINK: OnceCell<Sink> = OnceCell::new();
static RUNTIME: OnceCell<tokio::runtime::Runtime> = OnceCell::new();

/// Where every backend event goes (the UI's event queue). Call once at start-up.
pub fn set_sink(f: impl Fn(&str, Value) + Send + Sync + 'static) {
    let _ = SINK.set(Arc::new(f));
}

/// One small multi-threaded runtime for all background work.
pub fn runtime() -> &'static tokio::runtime::Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .max_blocking_threads(8)
            .thread_name("mc-worker")
            .enable_all()
            .build()
            .expect("tokio runtime")
    })
}

#[derive(Clone, Default)]
pub struct AppHandle;

#[derive(Debug)]
pub struct Error;

pub trait Emitter {
    fn emit<S: serde::Serialize + Clone>(&self, event: &str, payload: S) -> Result<(), Error>;
}

impl Emitter for AppHandle {
    fn emit<S: serde::Serialize + Clone>(&self, event: &str, payload: S) -> Result<(), Error> {
        if let Some(sink) = SINK.get() {
            sink(event, serde_json::to_value(payload).unwrap_or(Value::Null));
        }
        Ok(())
    }
}

pub mod async_runtime {
    pub use tokio::task::JoinHandle;

    pub fn spawn<F>(f: F) -> JoinHandle<F::Output>
    where
        F: std::future::Future + Send + 'static,
        F::Output: Send + 'static,
    {
        crate::runtime().spawn(f)
    }
}
