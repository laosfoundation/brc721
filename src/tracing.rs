use std::fs::OpenOptions;
use std::path::Path;

pub fn init(log_file: Option<&Path>) {
    use tracing_subscriber::prelude::*;
    let _ = tracing_log::LogTracer::init();

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    let file_layer = log_file.and_then(|log_file| {
        if let Some(parent) = log_file.parent().filter(|p| !p.as_os_str().is_empty()) {
            let _ = std::fs::create_dir_all(parent);
        }
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_file)
            .ok()
            .map(|file| {
                let (writer, guard) = tracing_appender::non_blocking(file);
                std::mem::forget(guard);
                tracing_subscriber::fmt::layer()
                    .with_ansi(false)
                    .with_writer(writer)
            })
    });

    let stderr_layer = tracing_subscriber::fmt::layer().with_writer(std::io::stderr);

    let _ = tracing_subscriber::registry()
        .with(env_filter)
        .with(stderr_layer)
        .with(file_layer)
        .try_init();
}
