use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

pub const LOG_FILE_NAME: &str = "app.log";

/// Writes every tracing event to both stdout and a file under the EDMS
/// root, so `/logs` has something to read back without losing console
/// output during local dev.
#[derive(Clone)]
pub struct AppLogWriter {
    file: Arc<Mutex<std::fs::File>>,
}

impl AppLogWriter {
    pub fn new(root: &Path) -> io::Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(root.join(LOG_FILE_NAME))?;
        Ok(Self {
            file: Arc::new(Mutex::new(file)),
        })
    }
}

impl Write for AppLogWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        io::stdout().write_all(buf)?;
        self.file.lock().unwrap().write_all(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        io::stdout().flush()?;
        self.file.lock().unwrap().flush()
    }
}
