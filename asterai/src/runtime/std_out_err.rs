use log::trace;
use std::io::Error;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::AsyncWrite;
use uuid::Uuid;
use wasmtime_wasi::cli::{IsTerminal, StdoutStream};

pub struct ComponentStdout {
    // TODO rename to env?
    pub app_id: Uuid,
}

pub struct ComponentStderr {
    pub app_id: Uuid,
}

struct PluginStdOutErrWriter {
    is_stderr: bool,
    app_id: Uuid,
}

impl IsTerminal for ComponentStdout {
    fn is_terminal(&self) -> bool {
        false
    }
}

impl IsTerminal for ComponentStderr {
    fn is_terminal(&self) -> bool {
        false
    }
}

impl StdoutStream for ComponentStdout {
    fn async_stream(&self) -> Box<dyn AsyncWrite + Send + Sync> {
        Box::new(PluginStdOutErrWriter {
            is_stderr: false,
            app_id: self.app_id,
        })
    }
}

impl StdoutStream for ComponentStderr {
    fn async_stream(&self) -> Box<dyn AsyncWrite + Send + Sync> {
        Box::new(PluginStdOutErrWriter {
            is_stderr: true,
            app_id: self.app_id,
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
        trace!("[app {}] [std{std_type}] {output}", self.app_id,);
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
