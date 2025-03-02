use crate::binary::binary_client::BinaryClient;
use crate::binary::ClientState;
use crate::client::{AutoLogin, Client, ConnectionString};
use crate::diagnostic::DiagnosticEvent;
use crate::error::IggyError;
use crate::tcp::buffer_pool;
use crate::tcp::config_client::TcpClientConfig;
use crate::tcp::tcp_connection_stream_kind::ConnectionStreamKind;
use crate::utils::duration::IggyDuration;
use crate::utils::timestamp::IggyTimestamp;
use async_broadcast::{broadcast, Receiver, Sender};
use async_trait::async_trait;
use crossbeam_utils::atomic::AtomicCell;
use std::fmt::Debug;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock as TokioRwLock;

/// TCP client for interacting with the Iggy API.
/// It requires a valid server address.
#[derive(Debug)]
pub struct TcpClient {
    // Using AtomicCell for frequently accessed client address with minimal contention
    // This allows lock-free reads and atomic updates
    pub(crate) client_address: AtomicCell<Option<SocketAddr>>,
    // TcpClientConfig is immutable thus no need for a RwLock
    pub(crate) config: Arc<TcpClientConfig>,
    // Using TokioRwLock instead of Mutex to allow multiple simultaneous readers
    // This significantly reduces contention as most operations only need read access
    pub(crate) stream: Arc<TokioRwLock<Option<ConnectionStreamKind>>>,
    // Use atomic for the state since it's a simple enum that fits in a AtomicU8
    // which is faster than Mutex or RwLock.
    pub(crate) state: AtomicU8,
    // Using AtomicCell for connection timestamp with minimal contention
    // This eliminates lock acquisition for connection time checks
    pub(crate) connected_at: AtomicCell<Option<IggyTimestamp>>,
    // Track last reconnection attempt for lock-free reconnection logic
    pub(crate) last_reconnect_attempt: AtomicCell<Option<IggyTimestamp>>,
    // Keep events as is
    pub(crate) events: (Sender<DiagnosticEvent>, Receiver<DiagnosticEvent>),
}

impl TcpClient {
    /// Create a new TCP client for the provided server address.
    pub fn new(
        server_address: &str,
        auto_sign_in: AutoLogin,
        heartbeat_interval: IggyDuration,
    ) -> Result<Self, IggyError> {
        Self::create(Arc::new(TcpClientConfig {
            heartbeat_interval,
            server_address: server_address.to_string(),
            auto_login: auto_sign_in,
            ..Default::default()
        }))
    }

    /// Create a new TCP client for the provided server address using TLS.
    pub fn new_tls(
        server_address: &str,
        domain: &str,
        auto_sign_in: AutoLogin,
        heartbeat_interval: IggyDuration,
    ) -> Result<Self, IggyError> {
        Self::create(Arc::new(TcpClientConfig {
            heartbeat_interval,
            server_address: server_address.to_string(),
            tls_enabled: true,
            tls_domain: domain.to_string(),
            auto_login: auto_sign_in,
            ..Default::default()
        }))
    }

    pub fn from_connection_string(connection_string: &str) -> Result<Self, IggyError> {
        Self::create(Arc::new(
            ConnectionString::from_str(connection_string)?.into(),
        ))
    }

    /// Create a new TCP client based on the provided configuration.
    pub fn create(config: Arc<TcpClientConfig>) -> Result<Self, IggyError> {
        // Initialize buffer pools to reduce allocation latency in critical path
        // This is done lazily once for the entire application
        static BUFFER_POOL_INITIALIZED: once_cell::sync::OnceCell<()> =
            once_cell::sync::OnceCell::new();
        BUFFER_POOL_INITIALIZED.get_or_init(|| {
            buffer_pool::initialize_buffer_pools();
            ()
        });

        Ok(Self {
            config,
            client_address: AtomicCell::new(None),
            stream: Arc::new(TokioRwLock::new(None)),
            state: AtomicU8::new(ClientState::Disconnected as u8),
            events: broadcast(1000),
            connected_at: AtomicCell::new(None),
            last_reconnect_attempt: AtomicCell::new(None),
        })
    }

    /// Fast, non-blocking check if the client is disconnected.
    ///
    /// # Returns
    ///
    /// true if the client is in Disconnected state, false otherwise.
    pub(crate) fn is_disconnected(&self) -> bool {
        self.state.load(Ordering::Relaxed) == ClientState::Disconnected as u8
    }

    /// Sync version of is_disconnected for performance-critical paths.
    /// Functionally identical to is_disconnected but explicitly named for clarity.
    ///
    /// # Returns
    ///
    /// true if the client is in Disconnected state, false otherwise.
    #[inline(always)]
    pub(crate) fn is_disconnected_sync(&self) -> bool {
        self.state.load(Ordering::Relaxed) == ClientState::Disconnected as u8
    }

    /// Fast, non-blocking check if the client is shut down.
    ///
    /// # Returns
    ///
    /// true if the client is in Shutdown state, false otherwise.
    pub(crate) fn is_shutdown(&self) -> bool {
        self.state.load(Ordering::Relaxed) == ClientState::Shutdown as u8
    }

    /// Sync version of is_shutdown for performance-critical paths.
    /// Functionally identical to is_shutdown but explicitly named for clarity.
    ///
    /// # Returns
    ///
    /// true if the client is in Shutdown state, false otherwise.
    #[inline(always)]
    pub(crate) fn is_shutdown_sync(&self) -> bool {
        self.state.load(Ordering::Relaxed) == ClientState::Shutdown as u8
    }

    /// Gets the client's address as a string, or "Unknown" if not connected.
    ///
    /// # Returns
    ///
    /// A string representing the client's address.
    pub(crate) async fn get_client_address_value(&self) -> String {
        // AtomicCell load is lock-free and doesn't need a guard
        if let Some(client_address) = self.client_address.load() {
            client_address.to_string()
        } else {
            String::from("Unknown")
        }
    }

    // Non-blocking version of get_client_address_value that doesn't require await
    pub(crate) fn get_client_address_value_sync(&self) -> String {
        if let Some(client_address) = self.client_address.load() {
            client_address.to_string()
        } else {
            String::from("Unknown")
        }
    }
}

impl BinaryClient for TcpClient {}

#[async_trait]
impl Client for TcpClient {
    async fn connect(&self) -> Result<(), IggyError> {
        TcpClient::connect(self).await
    }

    async fn disconnect(&self) -> Result<(), IggyError> {
        TcpClient::disconnect(self).await
    }

    async fn shutdown(&self) -> Result<(), IggyError> {
        TcpClient::shutdown(self).await
    }

    async fn subscribe_events(&self) -> Receiver<DiagnosticEvent> {
        self.events.1.clone()
    }
}

impl Default for TcpClient {
    fn default() -> Self {
        TcpClient::create(Arc::new(TcpClientConfig::default())).unwrap()
    }
}
