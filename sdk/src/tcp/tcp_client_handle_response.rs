use crate::error::{IggyError, IggyErrorDiscriminants};
use crate::tcp::tcp_client::TcpClient;
use crate::tcp::tcp_connection_stream_kind::ConnectionStreamKind;
use bytes::{Bytes, BytesMut};
use std::cmp::min;
use tracing::{debug, error, instrument, trace};

// Define common buffer sizes for reuse
const SMALL_RESPONSE_THRESHOLD: u32 = 4096;
const MEDIUM_RESPONSE_THRESHOLD: u32 = 65536; // 64KB
const LARGE_RESPONSE_THRESHOLD: u32 = 1048576; // 1MB

impl TcpClient {
    /// Handle TCP response - optimized for both small and large payloads
    #[instrument(skip_all, level = "trace")]
    pub(crate) async fn handle_response(
        &self,
        status: u32,
        length: u32,
        stream: &mut ConnectionStreamKind,
    ) -> Result<Bytes, IggyError> {
        // Fast path for error responses
        if status != 0 {
            // Pre-check known "already exists" errors which are common and expected
            let is_already_exists = match status {
                x if x == IggyErrorDiscriminants::TopicIdAlreadyExists as u32
                    || x == IggyErrorDiscriminants::TopicNameAlreadyExists as u32
                    || x == IggyErrorDiscriminants::StreamIdAlreadyExists as u32
                    || x == IggyErrorDiscriminants::StreamNameAlreadyExists as u32
                    || x == IggyErrorDiscriminants::UserAlreadyExists as u32
                    || x == IggyErrorDiscriminants::PersonalAccessTokenAlreadyExists as u32
                    || x == IggyErrorDiscriminants::ConsumerGroupIdAlreadyExists as u32
                    || x == IggyErrorDiscriminants::ConsumerGroupNameAlreadyExists as u32 =>
                {
                    true
                }
                _ => false,
            };

            // Only format error strings when enabled for that level
            if is_already_exists {
                if tracing::enabled!(tracing::Level::DEBUG) {
                    debug!(
                        "Resource already exists: {} ({})",
                        status,
                        IggyError::from_code_as_string(status)
                    );
                }
            } else if tracing::enabled!(tracing::Level::ERROR) {
                error!(
                    "Invalid response status: {} ({})",
                    status,
                    IggyError::from_code_as_string(status),
                );
            }

            return Err(IggyError::from_code(status));
        }

        if tracing::enabled!(tracing::Level::TRACE) {
            trace!("Status: OK. Response length: {}", length);
        }

        // Fast path for empty responses
        if length <= 1 {
            return Ok(Bytes::new());
        }

        // Use direct allocation for all response sizes instead of memory pools
        // to prevent p999 and p9999 latency spikes
        let len = length as usize;

        // For small responses that can be read in one go, use a simple read
        if length <= SMALL_RESPONSE_THRESHOLD {
            // Create a new buffer with exact capacity
            let mut buffer = BytesMut::with_capacity(len);
            // Set the length so we can write directly into it
            unsafe {
                buffer.set_len(len);
            }

            // Read directly into the buffer
            let read_bytes = stream.read(&mut buffer).await?;

            if read_bytes != len {
                if tracing::enabled!(tracing::Level::DEBUG) {
                    debug!(
                        "Incomplete read: expected {} bytes, got {}",
                        len, read_bytes
                    );
                }
                // Adjust buffer to actual read size
                unsafe {
                    buffer.set_len(read_bytes);
                }
            }

            // Convert to Bytes and return
            return Ok(buffer.freeze());
        }

        // For medium and large responses, use the chunked reading strategy
        // Choose an appropriate chunk size based on the response size
        let chunk_size = if length <= MEDIUM_RESPONSE_THRESHOLD {
            16384 // 16KB chunks for medium responses
        } else if length <= LARGE_RESPONSE_THRESHOLD {
            65536 // 64KB chunks for large responses
        } else {
            262144 // 256KB chunks for very large responses
        };

        // Use the chunked response reader directly, without buffer pooling
        Self::read_chunked_response(stream, len, chunk_size).await
    }

    // Optimized helper method to read a response in chunks
    async fn read_chunked_response(
        stream: &mut ConnectionStreamKind,
        total_len: usize,
        chunk_size: usize,
    ) -> Result<Bytes, IggyError> {
        // Preallocate the entire buffer at once to avoid reallocation
        let mut response_buffer = BytesMut::with_capacity(total_len);
        unsafe {
            response_buffer.set_len(total_len);
        }

        let mut bytes_read = 0;
        let mut remaining = total_len;

        // Read in chunks to efficiently handle responses of all sizes
        while remaining > 0 {
            let to_read = min(remaining, chunk_size);
            let segment = &mut response_buffer[bytes_read..bytes_read + to_read];

            match stream.read(segment).await {
                Ok(n) if n > 0 => {
                    bytes_read += n;
                    remaining -= n;

                    // Break if we read less than requested - no more data available
                    if n < to_read {
                        break;
                    }
                }
                Ok(_) => break, // EOF, no more data
                Err(e) => {
                    // Adjust buffer to contain only read data
                    if bytes_read > 0 {
                        unsafe {
                            response_buffer.set_len(bytes_read);
                        }
                        if tracing::enabled!(tracing::Level::DEBUG) {
                            debug!("Read error after {} bytes: {}", bytes_read, e);
                        }
                        return Ok(response_buffer.freeze());
                    }
                    return Err(e.into());
                }
            }
        }

        // Adjust buffer if we didn't read everything
        if bytes_read < total_len {
            unsafe {
                response_buffer.set_len(bytes_read);
            }
            if tracing::enabled!(tracing::Level::DEBUG) {
                debug!(
                    "Incomplete read: expected {} bytes, got {}",
                    total_len, bytes_read
                );
            }
        }

        Ok(response_buffer.freeze())
    }
}
