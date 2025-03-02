use crate::error::{IggyError, IggyErrorDiscriminants};
use crate::tcp::tcp_client::TcpClient;
use crate::tcp::tcp_connection_stream_kind::ConnectionStreamKind;
use bytes::{Bytes, BytesMut};
use std::cmp::min;
use std::time::Instant;
use tracing::{debug, error, instrument, trace};

// Define common buffer sizes for reuse
const SMALL_RESPONSE_THRESHOLD: u32 = 4096;
const MEDIUM_RESPONSE_THRESHOLD: u32 = 65536; // 64KB
const LARGE_RESPONSE_THRESHOLD: u32 = 1048576; // 1MB

// Thread-local stack buffer pool sizes for different response sizes
#[cfg(not(test))]
thread_local! {
    // Small buffer (4KB) for frequently used small responses
    static SMALL_BUFFER: std::cell::RefCell<[u8; SMALL_RESPONSE_THRESHOLD as usize]> =
        std::cell::RefCell::new([0u8; SMALL_RESPONSE_THRESHOLD as usize]);
}

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

        // Optimize different response size ranges
        let len = length as usize;

        // For small responses up to 4KB, use pre-allocated stack buffer
        if length <= SMALL_RESPONSE_THRESHOLD {
            #[cfg(not(test))]
            {
                // Create a stack buffer first
                let mut stack_buffer = [0u8; SMALL_RESPONSE_THRESHOLD as usize];
                // Use a simple stack buffer for now - can't use thread_local with await
                // Revisit with Arc<RefCell<>> if we need to optimize further
                return Self::read_into_fixed_buffer(stream, &mut stack_buffer[0..len], len).await;
            }

            #[cfg(test)]
            {
                let mut stack_buffer = [0u8; SMALL_RESPONSE_THRESHOLD as usize];
                return Self::read_into_fixed_buffer(stream, &mut stack_buffer[0..len], len).await;
            }
        }

        // For medium responses (4KB-64KB), handle in chunks with single allocation
        if length <= MEDIUM_RESPONSE_THRESHOLD {
            return Self::read_chunked_response(stream, len, 16384).await; // 16KB chunks
        }

        // For large responses (64KB-1MB), handle in larger chunks
        if length <= LARGE_RESPONSE_THRESHOLD {
            return Self::read_chunked_response(stream, len, 65536).await; // 64KB chunks
        }

        // For very large responses (>1MB), use truly massive chunks
        Self::read_chunked_response(stream, len, 262144).await // 256KB chunks
    }

    // Helper method to read into a fixed-size buffer
    #[inline(always)]
    async fn read_into_fixed_buffer(
        stream: &mut ConnectionStreamKind,
        buffer: &mut [u8],
        expected_len: usize,
    ) -> Result<Bytes, IggyError> {
        let read_bytes = stream.read(buffer).await?;

        if read_bytes != expected_len {
            if tracing::enabled!(tracing::Level::DEBUG) {
                debug!(
                    "Incomplete read: expected {} bytes, got {}",
                    expected_len, read_bytes
                );
            }
        }

        // Return exactly what we read
        Ok(Bytes::copy_from_slice(&buffer[0..read_bytes]))
    }

    // Helper method to read a large response in chunks
    async fn read_chunked_response(
        stream: &mut ConnectionStreamKind,
        total_len: usize,
        chunk_size: usize,
    ) -> Result<Bytes, IggyError> {
        #[cfg(debug_assertions)]
        let read_start = Instant::now();

        // Preallocate the entire buffer at once to avoid reallocation
        let mut response_buffer = BytesMut::with_capacity(total_len);
        unsafe {
            response_buffer.set_len(total_len);
        }

        let mut bytes_read = 0;
        let mut remaining = total_len;

        // Read in chunks to handle large responses more efficiently
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
                    unsafe {
                        response_buffer.set_len(bytes_read);
                    }

                    // Still return what we have if we've read anything
                    if bytes_read > 0 {
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

        #[cfg(debug_assertions)]
        {
            let read_time = read_start.elapsed();
            if read_time.as_millis() > 100 && bytes_read > SMALL_RESPONSE_THRESHOLD as usize {
                debug!("Large read: {} bytes took {:?}", bytes_read, read_time);
            }
        }

        Ok(response_buffer.freeze())
    }
}
