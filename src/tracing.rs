use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;
use std::sync::{Arc, OnceLock, RwLock};

#[derive(Clone)]
struct SharedWriter {
    inner: Arc<RwLock<Option<std::fs::File>>>,
}

struct MultiWriter {
    inner: Arc<RwLock<Option<std::fs::File>>>,
}

impl SharedWriter {
    fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(None)),
        }
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for SharedWriter {
    type Writer = MultiWriter;

    fn make_writer(&'a self) -> Self::Writer {
        MultiWriter {
            inner: self.inner.clone(),
        }
    }
}

impl Write for MultiWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = io::stderr().write(buf)?;
        if let Some(file) = &mut *self.inner.write().unwrap() {
            let _ = file.write_all(buf);
        }
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        io::stderr().flush()?;
        if let Some(file) = &mut *self.inner.write().unwrap() {
            let _ = file.flush();
        }
        Ok(())
    }
}

static WRITER: OnceLock<SharedWriter> = OnceLock::new();

pub fn init() {
    let _ = tracing_log::LogTracer::init();

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    let writer = SharedWriter::new();
    let _ = WRITER.set(writer.clone());

    let _ = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(writer)
        .try_init();
}

pub fn set_log_file(log_file: Option<&Path>) {
    if let Some(writer) = WRITER.get() {
        let mut guard = writer.inner.write().unwrap();
        if let Some(path) = log_file {
            if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(file) = OpenOptions::new().create(true).append(true).open(path) {
                *guard = Some(file);
            }
        } else {
            *guard = None;
        }
    }
}
