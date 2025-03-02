use crate::error::IggyError;
use crate::tcp::buffer_pool;
use crate::tcp::tcp_client::TcpClient;
use crate::tcp::tcp_client_fields::{REQUEST_INITIAL_BYTES_LENGTH, RESPONSE_INITIAL_BYTES_LENGTH};
use bytes::{BufMut, Bytes};
use std::convert::TryInto;
use std::time::{Duration, Instant};
use tracing::{error, trace};

// Latency targets for different operations
const TARGET_FLUSH_LATENCY_NS: u128 = 1_000_000; // 1ms target for flush operation
const MAX_FLUSH_WAIT_NS: u128 = 5_000_000; // 5ms maximum wait time for flush
const SMALL_PAYLOAD_THRESHOLD: usize = 1024; // 1KB

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
        let mut request_buffer = buffer_pool::get_sized_buffer(total_len + 4); // +4 for length field

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

        // Start high-precision timer for write+flush operations
        let op_start = Instant::now();

        // Write entire request in a single syscall
        let write_result = stream.write(&request_buffer).await;
        if let Err(e) = write_result {
            error!("Failed to write TCP request: {}", e);
            return Err(e.into());
        }

        // Get elapsed time after write to determine flush strategy
        let elapsed_ns = op_start.elapsed().as_nanos();

        // Choose a flush strategy based on payload size and elapsed time
        if payload_len <= SMALL_PAYLOAD_THRESHOLD {
            // For small payloads (<= 1KB), always flush immediately for lowest latency
            if let Err(e) = stream.flush().await {
                error!("Failed to flush TCP request: {}", e);
                return Err(e.into());
            }

            if tracing::enabled!(tracing::Level::TRACE) {
                trace!("Small payload immediate flush in {}ns", elapsed_ns);
            }
        } else if elapsed_ns < TARGET_FLUSH_LATENCY_NS {
            // For larger payloads, if we're under our latency budget, use adaptive flush
            // Calculate a dynamic wait time - this allows the kernel TCP stack to
            // potentially coalesce packets before flushing
            let remaining_budget = TARGET_FLUSH_LATENCY_NS.saturating_sub(elapsed_ns);
            let adaptive_wait = remaining_budget.min(MAX_FLUSH_WAIT_NS);

            if adaptive_wait > 0 && adaptive_wait < MAX_FLUSH_WAIT_NS {
                // Brief adaptive delay to allow packet coalescing
                tokio::time::sleep(Duration::from_nanos(adaptive_wait as u64)).await;
            }

            // Then flush
            if let Err(e) = stream.flush().await {
                error!("Failed to flush TCP request: {}", e);
                return Err(e.into());
            }

            if tracing::enabled!(tracing::Level::TRACE) {
                let total_ns = op_start.elapsed().as_nanos();
                trace!(
                    "Adaptive flush with {}ns wait, total {}ns",
                    adaptive_wait,
                    total_ns
                );
            }
        } else {
            // We've already exceeded our latency budget, flush immediately
            if let Err(e) = stream.flush().await {
                error!("Failed to flush TCP request: {}", e);
                return Err(e.into());
            }

            if tracing::enabled!(tracing::Level::TRACE) {
                trace!(
                    "Exceeded latency budget ({}ns), immediate flush",
                    elapsed_ns
                );
            }
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

        result
    }
}
