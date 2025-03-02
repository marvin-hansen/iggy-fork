use crate::test_server::ClientFactory;
use async_trait::async_trait;
use iggy::client::Client;
use iggy::tcp::config::TcpClientConfig;
use iggy::tcp::tcp_client::TcpClient;
use std::sync::Arc;

#[derive(Debug, Clone, Default)]
pub struct TcpClientFactory {
    pub server_addr: String,
    pub nodelay: bool,
}

#[async_trait]
impl ClientFactory for TcpClientFactory {
    async fn create_client(&self) -> Box<dyn Client> {
        let config = TcpClientConfig {
            server_address: self.server_addr.clone(),
            nodelay: self.nodelay,
            ..TcpClientConfig::default()
        };
        let client = TcpClient::create(Arc::new(config)).unwrap_or_else(|e| {
            panic!(
                "Failed to create TcpClient, iggy-server has address {}, error: {:?}",
                self.server_addr, e
            )
        });
        iggy::client::Client::connect(&client)
            .await
            .unwrap_or_else(|e| {
                panic!(
                    "Failed to connect to iggy-server at {}, error: {:?}",
                    self.server_addr, e
                )
            });
        Box::new(client)
    }
}

unsafe impl Send for TcpClientFactory {}
unsafe impl Sync for TcpClientFactory {}
