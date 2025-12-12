use crate::plugin::Plugin;
use bytes::Bytes;
use log::trace;
use std::sync::{Arc, Mutex};
use uuid::Uuid;
use wasmtime_wasi::{HostOutputStream, StdoutStream, StreamResult, Subscribe, async_trait};
use wiggle::tracing::error;

pub struct PluginStdout {
    pub app_id: Uuid,
    pub plugin: Arc<Mutex<Option<Plugin>>>,
}

pub struct PluginStderr {
    pub app_id: Uuid,
    pub plugin: Arc<Mutex<Option<Plugin>>>,
}

struct PluginStdOutErrWriter {
    is_stderr: bool,
    app_id: Uuid,
    plugin: Option<Plugin>,
}

impl StdoutStream for PluginStdout {
    fn stream(&self) -> Box<dyn HostOutputStream> {
        Box::new(PluginStdOutErrWriter {
            is_stderr: false,
            app_id: self.app_id,
            plugin: self.plugin.lock().unwrap().clone(),
        })
    }

    fn isatty(&self) -> bool {
        false
    }
}

impl StdoutStream for PluginStderr {
    fn stream(&self) -> Box<dyn HostOutputStream> {
        Box::new(PluginStdOutErrWriter {
            is_stderr: true,
            app_id: self.app_id,
            plugin: self.plugin.lock().unwrap().clone(),
        })
    }

    fn isatty(&self) -> bool {
        false
    }
}

#[async_trait]
impl Subscribe for PluginStdOutErrWriter {
    async fn ready(&mut self) {}
}

impl HostOutputStream for PluginStdOutErrWriter {
    fn write(&mut self, buf: Bytes) -> Result<(), wasmtime_wasi::StreamError> {
        let output = String::from_utf8_lossy(&buf);
        let std_type = match self.is_stderr {
            true => "out",
            false => "err",
        };
        let Some(plugin) = self.plugin.clone() else {
            error!(
                "received std{std_type} for app {} with missing plugin",
                self.app_id
            );
            return Ok(());
        };
        // TODO: allow a sink arg for the logs?
        trace!(
            "[app {}] [plugin {}] [std{std_type}] {output}",
            self.app_id,
            plugin.id(),
        );
        Ok(())
    }

    fn flush(&mut self) -> Result<(), wasmtime_wasi::StreamError> {
        // No buffering, so nothing to flush.
        Ok(())
    }

    fn check_write(&mut self) -> StreamResult<usize> {
        Ok(usize::MAX)
    }
}
