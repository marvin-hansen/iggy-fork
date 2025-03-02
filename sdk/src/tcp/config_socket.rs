#[derive(Debug, Copy, Clone)]
pub struct TcpSocketConfig {
    /// Size of the socket receive buffer in bytes (SO_RCVBUF)
    /// Default: 4 MB (4194304 bytes)
    pub receive_buffer_size: u32, // 4 bytes
    /// Size of the socket send buffer in bytes (SO_SNDBUF)
    /// Default: 4 MB (4194304 bytes)
    pub send_buffer_size: u32, // 4 bytes
    /// Enable/disable TCP_QUICKACK to disable delayed ACKs
    /// Default: true (enabled)
    pub quick_ack: bool, // 1 byte
    /// Enable/disable TCP_NODELAY to disable Nagle's algorithm
    /// Default: true (enabled)
    pub nodelay: bool, // 1 byte
    /// Enable/disable TCP keepalive (SO_KEEPALIVE)
    /// Default: true (enabled)
    pub keepalive: bool, // 1 byte
    /// TCP keepalive time in seconds (TCP_KEEPIDLE)
    /// Default: 60 seconds
    pub keepalive_time: u32, // 4 bytes
    /// TCP keepalive interval in seconds (TCP_KEEPINTVL)
    /// Default: 10 seconds
    pub keepalive_interval: u32, // 4 bytes
    /// TCP keepalive probe count (TCP_KEEPCNT)
    /// Default: 6 probes
    pub keepalive_probes: u32, // 4 bytes
    /// Enable/disable latency optimization mode
    /// When enabled, smaller buffer sizes will be used to reduce latency
    /// Default: false (disabled)
    pub latency_mode: bool, // 1 byte
    /// Size of the socket receive buffer in bytes when in latency mode
    /// Default: 8 KB (8192 bytes)
    latency_mode_receive_buffer_size: u32, // 4 bytes
    /// Size of the socket send buffer in bytes when in latency mode
    /// Default: 8 KB (8192 bytes)
    latency_mode_send_buffer_size: u32, // 4 bytes
}

impl TcpSocketConfig {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        receive_buffer_size: u32,
        send_buffer_size: u32,
        quick_ack: bool,
        nodelay: bool,
        keepalive: bool,
        keepalive_time: u32,
        keepalive_interval: u32,
        keepalive_probes: u32,
        latency_mode: bool,
        latency_mode_receive_buffer_size: u32,
        latency_mode_send_buffer_size: u32,
    ) -> Self {
        Self {
            receive_buffer_size,
            send_buffer_size,
            quick_ack,
            nodelay,
            keepalive,
            keepalive_time,
            keepalive_interval,
            keepalive_probes,
            latency_mode,
            latency_mode_receive_buffer_size,
            latency_mode_send_buffer_size,
        }
    }

    pub fn latency_mode_receive_buffer_size(&self) -> u32 {
        if self.latency_mode {
            // Use latency mode buffer size when in latency mode
            self.latency_mode_receive_buffer_size
        } else {
            // Fall back to normal buffer size when not in latency mode
            self.receive_buffer_size
        }
    }

    pub fn latency_mode_send_buffer_size(&self) -> u32 {
        if self.latency_mode {
            // Use latency mode buffer size when in latency mode
            self.latency_mode_send_buffer_size
        } else {
            // Fall back to normal buffer size when not in latency mode
            self.send_buffer_size
        }
    }
}

impl Default for TcpSocketConfig {
    fn default() -> Self {
        Self {
            // 4 MB for both receive and send buffers in normal mode
            receive_buffer_size: 4 * 1024 * 1024,
            send_buffer_size: 4 * 1024 * 1024,
            quick_ack: true,
            nodelay: true,
            keepalive: true,
            keepalive_time: 60,
            keepalive_interval: 10,
            keepalive_probes: 6,
            // Disable low latency mode by default
            latency_mode: false,
            // 8 KB buffers for latency optimization
            latency_mode_receive_buffer_size: 8 * 1024,
            latency_mode_send_buffer_size: 8 * 1024,
        }
    }
}
