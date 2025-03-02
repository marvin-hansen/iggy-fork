use crate::command::Command;
use crate::diagnostic::DiagnosticEvent;
use crate::error::IggyError;
use crate::utils::duration::IggyDuration;
use async_trait::async_trait;
use bytes::Bytes;

#[allow(deprecated)]
pub mod binary_client;
mod binary_client_state;
#[allow(deprecated)]
pub mod consumer_groups;
#[allow(deprecated)]
pub mod consumer_offsets;
mod mapper;
#[allow(deprecated)]
pub mod messages;
#[allow(deprecated)]
pub mod partitions;
#[allow(deprecated)]
pub mod personal_access_tokens;
#[allow(deprecated)]
pub mod streams;
#[allow(deprecated)]
pub mod system;
#[allow(deprecated)]
pub mod topics;
#[allow(deprecated)]
pub mod users;

pub use binary_client_state::ClientState;

#[async_trait]
pub trait BinaryTransport {
    /// Gets the state of the client.
    async fn get_state(&self) -> ClientState;
    /// Sets the state of the client.
    async fn set_state(&self, state: ClientState);
    async fn publish_event(&self, event: DiagnosticEvent);
    /// Sends a command and returns the response.
    async fn send_with_response<T: Command>(&self, command: &T) -> Result<Bytes, IggyError>;
    async fn send_raw_with_response(&self, code: u32, payload: Bytes) -> Result<Bytes, IggyError>;
    fn get_heartbeat_interval(&self) -> IggyDuration;
}

async fn fail_if_not_authenticated<T: BinaryTransport>(transport: &T) -> Result<(), IggyError> {
    match transport.get_state().await {
        ClientState::Shutdown => Err(IggyError::ClientShutdown),
        ClientState::Disconnected | ClientState::Connecting | ClientState::Authenticating => {
            Err(IggyError::Disconnected)
        }
        ClientState::Connected => Err(IggyError::Unauthenticated),
        ClientState::Authenticated => Ok(()),
    }
}
