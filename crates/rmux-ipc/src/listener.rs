//! Local listener handles.

use std::io;

use crate::{LocalEndpoint, LocalStream, PeerIdentity};

/// Local IPC listener.
#[cfg(unix)]
#[derive(Debug)]
pub struct LocalListener {
    inner: tokio::net::UnixListener,
}

/// Local IPC listener default_value for Windows until named pipes are added.
#[cfg(windows)]
#[derive(Debug)]
pub struct LocalListener;

impl LocalListener {
    /// Binds a local listener.
    pub fn bind(endpoint: &LocalEndpoint) -> io::Result<Self> {
        bind_impl(endpoint)
    }

    /// Accepts one local client and returns its byte stream plus peer identity.
    pub async fn accept(&self) -> io::Result<(LocalStream, PeerIdentity)> {
        accept_impl(self).await
    }
}

#[cfg(unix)]
fn bind_impl(endpoint: &LocalEndpoint) -> io::Result<LocalListener> {
    Ok(LocalListener {
        inner: tokio::net::UnixListener::bind(endpoint.as_path())?,
    })
}

#[cfg(windows)]
fn bind_impl(_endpoint: &LocalEndpoint) -> io::Result<LocalListener> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Windows named-pipe listener is not implemented until Milestone 6",
    ))
}

#[cfg(unix)]
async fn accept_impl(listener: &LocalListener) -> io::Result<(LocalStream, PeerIdentity)> {
    let (stream, _addr) = listener.inner.accept().await?;
    let peer = PeerIdentity::from_unix_stream(&stream)?;
    Ok((stream, peer))
}

#[cfg(windows)]
async fn accept_impl(_listener: &LocalListener) -> io::Result<(LocalStream, PeerIdentity)> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Windows named-pipe listener is not implemented until Milestone 6",
    ))
}
