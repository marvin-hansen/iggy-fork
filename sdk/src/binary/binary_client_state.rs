use derive_more::Display;

/// The state of the client.
#[derive(Debug, Copy, Clone, PartialEq, Display)]
#[repr(u8)]
pub enum ClientState {
    /// The client is shutdown.
    #[display("shutdown")]
    Shutdown = 0,
    /// The client is disconnected.
    #[display("disconnected")]
    Disconnected = 1,
    /// The client is connecting.
    #[display("connecting")]
    Connecting = 2,
    /// The client is connected.
    #[display("connected")]
    Connected = 3,
    /// The client is authenticating.
    #[display("authenticating")]
    Authenticating = 4,
    /// The client is connected and authenticated.
    #[display("authenticated")]
    Authenticated = 5,
}

impl From<ClientState> for u8 {
    fn from(value: ClientState) -> Self {
        value as u8
    }
}

impl From<u8> for ClientState {
    fn from(value: u8) -> Self {
        match value {
            0 => ClientState::Shutdown,
            1 => ClientState::Disconnected,
            2 => ClientState::Connecting,
            3 => ClientState::Connected,
            4 => ClientState::Authenticating,
            5 => ClientState::Authenticated,
            // We cannot extend th Enum range without breaking compatibility
            // But we also need to a way to catch invalid values without a panic thus Disconnected as fallback
            _ => ClientState::Disconnected,
        }
    }
}
