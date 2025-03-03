use crate::binary::{BinaryTransport, ClientState};
use crate::diagnostic::DiagnosticEvent;
use crate::error::IggyError;
use crate::tcp::tcp_client::TcpClient;
use crate::tcp::tcp_client_fields::NAME;
use crate::utils::timestamp::IggyTimestamp;
use std::sync::atomic::Ordering;
use tracing::info;

impl TcpClient {
    pub(crate) async fn disconnect(&self) -> Result<(), IggyError> {
        // Fast path: Use non-blocking is_disconnected check instead of awaiting get_state
        if self.is_disconnected() {
            return Ok(());
        }

        // Use sync version to avoid await
        let client_address = self.get_client_address_value_sync();
        info!("Client: {client_address} is disconnecting from server...");

        // Use atomic store directly instead of awaiting set_state
        self.state
            .store(ClientState::Disconnected as u8, Ordering::Release);

        // Takes the value out of the option
        self.stream.lock().await.take();

        self.publish_event(DiagnosticEvent::Disconnected).await;

        let now = IggyTimestamp::now();
        info!("{NAME} client: {client_address} has disconnected from server at: {now}.");

        // Record disconnect time for reconnection logic
        self.last_reconnect_attempt
            .store(Some(IggyTimestamp::now()));

        Ok(())
    }
}
