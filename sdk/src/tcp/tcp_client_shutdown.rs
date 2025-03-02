use crate::binary::{BinaryTransport, ClientState};
use crate::diagnostic::DiagnosticEvent;
use crate::error::IggyError;
use crate::tcp::tcp_client::TcpClient;
use crate::tcp::tcp_client_fields::NAME;
use std::sync::atomic::Ordering;
use tracing::info;

impl TcpClient {
    pub(crate) async fn shutdown(&self) -> Result<(), IggyError> {
        // Use fast non-blocking state check
        if self.is_shutdown() {
            return Ok(());
        }

        // Use non-blocking method for address access
        let client_address = self.get_client_address_value_sync();
        info!("Shutting down TCP client: {client_address}");

        let stream = self.stream.write().await.take();
        if let Some(mut stream) = stream {
            stream.shutdown().await?;
        }

        self.state
            .store(ClientState::Shutdown as u8, Ordering::Release);
        info!("TCP client: {client_address} has been shutdown.");
        self.publish_event(DiagnosticEvent::Shutdown).await;

        info!("{NAME} TCP client: {client_address} has been shutdown.");
        Ok(())
    }
}
