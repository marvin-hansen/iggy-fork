use crate::error::IggyError;
use crate::tcp::config_socket::{SocketOptimizationProfile, TcpSocketConfig};
use std::io;
use tokio::net::TcpStream;
use tracing::{debug, error, trace};

/// Socket optimizer trait for applying platform-specific socket optimizations
pub trait SocketOptimizer {
    /// Apply socket options to a TcpStream based on the provided configuration
    fn apply_socket_options(stream: &TcpStream, config: &TcpSocketConfig) -> Result<(), IggyError>;
}

/// Default implementation of SocketOptimizer that works on all platforms
pub struct DefaultSocketOptimizer;

impl SocketOptimizer for DefaultSocketOptimizer {
    fn apply_socket_options(stream: &TcpStream, config: &TcpSocketConfig) -> Result<(), IggyError> {
        // Apply common socket options available on all platforms
        apply_common_socket_options(stream, config)?;

        // Apply platform-specific socket options
        #[cfg(target_os = "linux")]
        apply_linux_socket_options(stream, config)?;

        #[cfg(target_os = "macos")]
        apply_macos_socket_options(stream, config)?;

        Ok(())
    }
}

/// Apply socket options common to all platforms
fn apply_common_socket_options(
    stream: &TcpStream,
    config: &TcpSocketConfig,
) -> Result<(), IggyError> {
    // Set TCP_NODELAY (disable Nagle's algorithm)
    if let Err(e) = stream.set_nodelay(config.nodelay) {
        error!("Failed to set TCP_NODELAY to {}: {}", config.nodelay, e);
        return Err(IggyError::TcpError);
    }

    // Get buffer sizes for logging
    let recv_buf_size = config.get_receive_buffer_size();
    let send_buf_size = config.get_send_buffer_size();

    // For other socket options, we need to use the std socket APIs
    #[cfg(unix)]
    {
        use std::os::unix::io::{AsRawFd, FromRawFd};
        let fd = stream.as_raw_fd();
        let socket = unsafe { socket2::Socket::from_raw_fd(fd) };

        // Set SO_KEEPALIVE
        if let Err(e) = socket.set_keepalive(config.keepalive) {
            error!("Failed to set SO_KEEPALIVE to {}: {}", config.keepalive, e);
            return Err(IggyError::TcpError);
        }

        // Set SO_REUSEADDR
        if let Err(e) = socket.set_reuse_address(config.reuse_address) {
            error!(
                "Failed to set SO_REUSEADDR to {}: {}",
                config.reuse_address, e
            );
            return Err(IggyError::TcpError);
        }

        // Set receive buffer size based on optimization profile
        if let Err(e) = socket.set_recv_buffer_size(recv_buf_size as usize) {
            error!("Failed to set SO_RCVBUF to {}: {}", recv_buf_size, e);
            return Err(IggyError::TcpError);
        }

        // Set send buffer size based on optimization profile
        if let Err(e) = socket.set_send_buffer_size(send_buf_size as usize) {
            error!("Failed to set SO_SNDBUF to {}: {}", send_buf_size, e);
            return Err(IggyError::TcpError);
        }

        // Set SO_REUSEPORT if supported and enabled
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        if config.reuse_port {
            if let Err(e) = set_reuse_port(&socket) {
                error!("Failed to set SO_REUSEPORT: {}", e);
                // Don't fail if this option is not supported
            }
        }

        // We don't want to close the fd when socket is dropped
        std::mem::forget(socket);
    }

    debug!("Applied common socket options: nodelay={}, keepalive={}, reuse_address={}, recv_buffer={}, send_buffer={}",
        config.nodelay, config.keepalive, config.reuse_address, recv_buf_size, send_buf_size);

    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn set_reuse_port(socket: &socket2::Socket) -> io::Result<()> {
    use std::os::unix::io::AsRawFd;

    let fd = socket.as_raw_fd();

    unsafe {
        let val: libc::c_int = 1;
        if libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_REUSEPORT,
            &val as *const _ as *const libc::c_void,
            std::mem::size_of_val(&val) as libc::socklen_t,
        ) < 0
        {
            return Err(io::Error::last_os_error());
        }
    }

    Ok(())
}

/// Apply Linux-specific socket options with advanced performance optimizations
#[cfg(target_os = "linux")]
fn apply_linux_socket_options(
    stream: &TcpStream,
    config: &TcpSocketConfig,
) -> Result<(), IggyError> {
    use std::os::unix::io::AsRawFd;

    let fd = stream.as_raw_fd();

    // Define common TCP options constants
    // TCP_DEFER_ACCEPT - Complete connections only when data arrives
    const TCP_DEFER_ACCEPT: libc::c_int = 9;
    // TCP_THIN_LINEAR_TIMEOUTS - Use linear timeouts for thin streams
    const TCP_THIN_LINEAR_TIMEOUTS: libc::c_int = 16;
    // TCP_NOTSENT_LOWAT - Set a low watermark for best effort transmission
    const TCP_NOTSENT_LOWAT: libc::c_int = 25;

    // Set TCP_QUICKACK if enabled - reduces latency by immediately acknowledging packets
    if config.quick_ack {
        unsafe {
            let val: libc::c_int = 1;
            if libc::setsockopt(
                fd,
                libc::IPPROTO_TCP,
                libc::TCP_QUICKACK,
                &val as *const _ as *const libc::c_void,
                std::mem::size_of_val(&val) as libc::socklen_t,
            ) < 0
            {
                let err = io::Error::last_os_error();
                error!("Failed to set TCP_QUICKACK: {}", err);
                return Err(IggyError::TcpError);
            }
        }
        trace!("Applied Linux TCP_QUICKACK=1");
    }

    // Apply thin stream optimizations for low-latency profile
    if config.optimization_profile == SocketOptimizationProfile::LowestLatency {
        unsafe {
            // Enable linear timeouts for thin streams (better for interactive traffic)
            let val: libc::c_int = 1;
            if libc::setsockopt(
                fd,
                libc::IPPROTO_TCP,
                TCP_THIN_LINEAR_TIMEOUTS,
                &val as *const _ as *const libc::c_void,
                std::mem::size_of_val(&val) as libc::socklen_t,
            ) < 0
            {
                // Don't fail if not supported
            } else {
                trace!("Applied Linux TCP_THIN_LINEAR_TIMEOUTS=1");
            }

            // Set low watermark for unsent data to reduce latency
            let val: libc::c_int = 4096; // 4KB
            if libc::setsockopt(
                fd,
                libc::IPPROTO_TCP,
                TCP_NOTSENT_LOWAT,
                &val as *const _ as *const libc::c_void,
                std::mem::size_of_val(&val) as libc::socklen_t,
            ) < 0
            {
                // Don't fail if not supported
            } else {
                trace!("Applied Linux TCP_NOTSENT_LOWAT={}", val);
            }
        }
    }

    // Set TCP_FASTOPEN if enabled - allows data exchange during the handshake
    if config.tcp_fastopen {
        unsafe {
            let val: libc::c_int = 1;
            if libc::setsockopt(
                fd,
                libc::IPPROTO_TCP,
                libc::TCP_FASTOPEN,
                &val as *const _ as *const libc::c_void,
                std::mem::size_of_val(&val) as libc::socklen_t,
            ) < 0
            {
                let err = io::Error::last_os_error();
                error!("Failed to set TCP_FASTOPEN: {}", err);
                // Don't fail if this option is not supported
            } else {
                trace!("Applied Linux TCP_FASTOPEN=1");
            }
        }
    }

    // Apply TCP_CORK for throughput profile - buffers small packets
    // Must be mutually exclusive with TCP_NODELAY
    if config.cork_or_nopush && !config.nodelay {
        unsafe {
            let val: libc::c_int = 1;
            if libc::setsockopt(
                fd,
                libc::IPPROTO_TCP,
                libc::TCP_CORK,
                &val as *const _ as *const libc::c_void,
                std::mem::size_of_val(&val) as libc::socklen_t,
            ) < 0
            {
                let err = io::Error::last_os_error();
                error!("Failed to set TCP_CORK: {}", err);
                // Don't fail if this option is not supported
            } else {
                trace!("Applied Linux TCP_CORK=1");
            }
        }
    }

    // Apply TCP_DEFER_ACCEPT for server sockets
    // This completes connections only when data arrives, reducing resource usage
    unsafe {
        let val: libc::c_int = 5; // Accept after 5 seconds or when data arrives
        if libc::setsockopt(
            fd,
            libc::IPPROTO_TCP,
            TCP_DEFER_ACCEPT,
            &val as *const _ as *const libc::c_void,
            std::mem::size_of_val(&val) as libc::socklen_t,
        ) < 0
        {
            // Don't fail if not supported
        } else {
            trace!("Applied Linux TCP_DEFER_ACCEPT={}", val);
        }
    }

    // Set TCP keepalive parameters
    if config.keepalive {
        unsafe {
            // Set TCP_KEEPIDLE (time before sending keepalive probes)
            let val: libc::c_int = config.keepalive_time as libc::c_int;
            if libc::setsockopt(
                fd,
                libc::IPPROTO_TCP,
                libc::TCP_KEEPIDLE,
                &val as *const _ as *const libc::c_void,
                std::mem::size_of_val(&val) as libc::socklen_t,
            ) < 0
            {
                let err = io::Error::last_os_error();
                error!("Failed to set TCP_KEEPIDLE: {}", err);
                // Don't fail if this option is not supported
            }

            // Set TCP_KEEPINTVL (interval between keepalive probes)
            let val: libc::c_int = config.keepalive_interval as libc::c_int;
            if libc::setsockopt(
                fd,
                libc::IPPROTO_TCP,
                libc::TCP_KEEPINTVL,
                &val as *const _ as *const libc::c_void,
                std::mem::size_of_val(&val) as libc::socklen_t,
            ) < 0
            {
                let err = io::Error::last_os_error();
                error!("Failed to set TCP_KEEPINTVL: {}", err);
                // Don't fail if this option is not supported
            }

            // Set TCP_KEEPCNT (number of keepalive probes)
            let val: libc::c_int = config.keepalive_probes as libc::c_int;
            if libc::setsockopt(
                fd,
                libc::IPPROTO_TCP,
                libc::TCP_KEEPCNT,
                &val as *const _ as *const libc::c_void,
                std::mem::size_of_val(&val) as libc::socklen_t,
            ) < 0
            {
                let err = io::Error::last_os_error();
                error!("Failed to set TCP_KEEPCNT: {}", err);
                // Don't fail if this option is not supported
            }
        }

        debug!(
            "Applied Linux keepalive parameters: idle={}, interval={}, count={}",
            config.keepalive_time, config.keepalive_interval, config.keepalive_probes
        );
    }

    Ok(())
}

/// Apply macOS-specific socket options
#[cfg(target_os = "macos")]
fn apply_macos_socket_options(
    stream: &TcpStream,
    config: &TcpSocketConfig,
) -> Result<(), IggyError> {
    use std::os::unix::io::AsRawFd;

    let fd = stream.as_raw_fd();

    // Set TCP_NOPUSH if enabled (macOS equivalent of TCP_CORK, mutually exclusive with TCP_NODELAY)
    if config.cork_or_nopush && !config.nodelay {
        unsafe {
            let val: libc::c_int = 1;
            if libc::setsockopt(
                fd,
                libc::IPPROTO_TCP,
                libc::TCP_NOPUSH,
                &val as *const _ as *const libc::c_void,
                std::mem::size_of_val(&val) as libc::socklen_t,
            ) < 0
            {
                let err = io::Error::last_os_error();
                error!("Failed to set TCP_NOPUSH: {}", err);
                // Don't fail if this option is not supported
            } else {
                debug!("Applied macOS TCP_NOPUSH=1");
            }
        }
    }

    // Set TCP keepalive parameters
    if config.keepalive {
        unsafe {
            // Set TCP_KEEPALIVE (time before sending keepalive probes)
            let val: libc::c_int = config.keepalive_time as libc::c_int;
            if libc::setsockopt(
                fd,
                libc::IPPROTO_TCP,
                libc::TCP_KEEPALIVE,
                &val as *const _ as *const libc::c_void,
                std::mem::size_of_val(&val) as libc::socklen_t,
            ) < 0
            {
                let err = io::Error::last_os_error();
                error!("Failed to set TCP_KEEPALIVE: {}", err);
                // Don't fail if this option is not supported
            }

            // macOS doesn't have direct equivalents for TCP_KEEPINTVL and TCP_KEEPCNT
            // but we can use TCP_CONNECTIONTIMEOUT for similar functionality

            debug!(
                "Applied macOS keepalive parameters: keepalive={}",
                config.keepalive_time
            );
        }
    }

    Ok(())
}

/// Public function to optimize a TcpStream based on the provided configuration
pub fn optimize_tcp_stream(stream: &TcpStream, config: &TcpSocketConfig) -> Result<(), IggyError> {
    trace!(
        "Optimizing TCP stream with profile: {:?}",
        config.optimization_profile
    );
    DefaultSocketOptimizer::apply_socket_options(stream, config)
}

/// Public function to create a pre-optimized socket configuration for ultra-low latency
pub fn create_low_latency_config() -> TcpSocketConfig {
    let mut config = TcpSocketConfig::default();
    config.optimization_profile = SocketOptimizationProfile::LowestLatency;

    // Maximum optimizations for ultra-low latency
    config.nodelay = true; // Disable Nagle's algorithm completely
    config.cork_or_nopush = false; // No packet coalescing
    config.quick_ack = true; // Enable quick ACK (Linux)
    config.tcp_fastopen = true; // Fast connection establishment

    // Use smaller buffers for reduced latency
    config.latency_mode_receive_buffer_size = 4 * 1024; // 4KB
    config.latency_mode_send_buffer_size = 4 * 1024; // 4KB

    // Keepalive settings optimized for quick detection of dropped connections
    config.keepalive = true;
    config.keepalive_time = 15; // 15 seconds
    config.keepalive_interval = 5; // 5 seconds
    config.keepalive_probes = 3; // 3 attempts

    config
}

/// Public function to create a pre-optimized socket configuration for maximum throughput
pub fn create_high_throughput_config() -> TcpSocketConfig {
    let mut config = TcpSocketConfig::default();
    config.optimization_profile = SocketOptimizationProfile::HighestThroughput;

    // Optimize for maximum data transfer
    config.nodelay = false; // Enable Nagle's algorithm for better coalescing
    config.cork_or_nopush = true; // Enable packet coalescing

    // Use massive buffers for high throughput
    config.throughput_mode_receive_buffer_size = 16 * 1024 * 1024; // 16MB
    config.throughput_mode_send_buffer_size = 16 * 1024 * 1024; // 16MB

    // Less aggressive keepalive for stable long-running connections
    config.keepalive = true;
    config.keepalive_time = 120; // 2 minutes
    config.keepalive_interval = 30; // 30 seconds
    config.keepalive_probes = 8; // 8 attempts

    config
}

/// Public function to create a balanced socket configuration with good performance characteristics
pub fn create_balanced_config() -> TcpSocketConfig {
    let mut config = TcpSocketConfig::default();

    // Balance between latency and throughput
    config.nodelay = true; // Prefer lower latency by default
    config.cork_or_nopush = false;
    config.quick_ack = true;
    config.tcp_fastopen = true;

    // Balanced buffer sizes
    config.receive_buffer_size = 8 * 1024 * 1024; // 8MB
    config.send_buffer_size = 8 * 1024 * 1024; // 8MB

    // Standard keepalive settings
    config.keepalive = true;
    config.keepalive_time = 60; // 1 minute
    config.keepalive_interval = 10; // 10 seconds
    config.keepalive_probes = 6; // 6 attempts

    config
}
