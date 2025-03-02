use crate::error::IggyError;
use crate::tcp::tcp_client::TcpClient;
use crate::tcp::tcp_client_fields::{REQUEST_INITIAL_BYTES_LENGTH, RESPONSE_INITIAL_BYTES_LENGTH};
use bytes::{BufMut, Bytes, BytesMut};
use std::convert::TryInto;
use std::time::Instant;
use tracing::{debug, error, trace};

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

        // Skip timing unless we're at trace log level
        #[cfg(debug_assertions)]
        let start_time = Instant::now();

        // Pre-calculate total length to avoid multiple additions
        let payload_len = payload.len();
        let total_len = payload_len + REQUEST_INITIAL_BYTES_LENGTH;

        // Fast path for small payloads: use preallocated buffer
        // Use preallocated BytesMut with exact capacity to avoid allocations
        let mut request_buffer = BytesMut::with_capacity(total_len + 4); // +4 for length field

        // Optimize buffer writing with minimal method calls
        request_buffer.put_u32_le(total_len as u32);
        request_buffer.put_u32_le(code);
        request_buffer.extend_from_slice(&payload);
        let request_buffer = request_buffer.freeze();

        // Acquire stream lock with minimal scope
        let mut stream_guard = self.stream.write().await;
        let stream = match stream_guard.as_mut() {
            Some(s) => s,
            None => {
                error!("Cannot send data. Client is not connected.");
                return Err(IggyError::NotConnected);
            }
        };

        // Trace logging only if enabled (avoid string formatting cost)
        if tracing::enabled!(tracing::Level::TRACE) {
            trace!(
                "Sending a TCP request with code: {}, size: {}",
                code,
                total_len + 4
            );
        }

        // Write entire request in a single syscall
        let write_result = stream.write(&request_buffer).await;
        if let Err(e) = write_result {
            error!("Failed to write TCP request: {}", e);
            return Err(e.into());
        }

        // Explicitly flush to ensure data is sent immediately
        // This is critical for reliable communication
        if let Err(e) = stream.flush().await {
            error!("Failed to flush TCP request: {}", e);
            return Err(e.into());
        }

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
        let result = self.handle_response(status, length, stream).await;

        // Performance monitoring if debug enabled
        #[cfg(debug_assertions)]
        {
            let elapsed = start_time.elapsed();
            if elapsed.as_millis() > 100 {
                debug!("TCP request code: {} completed in {:?}", code, elapsed);
            } else if tracing::enabled!(tracing::Level::TRACE) {
                trace!("TCP request code: {} completed in {:?}", code, elapsed);
            }
        }

        result
    }
}
