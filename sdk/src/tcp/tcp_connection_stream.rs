use crate::error::IggyError;
use crate::tcp::tcp_client_connection_stream::ConnectionStream;
use async_trait::async_trait;
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;
use tracing::error;

#[derive(Debug)]
pub(crate) struct TcpConnectionStream {
    client_address: SocketAddr,
    reader: BufReader<OwnedReadHalf>,
    writer: BufWriter<OwnedWriteHalf>,
}

impl TcpConnectionStream {
    pub fn new(client_address: SocketAddr, stream: TcpStream) -> Self {
        let (reader, writer) = stream.into_split();
        Self {
            client_address,
            reader: BufReader::new(reader),
            writer: BufWriter::new(writer),
        }
    }
}

#[async_trait]
impl ConnectionStream for TcpConnectionStream {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, IggyError> {
        self.reader.read_exact(buf).await.map_err(|error| {
            error!(
                "Failed to read data by client: {} from the TCP connection: {error}",
                self.client_address
            );
            IggyError::TcpError
        })
    }

    async fn write(&mut self, buf: &[u8]) -> Result<(), IggyError> {
        self.writer.write_all(buf).await.map_err(|error| {
            error!(
                "Failed to write data by client: {} to the TCP connection: {error}",
                self.client_address
            );
            IggyError::TcpError
        })
    }

    async fn flush(&mut self) -> Result<(), IggyError> {
        self.writer.flush().await.map_err(|error| {
            error!(
                "Failed to flush data by client: {} to the TCP connection: {error}",
                self.client_address
            );
            IggyError::TcpError
        })
    }

    async fn shutdown(&mut self) -> Result<(), IggyError> {
        self.writer.shutdown().await.map_err(|error| {
            error!(
                "Failed to shutdown the TCP connection by client: {} to the TCP connection: {error}",
                self.client_address
            );
            IggyError::TcpError
        })
    }
}
