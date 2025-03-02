use crate::error::IggyError;
use crate::tcp::tcp_client_connection_stream::ConnectionStream;
use async_trait::async_trait;
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::TlsStream;
use tracing::error;

#[derive(Debug)]
pub(crate) struct TcpTlsConnectionStream {
    client_address: SocketAddr,
    stream: TlsStream<TcpStream>,
}

impl TcpTlsConnectionStream {
    pub fn new(client_address: SocketAddr, stream: TlsStream<TcpStream>) -> Self {
        Self {
            client_address,
            stream,
        }
    }
}

#[async_trait]
impl ConnectionStream for TcpTlsConnectionStream {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, IggyError> {
        self.stream.read(buf).await.map_err(|error| {
            error!(
                "Failed to read data by client: {} from the TCP TLS connection: {error}",
                self.client_address
            );
            IggyError::TcpError
        })
    }

    async fn write(&mut self, buf: &[u8]) -> Result<(), IggyError> {
        self.stream.write_all(buf).await.map_err(|error| {
            error!(
                "Failed to write data by client: {} to the TCP TLS connection: {error}",
                self.client_address
            );
            IggyError::TcpError
        })
    }

    async fn flush(&mut self) -> Result<(), IggyError> {
        self.stream.flush().await.map_err(|error| {
            error!(
                "Failed to flush data by client: {} to the TCP TLS connection: {error}",
                self.client_address
            );
            IggyError::TcpError
        })
    }

    async fn shutdown(&mut self) -> Result<(), IggyError> {
        self.stream.shutdown().await.map_err(|error| {
            error!(
                "Failed to shutdown the TCP TLS connection by client: {} to the TCP TLS connection: {error}",
                self.client_address
            );
            IggyError::TcpError
        })
    }
}
