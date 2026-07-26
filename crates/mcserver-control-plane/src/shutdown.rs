use std::io;

use tokio::signal::unix::{Signal, SignalKind, signal};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownSignal {
    Interrupt,
    Terminate,
}

impl ShutdownSignal {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Interrupt => "SIGINT",
            Self::Terminate => "SIGTERM",
        }
    }
}

pub struct ShutdownSignals {
    interrupt: Signal,
    terminate: Signal,
}

impl ShutdownSignals {
    pub fn new() -> Result<Self, io::Error> {
        Ok(Self {
            interrupt: signal(SignalKind::interrupt())?,
            terminate: signal(SignalKind::terminate())?,
        })
    }

    pub async fn recv(&mut self) -> ShutdownSignal {
        tokio::select! {
            _ = self.interrupt.recv() => ShutdownSignal::Interrupt,
            _ = self.terminate.recv() => ShutdownSignal::Terminate,
        }
    }
}
