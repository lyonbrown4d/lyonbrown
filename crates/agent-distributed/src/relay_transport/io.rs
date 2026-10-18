use std::{
  io,
  pin::Pin,
  task::{Context, Poll},
};

use tokio::{
  io::{AsyncRead, AsyncWrite, DuplexStream, ReadBuf},
  net::TcpStream,
};
use tonic::transport::server::{Connected, TcpConnectInfo};

/// An authenticated Noise link carrying the unchanged inner mutual-TLS byte stream.
pub struct RelayIo {
  pub(super) inner: DuplexStream,
  pub(super) id: String,
  pub(super) generation: u64,
  pub(super) commands: tokio::sync::mpsc::Sender<super::Command>,
  pub(super) stop: tokio_util::sync::CancellationToken,
}

impl Drop for RelayIo {
  fn drop(&mut self) {
    self.stop.cancel();
    let _ = self.commands.try_send(super::Command::Abort {
      id: self.id.clone(),
      generation: self.generation,
    });
  }
}

impl Connected for RelayIo {
  type ConnectInfo = TcpConnectInfo;
  fn connect_info(&self) -> Self::ConnectInfo {
    TcpConnectInfo {
      local_addr: None,
      remote_addr: None,
    }
  }
}

impl AsyncRead for RelayIo {
  fn poll_read(
    mut self: Pin<&mut Self>,
    cx: &mut Context<'_>,
    buffer: &mut ReadBuf<'_>,
  ) -> Poll<io::Result<()>> {
    Pin::new(&mut self.inner).poll_read(cx, buffer)
  }
}
impl AsyncWrite for RelayIo {
  fn poll_write(
    mut self: Pin<&mut Self>,
    cx: &mut Context<'_>,
    buffer: &[u8],
  ) -> Poll<io::Result<usize>> {
    Pin::new(&mut self.inner).poll_write(cx, buffer)
  }
  fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
    Pin::new(&mut self.inner).poll_flush(cx)
  }
  fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
    Pin::new(&mut self.inner).poll_shutdown(cx)
  }
}

/// Both routes retain tonic's existing TlsConnectInfo<TcpConnectInfo> authorization.
pub enum PeerIo {
  Tcp(TcpStream),
  Relay(RelayIo),
}
impl From<TcpStream> for PeerIo {
  fn from(stream: TcpStream) -> Self {
    Self::Tcp(stream)
  }
}
impl From<RelayIo> for PeerIo {
  fn from(stream: RelayIo) -> Self {
    Self::Relay(stream)
  }
}
impl Connected for PeerIo {
  type ConnectInfo = TcpConnectInfo;
  fn connect_info(&self) -> Self::ConnectInfo {
    match self {
      Self::Tcp(stream) => stream.connect_info(),
      Self::Relay(stream) => stream.connect_info(),
    }
  }
}
impl AsyncRead for PeerIo {
  fn poll_read(
    mut self: Pin<&mut Self>,
    cx: &mut Context<'_>,
    buffer: &mut ReadBuf<'_>,
  ) -> Poll<io::Result<()>> {
    match &mut *self {
      Self::Tcp(stream) => Pin::new(stream).poll_read(cx, buffer),
      Self::Relay(stream) => Pin::new(stream).poll_read(cx, buffer),
    }
  }
}
impl AsyncWrite for PeerIo {
  fn poll_write(
    mut self: Pin<&mut Self>,
    cx: &mut Context<'_>,
    buffer: &[u8],
  ) -> Poll<io::Result<usize>> {
    match &mut *self {
      Self::Tcp(stream) => Pin::new(stream).poll_write(cx, buffer),
      Self::Relay(stream) => Pin::new(stream).poll_write(cx, buffer),
    }
  }
  fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
    match &mut *self {
      Self::Tcp(stream) => Pin::new(stream).poll_flush(cx),
      Self::Relay(stream) => Pin::new(stream).poll_flush(cx),
    }
  }
  fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
    match &mut *self {
      Self::Tcp(stream) => Pin::new(stream).poll_shutdown(cx),
      Self::Relay(stream) => Pin::new(stream).poll_shutdown(cx),
    }
  }
}
