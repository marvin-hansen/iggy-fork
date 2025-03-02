/// Socket optimization profile for different workload types
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum SocketOptimizationProfile {
    /// Optimize for lowest latency (smaller buffers, aggressive TCP options)
    LowestLatency,
    /// Balance between latency and throughput
    Balanced,
    /// Optimize for highest throughput (larger buffers)
    HighestThroughput,
}

impl Default for SocketOptimizationProfile {
    fn default() -> Self {
        SocketOptimizationProfile::Balanced
    }
}

/// Enhanced TCP socket configuration with platform-specific optimizations
#[derive(Debug, Copy, Clone)]
pub struct TcpSocketConfig {
    /// Size of the socket receive buffer in bytes (SO_RCVBUF)
    /// Default: 4 MB (4194304 bytes) for high throughput
    pub receive_buffer_size: u32,

    /// Size of the socket send buffer in bytes (SO_SNDBUF)
    /// Default: 4 MB (4194304 bytes) for high throughput
    pub send_buffer_size: u32,

    /// Enable/disable TCP_NODELAY to disable Nagle's algorithm
    /// Default: true (enabled) for lower latency
    pub nodelay: bool,

    /// Enable/disable TCP_QUICKACK to disable delayed ACKs (Linux only)
    /// Default: true (enabled) for lower latency
    pub quick_ack: bool,

    /// Enable/disable TCP_FASTOPEN for faster connection establishment (Linux only)
    /// Default: true (enabled) for faster connections
    pub tcp_fastopen: bool,

    /// Enable/disable TCP keepalive (SO_KEEPALIVE)
    /// Default: true (enabled) for connection stability
    pub keepalive: bool,

    /// TCP keepalive time in seconds (TCP_KEEPIDLE on Linux, TCP_KEEPALIVE on macOS)
    /// Default: 60 seconds
    pub keepalive_time: u32,

    /// TCP keepalive interval in seconds (TCP_KEEPINTVL)
    /// Default: 10 seconds
    pub keepalive_interval: u32,

    /// TCP keepalive probe count (TCP_KEEPCNT)
    /// Default: 6 probes
    pub keepalive_probes: u32,

    /// Enable/disable SO_REUSEADDR for socket address reuse
    /// Default: true (enabled) for faster reconnection
    pub reuse_address: bool,

    /// Enable/disable SO_REUSEPORT for socket port reuse (Linux and macOS)
    /// Default: true (enabled) for better load balancing
    pub reuse_port: bool,

    /// Enable/disable TCP_CORK/TCP_NOPUSH to optimize packet coalescing
    /// Default: false (disabled) for lower latency by default
    pub cork_or_nopush: bool,

    /// Socket optimization profile
    /// Default: Balanced
    pub optimization_profile: SocketOptimizationProfile,

    /// Low latency mode receive buffer size (8KB)
    pub(crate) latency_mode_receive_buffer_size: u32,

    /// Low latency mode send buffer size (8KB)
    pub(crate) latency_mode_send_buffer_size: u32,

    /// High throughput mode receive buffer size (8MB)
    pub(crate) throughput_mode_receive_buffer_size: u32,

    /// High throughput mode send buffer size (8MB)
    pub(crate) throughput_mode_send_buffer_size: u32,
}

impl TcpSocketConfig {
    /// Create a new TcpSocketConfig with custom settings
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        receive_buffer_size: u32,
        send_buffer_size: u32,
        nodelay: bool,
        quick_ack: bool,
        tcp_fastopen: bool,
        keepalive: bool,
        keepalive_time: u32,
        keepalive_interval: u32,
        keepalive_probes: u32,
        reuse_address: bool,
        reuse_port: bool,
        cork_or_nopush: bool,
        optimization_profile: SocketOptimizationProfile,
    ) -> Self {
        Self {
            receive_buffer_size,
            send_buffer_size,
            nodelay,
            quick_ack,
            tcp_fastopen,
            keepalive,
            keepalive_time,
            keepalive_interval,
            keepalive_probes,
            reuse_address,
            reuse_port,
            cork_or_nopush,
            optimization_profile,
            // Default buffer sizes for different modes
            latency_mode_receive_buffer_size: 8 * 1024, // 8 KB
            latency_mode_send_buffer_size: 8 * 1024,    // 8 KB
            throughput_mode_receive_buffer_size: 8 * 1024 * 1024, // 8 MB
            throughput_mode_send_buffer_size: 8 * 1024 * 1024, // 8 MB
        }
    }

    /// Create a new configuration optimized for lowest latency
    pub fn for_lowest_latency() -> Self {
        let mut config = Self::default();
        config.optimization_profile = SocketOptimizationProfile::LowestLatency;
        config.nodelay = true;
        config.quick_ack = true;
        config.tcp_fastopen = true;
        config.cork_or_nopush = false;
        config
    }

    /// Create a new configuration optimized for highest throughput
    pub fn for_highest_throughput() -> Self {
        let mut config = Self::default();
        config.optimization_profile = SocketOptimizationProfile::HighestThroughput;
        config.nodelay = false; // Enable Nagle's algorithm for better packet coalescing
        config.cork_or_nopush = true; // Enable packet coalescing
        config
    }

    /// Get the appropriate receive buffer size based on the optimization profile
    pub fn get_receive_buffer_size(&self) -> u32 {
        match self.optimization_profile {
            SocketOptimizationProfile::LowestLatency => self.latency_mode_receive_buffer_size,
            SocketOptimizationProfile::Balanced => self.receive_buffer_size,
            SocketOptimizationProfile::HighestThroughput => {
                self.throughput_mode_receive_buffer_size
            }
        }
    }

    /// Get the appropriate send buffer size based on the optimization profile
    pub fn get_send_buffer_size(&self) -> u32 {
        match self.optimization_profile {
            SocketOptimizationProfile::LowestLatency => self.latency_mode_send_buffer_size,
            SocketOptimizationProfile::Balanced => self.send_buffer_size,
            SocketOptimizationProfile::HighestThroughput => self.throughput_mode_send_buffer_size,
        }
    }
    /// Get a reference to the current optimization profile
    pub fn optimization_profile(&self) -> &SocketOptimizationProfile {
        &self.optimization_profile
    }

    /// Update the socket configuration to use the specified optimization profile
    pub fn with_optimization_profile(mut self, profile: SocketOptimizationProfile) -> Self {
        self.optimization_profile = profile;
        self
    }

    /// Apply all socket options to the given TcpStream
    /// This will apply both common and platform-specific socket options
    pub fn apply_to_stream(
        &self,
        stream: &tokio::net::TcpStream,
    ) -> Result<(), crate::error::IggyError> {
        crate::tcp::socket_optimizer::optimize_tcp_stream(stream, self)
    }
}

impl Default for TcpSocketConfig {
    fn default() -> Self {
        Self {
            // 4 MB for both receive and send buffers in balanced mode
            receive_buffer_size: 4 * 1024 * 1024,
            send_buffer_size: 4 * 1024 * 1024,
            // Enable TCP_NODELAY by default for lower latency
            nodelay: true,
            // Enable TCP_QUICKACK by default for lower latency (Linux only)
            quick_ack: true,
            // Enable TCP_FASTOPEN by default for faster connections (Linux only)
            tcp_fastopen: true,
            // Enable keepalive by default to detect stale connections
            keepalive: true,
            keepalive_time: 60,
            keepalive_interval: 10,
            keepalive_probes: 6,
            // Enable address reuse for faster reconnection
            reuse_address: true,
            // Enable port reuse for better load balancing
            reuse_port: true,
            // Disable TCP_CORK/TCP_NOPUSH by default for lower latency
            cork_or_nopush: false,
            // Use balanced profile by default
            optimization_profile: SocketOptimizationProfile::HighestThroughput,
            // Buffer sizes for different optimization modes
            latency_mode_receive_buffer_size: 8 * 1024, // 8 KB
            latency_mode_send_buffer_size: 8 * 1024,    // 8 KB
            throughput_mode_receive_buffer_size: 8 * 1024 * 1024, // 8 MB
            throughput_mode_send_buffer_size: 8 * 1024 * 1024, // 8 MB
        }
    }
}
