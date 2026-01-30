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
                tracing_subscriber::fmt::layer()
                    .with_ansi(false)
                    .with_writer(file)
            })
    });

    let stderr_layer = tracing_subscriber::fmt::layer().with_writer(std::io::stderr);

    let _ = tracing_subscriber::registry()
        .with(env_filter)
        .with(stderr_layer)
        .with(file_layer)
        .try_init();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::thread::sleep;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    use tempfile::TempDir;

    #[test]
    fn init_writes_logs_to_file() {
        let dir = TempDir::new().expect("temp dir");
        let log_path = dir.path().join("brc721.log");

        init(Some(&log_path));

        let marker = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        log::info!("file log test marker={}", marker);

        for _ in 0..20 {
            if let Ok(contents) = fs::read_to_string(&log_path) {
                if contents.contains(&format!("marker={}", marker)) {
                    return;
                }
            }
            sleep(Duration::from_millis(50));
        }

        let contents = fs::read_to_string(&log_path).unwrap_or_default();
        panic!("expected log marker in file, got: {}", contents);
    }
}
