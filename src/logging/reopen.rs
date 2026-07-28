use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use arc_swap::ArcSwap;
use tracing_subscriber::fmt::writer::MakeWriter;

pub struct ReopenableFileWriter {
    file: Arc<ArcSwap<File>>,
}

impl ReopenableFileWriter {
    pub fn new(path: &Path) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let file = File::create(path)?;
        Ok(Self {
            file: Arc::new(ArcSwap::from_pointee(file)),
        })
    }

    pub fn handle(&self) -> LogReopenHandle {
        LogReopenHandle {
            file: self.file.clone(),
            path: PathBuf::new(),
        }
    }

    pub fn handle_with_path(&self, path: PathBuf) -> LogReopenHandle {
        LogReopenHandle {
            file: self.file.clone(),
            path,
        }
    }
}

impl<'a> MakeWriter<'a> for ReopenableFileWriter {
    type Writer = ReopenableFileWriterHandle;
    fn make_writer(&'a self) -> Self::Writer {
        ReopenableFileWriterHandle {
            file: self.file.load_full(),
        }
    }
}

pub struct ReopenableFileWriterHandle {
    file: Arc<File>,
}

impl Write for ReopenableFileWriterHandle {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.file.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

pub struct LogReopenHandle {
    file: Arc<ArcSwap<File>>,
    path: PathBuf,
}

impl LogReopenHandle {
    pub fn reopen(&self) -> io::Result<()> {
        if self.path.as_os_str().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "log reopen path not configured",
            ));
        }
        let new_file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.path)?;
        self.file.store(Arc::new(new_file));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn reopen_swaps_underlying_file() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("access.log");

        let writer = ReopenableFileWriter::new(&log_path).unwrap();
        let handle = writer.handle_with_path(log_path.clone());

        let mut w = writer.make_writer();
        w.write_all(b"first").unwrap();
        w.flush().unwrap();

        std::fs::write(&log_path, "ROTATED").unwrap();

        handle.reopen().unwrap();

        let mut w = writer.make_writer();
        w.write_all(b"second").unwrap();
        w.flush().unwrap();

        let mut content = String::new();
        File::open(&log_path)
            .unwrap()
            .read_to_string(&mut content)
            .unwrap();
        assert_eq!(content, "second");
    }

    #[test]
    fn reopen_without_path_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("access.log");
        let writer = ReopenableFileWriter::new(&log_path).unwrap();
        let handle = writer.handle();
        let result = handle.reopen();
        assert!(result.is_err());
    }
}