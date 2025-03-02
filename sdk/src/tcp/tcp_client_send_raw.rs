use crate::binary::{BinaryTransport, ClientState};
use crate::error::IggyError;
use crate::tcp::tcp_client::TcpClient;
use crate::tcp::tcp_client_fields::{REQUEST_INITIAL_BYTES_LENGTH, RESPONSE_INITIAL_BYTES_LENGTH};
use bytes::Bytes;
use tracing::{error, trace};

impl TcpClient {
    pub(crate) async fn send_raw(&self, code: u32, payload: Bytes) -> Result<Bytes, IggyError> {
        match self.get_state().await {
            ClientState::Shutdown => {
                trace!("Cannot send data. Client is shutdown.");
                return Err(IggyError::ClientShutdown);
            }
            ClientState::Disconnected => {
                trace!("Cannot send data. Client is not connected.");
                return Err(IggyError::NotConnected);
            }
            ClientState::Connecting => {
                trace!("Cannot send data. Client is still connecting.");
                return Err(IggyError::NotConnected);
            }
            _ => {}
        }

        let mut stream = self.stream.lock().await;
        if let Some(stream) = stream.as_mut() {
            let payload_length = payload.len() + REQUEST_INITIAL_BYTES_LENGTH;
            trace!("Sending a TCP request with code: {code}");
            stream.write(&(payload_length as u32).to_le_bytes()).await?;
            stream.write(&code.to_le_bytes()).await?;
            stream.write(&payload).await?;
            stream.flush().await?;
            trace!("Sent a TCP request with code: {code}, waiting for a response...");

            let mut response_buffer = [0u8; RESPONSE_INITIAL_BYTES_LENGTH];
            let read_bytes = stream.read(&mut response_buffer).await.map_err(|error| {
                error!(
                    "Failed to read response for TCP request with code: {code}: {error}",
                    code = code,
                    error = error
                );
                IggyError::Disconnected
            })?;

            if read_bytes != RESPONSE_INITIAL_BYTES_LENGTH {
                error!("Received an invalid or empty response.");
                return Err(IggyError::EmptyResponse);
            }

            let status = u32::from_le_bytes(
                response_buffer[..4]
                    .try_into()
                    .map_err(|_| IggyError::InvalidNumberEncoding)?,
            );
            let length = u32::from_le_bytes(
                response_buffer[4..]
                    .try_into()
                    .map_err(|_| IggyError::InvalidNumberEncoding)?,
            );
            return self.handle_response(status, length, stream).await;
        }

        error!("Cannot send data. Client is not connected.");
        Err(IggyError::NotConnected)
    }
}
