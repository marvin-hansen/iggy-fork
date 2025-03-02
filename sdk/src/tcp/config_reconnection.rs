use crate::utils::duration::IggyDuration;
use std::str::FromStr;

#[derive(Debug, Clone)]
pub struct TcpClientReconnectionConfig {
    pub enabled: bool,
    pub max_retries: Option<u32>,
    pub interval: IggyDuration,
    pub reestablish_after: IggyDuration,
}

impl Default for TcpClientReconnectionConfig {
    fn default() -> TcpClientReconnectionConfig {
        TcpClientReconnectionConfig {
            enabled: true,
            max_retries: None,
            interval: IggyDuration::from_str("1s").unwrap(),
            reestablish_after: IggyDuration::from_str("5s").unwrap(),
        }
    }
}
