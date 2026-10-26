//! tracing setup. Secrets are kept out of logs by construction: they live in
//! `Secret<T>` (prints `***`) and HTTP header values carrying tokens are
//! marked sensitive. Noisy transport crates are capped at `warn` regardless.

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter, Layer};

/// The app's log filter. The transport caps are hard-coded and apply even
/// with `--debug`; in particular `tungstenite` logs the raw websocket
/// handshake (including the `Authorization` header) at TRACE, so it must
/// never be allowed below `warn`. `tests/no_secrets_in_logs.rs` enforces this.
/// `RUST_LOG` is deliberately not consulted.
pub fn filter(debug: bool) -> EnvFilter {
    let level = if debug { "debug" } else { "info" };
    EnvFilter::new(format!(
        "{level},hyper=warn,hyper_util=warn,reqwest=warn,rustls=warn,tungstenite=warn,tokio_tungstenite=warn,ehttp=warn,winit=warn,eframe=warn,egui_glow=warn,egui-wgpu=warn,egui_wgpu=warn,wgpu=error,wgpu_core=error,wgpu_hal=error,naga=warn"
    ))
}

/// Initialise logging to stderr and, if possible, a daily-rotated file in
/// the cache dir. Returns the file writer guard, which must be kept alive.
pub fn init(debug: bool) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let stderr = fmt::layer().with_writer(std::io::stderr).with_filter(filter(debug));
    let file = crate::config::cache_dir().and_then(|dir| {
        tracing_appender::rolling::Builder::new()
            .rotation(tracing_appender::rolling::Rotation::DAILY)
            .filename_prefix("tracked")
            .filename_suffix("log")
            .max_log_files(5)
            .build(dir.join("logs"))
            .ok()
    });
    match file {
        Some(appender) => {
            let (writer, guard) = tracing_appender::non_blocking(appender);
            let file_layer = fmt::layer()
                .with_ansi(false)
                .with_writer(writer)
                .with_filter(filter(debug));
            tracing_subscriber::registry().with(stderr).with(file_layer).init();
            Some(guard)
        }
        None => {
            tracing_subscriber::registry().with(stderr).init();
            None
        }
    }
}
