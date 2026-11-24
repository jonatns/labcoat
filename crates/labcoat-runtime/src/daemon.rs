//! Run Labcoat Network as a background process: `labcoat node`.

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use crate::node::Node;
use crate::{IndexerModule, Runtime};

/// The file holding the chain inside the data directory.
pub const CHAIN_FILE: &str = "chain.log";
/// The file holding the serving process's id inside the data directory.
pub const PID_FILE: &str = "node.pid";

pub struct Options {
    /// The local port to serve JSON-RPC on.
    pub port: u16,
    /// Where the chain is saved.
    pub data_dir: PathBuf,
    /// The Alkanes indexer module.
    pub indexer: PathBuf,
    /// Where compiled indexer modules are cached.
    pub cache_dir: PathBuf,
}

/// Load the chain and serve it until a `stop` request arrives.
pub fn run(options: &Options) -> Result<(), String> {
    let started = Instant::now();
    std::fs::create_dir_all(&options.data_dir)
        .map_err(|error| format!("cannot create {}: {error}", options.data_dir.display()))?;

    // Bind first, so a second node fails before touching the chain file.
    let listener = TcpListener::bind(("127.0.0.1", options.port)).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AddrInUse {
            format!(
                "port {} is already in use; run `labcoat down`, or stop the process using it",
                options.port
            )
        } else {
            format!("cannot listen on port {}: {error}", options.port)
        }
    })?;

    let module = IndexerModule::load_cached(&options.indexer, &options.cache_dir)
        .map_err(|error| error.to_string())?;
    let loaded = started.elapsed();
    let runtime = Runtime::open(module, &options.data_dir.join(CHAIN_FILE))
        .map_err(|error| error.to_string())?;
    tracing::info!(
        "Labcoat Network ready at block {} on 127.0.0.1:{} (indexer loaded in {:?}, chain in {:?})",
        runtime.height(),
        options.port,
        loaded,
        started.elapsed() - loaded,
    );

    let pid_file = options.data_dir.join(PID_FILE);
    std::fs::write(&pid_file, std::process::id().to_string())
        .map_err(|error| format!("cannot write {}: {error}", pid_file.display()))?;
    let served = crate::server::serve(Arc::new(Node::new(runtime)), listener);
    remove_pid_file(&pid_file);
    served.map_err(|error| format!("Labcoat Network stopped: {error}"))
}

/// The id of the process serving the chain in `data_dir`, if one wrote it.
pub fn pid(data_dir: &Path) -> Option<u32> {
    std::fs::read_to_string(data_dir.join(PID_FILE))
        .ok()?
        .trim()
        .parse()
        .ok()
}

fn remove_pid_file(path: &Path) {
    // Only remove the file this process wrote.
    if std::fs::read_to_string(path).ok().as_deref().map(str::trim)
        == Some(std::process::id().to_string().as_str())
    {
        let _ = std::fs::remove_file(path);
    }
}
