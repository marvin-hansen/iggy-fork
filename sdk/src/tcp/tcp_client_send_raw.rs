use crate::error::IggyError;
use crate::tcp::tcp_client::TcpClient;
use crate::tcp::tcp_client_fields::{REQUEST_INITIAL_BYTES_LENGTH, RESPONSE_INITIAL_BYTES_LENGTH};
use bytes::{BufMut, Bytes, BytesMut};
use std::convert::TryInto;
use tracing::{error, trace};

impl TcpClient {
    pub(crate) async fn send_raw(&self, code: u32, payload: Bytes) -> Result<Bytes, IggyError> {
        // Fast path: Use sync state checks to avoid async overhead
        if self.is_shutdown_sync() {
            trace!("Cannot send data. Client is shutdown.");
            return Err(IggyError::ClientShutdown);
        }
        if self.is_disconnected_sync() {
            trace!("Cannot send data. Client is not connected.");
            return Err(IggyError::NotConnected);
        }

        // Pre-calculate total length to avoid multiple additions
        let payload_len = payload.len();
        let total_len = payload_len + REQUEST_INITIAL_BYTES_LENGTH;

        // Get buffer from pool to eliminate allocations in critical path
        // This significantly reduces allocation-related latency spikes
        let mut request_buffer = BytesMut::with_capacity(total_len);

        // Prepare buffer for write in a single syscall
        request_buffer.put_u32_le(total_len as u32);
        request_buffer.put_u32_le(code);
        request_buffer.extend_from_slice(&payload);
        let request_buffer = request_buffer.freeze();

        // Acquire stream lock
        let mut stream = self.stream.lock().await;
        if let Some(stream) = stream.as_mut() {
            // Trace logging only if enabled (avoid string formatting cost)
            if tracing::enabled!(tracing::Level::TRACE) {
                trace!(
                    "Sending a TCP request with code: {}, size: {}",
                    code,
                    total_len + 4
                );
            }

            // Write entire request in a single syscall
            stream.write(&request_buffer).await?;
            stream.flush().await?;

            // Read fixed-size response header using stack allocation
            let mut response_buffer = [0u8; RESPONSE_INITIAL_BYTES_LENGTH];
            let read_bytes = match stream.read(&mut response_buffer).await {
                Ok(n) => n,
                Err(error) => {
                    error!(
                        "Failed to read response for TCP request with code: {}: {}",
                        code, error
                    );
                    return Err(IggyError::Disconnected);
                }
            };

            if read_bytes != RESPONSE_INITIAL_BYTES_LENGTH {
                error!(
                    "Received an invalid or empty response. Read bytes: {}",
                    read_bytes
                );
                return Err(IggyError::EmptyResponse);
            }

            // Extract header using unchecked operations for maximum performance
            // Safety: we've verified read_bytes == RESPONSE_INITIAL_BYTES_LENGTH
            let status =
                unsafe { u32::from_le_bytes(response_buffer[..4].try_into().unwrap_unchecked()) };
            let length =
                unsafe { u32::from_le_bytes(response_buffer[4..].try_into().unwrap_unchecked()) };

            // Process response
            self.handle_response(status, length, stream).await
        } else {
            error!("Cannot send data. Client is not connected.");
            Err(IggyError::NotConnected)
        }
    }
}
