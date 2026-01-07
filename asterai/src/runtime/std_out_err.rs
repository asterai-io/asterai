use crate::plugin::Plugin;
use log::trace;
use std::io::Error;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use tokio::io::AsyncWrite;
use uuid::Uuid;
use wasmtime_wasi::cli::{IsTerminal, StdoutStream};
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

impl IsTerminal for PluginStdout {
    fn is_terminal(&self) -> bool {
        false
    }
}

impl IsTerminal for PluginStderr {
    fn is_terminal(&self) -> bool {
        false
    }
}

impl StdoutStream for PluginStdout {
    fn async_stream(&self) -> Box<dyn AsyncWrite + Send + Sync> {
        Box::new(PluginStdOutErrWriter {
            is_stderr: false,
            app_id: self.app_id,
            plugin: self.plugin.lock().unwrap().clone(),
        })
    }
}

impl StdoutStream for PluginStderr {
    fn async_stream(&self) -> Box<dyn AsyncWrite + Send + Sync> {
        Box::new(PluginStdOutErrWriter {
            is_stderr: true,
            app_id: self.app_id,
            plugin: self.plugin.lock().unwrap().clone(),
        })
    }
}

impl AsyncWrite for PluginStdOutErrWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, Error>> {
        let output = String::from_utf8_lossy(buf);
        let std_type = match self.is_stderr {
            true => "err",
            false => "out",
        };
        if let Some(plugin) = self.plugin.clone() {
            trace!(
                "[app {}] [plugin {}] [std{std_type}] {output}",
                self.app_id,
                plugin.id(),
            );
        } else {
            error!(
                "received std{std_type} for app {} with missing plugin",
                self.app_id
            );
        }
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        // No buffering, so nothing to flush.
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        // No special cleanup needed.
        Poll::Ready(Ok(()))
    }
}
