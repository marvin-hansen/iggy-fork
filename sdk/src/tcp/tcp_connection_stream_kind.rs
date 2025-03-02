use crate::error::IggyError;
use crate::tcp::tcp_client_connection_stream::ConnectionStream;
use crate::tcp::tcp_connection_stream::TcpConnectionStream;
use crate::tcp::tcp_tls_connection_stream::TcpTlsConnectionStream;

#[derive(Debug)]
pub(crate) enum ConnectionStreamKind {
    Tcp(TcpConnectionStream),
    TcpTls(TcpTlsConnectionStream),
}

impl ConnectionStreamKind {
    pub async fn read(&mut self, buf: &mut [u8]) -> Result<usize, IggyError> {
        match self {
            Self::Tcp(c) => c.read(buf).await,
            Self::TcpTls(c) => c.read(buf).await,
        }
    }

    pub async fn write(&mut self, buf: &[u8]) -> Result<(), IggyError> {
        match self {
            Self::Tcp(c) => c.write(buf).await,
            Self::TcpTls(c) => c.write(buf).await,
        }
    }

    pub async fn flush(&mut self) -> Result<(), IggyError> {
        match self {
            Self::Tcp(c) => c.flush().await,
            Self::TcpTls(c) => c.flush().await,
        }
    }

    pub async fn shutdown(&mut self) -> Result<(), IggyError> {
        match self {
            Self::Tcp(c) => c.shutdown().await,
            Self::TcpTls(c) => c.shutdown().await,
        }
    }
}
