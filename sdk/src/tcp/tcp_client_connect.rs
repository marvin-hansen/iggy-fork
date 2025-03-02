use crate::binary::{BinaryTransport, ClientState};
use crate::client::{AutoLogin, Credentials, PersonalAccessTokenClient, UserClient};
use crate::diagnostic::DiagnosticEvent;
use crate::error::IggyError;
use crate::tcp::tcp_client::TcpClient;
use crate::tcp::tcp_client_fields::NAME;
use crate::tcp::tcp_connection_stream::TcpConnectionStream;
use crate::tcp::tcp_connection_stream_kind::ConnectionStreamKind;
use crate::tcp::tcp_tls_connection_stream::TcpTlsConnectionStream;
use crate::utils::duration::IggyDuration;
use crate::utils::timestamp::IggyTimestamp;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, ServerName};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::time::sleep;
use tokio_rustls::{TlsConnector, TlsStream};
use tracing::{error, info, trace, warn};

impl TcpClient {
    pub(crate) async fn connect(&self) -> Result<(), IggyError> {
        match self.get_state().await {
            ClientState::Shutdown => {
                trace!("Cannot connect. Client is shutdown.");
                return Err(IggyError::ClientShutdown);
            }
            ClientState::Connected | ClientState::Authenticating | ClientState::Authenticated => {
                let client_address = self.get_client_address_value().await;
                trace!("Client: {client_address} is already connected.");
                return Ok(());
            }
            ClientState::Connecting => {
                trace!("Client is already connecting.");
                return Ok(());
            }
            _ => {}
        }

        self.set_state(ClientState::Connecting).await;
        if let Some(connected_at) = self.connected_at.load() {
            let now = IggyTimestamp::now();
            let elapsed = now.as_micros() - connected_at.as_micros();
            let interval = self.config.reconnection.reestablish_after.as_micros();
            trace!(
                "Elapsed time since last connection: {}",
                IggyDuration::from(elapsed)
            );
            if elapsed < interval {
                let remaining = IggyDuration::from(interval - elapsed);
                info!("Trying to connect to the server in: {remaining}",);
                sleep(remaining.get_duration()).await;
            }
        }

        let tls_enabled = self.config.tls_enabled;
        let mut retry_count = 0;
        let connection_stream: ConnectionStreamKind;
        let remote_address;
        let client_address;
        loop {
            info!(
                "{NAME} client is connecting to server: {}...",
                self.config.server_address
            );

            let connection = TcpStream::connect(&self.config.server_address).await;
            if connection.is_err() {
                error!(
                    "Failed to connect to server: {}",
                    self.config.server_address
                );
                if !self.config.reconnection.enabled {
                    warn!("Automatic reconnection is disabled.");
                    return Err(IggyError::CannotEstablishConnection);
                }

                let unlimited_retries = self.config.reconnection.max_retries.is_none();
                let max_retries = self.config.reconnection.max_retries.unwrap_or_default();
                let max_retries_str =
                    if let Some(max_retries) = self.config.reconnection.max_retries {
                        max_retries.to_string()
                    } else {
                        "unlimited".to_string()
                    };

                let interval_str = self.config.reconnection.interval.as_human_time_string();
                if unlimited_retries || retry_count < max_retries {
                    retry_count += 1;
                    info!(
                        "Retrying to connect to server ({retry_count}/{max_retries_str}): {} in: {interval_str}",
                        self.config.server_address,
                    );
                    sleep(self.config.reconnection.interval.get_duration()).await;
                    continue;
                }

                self.set_state(ClientState::Disconnected).await;
                self.publish_event(DiagnosticEvent::Disconnected).await;
                return Err(IggyError::CannotEstablishConnection);
            }

            let stream = connection.map_err(|error| {
                error!("Failed to establish TCP connection to the server: {error}",);
                IggyError::CannotEstablishConnection
            })?;
            client_address = stream.local_addr().map_err(|error| {
                error!("Failed to get the local address of the client: {error}",);
                IggyError::CannotEstablishConnection
            })?;
            remote_address = stream.peer_addr().map_err(|error| {
                error!("Failed to get the remote address of the server: {error}",);
                IggyError::CannotEstablishConnection
            })?;
            self.client_address.store(Some(client_address));

            if let Err(e) = stream.set_nodelay(self.config.nodelay) {
                error!("Failed to set the nodelay option on the client: {e}, continuing...",);
            }

            if !tls_enabled {
                connection_stream =
                    ConnectionStreamKind::Tcp(TcpConnectionStream::new(client_address, stream));
                break;
            }

            let mut root_cert_store = rustls::RootCertStore::empty();
            if let Some(certificate_path) = &self.config.tls_ca_file {
                for cert in CertificateDer::pem_file_iter(certificate_path).map_err(|error| {
                    error!("Failed to read the CA file: {certificate_path}. {error}",);
                    IggyError::InvalidTlsCertificatePath
                })? {
                    let certificate = cert.map_err(|error| {
                        error!(
                            "Failed to read a certificate from the CA file: {certificate_path}. {error}",
                        );
                        IggyError::InvalidTlsCertificate
                    })?;
                    root_cert_store.add(certificate).map_err(|error| {
                        error!(
                            "Failed to add a certificate to the root certificate store. {error}",
                        );
                        IggyError::InvalidTlsCertificate
                    })?;
                }
            } else {
                root_cert_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            }

            let config = rustls::ClientConfig::builder()
                .with_root_certificates(root_cert_store)
                .with_no_client_auth();
            let connector = TlsConnector::from(Arc::new(config));
            let stream = TcpStream::connect(client_address).await.map_err(|error| {
                error!("Failed to establish TCP connection to the server: {error}",);
                IggyError::CannotEstablishConnection
            })?;
            let tls_domain = self.config.tls_domain.to_owned();
            let domain = ServerName::try_from(tls_domain).map_err(|error| {
                error!("Failed to create a server name from the domain. {error}",);
                IggyError::InvalidTlsDomain
            })?;
            let stream = connector.connect(domain, stream).await.map_err(|error| {
                error!("Failed to establish a TLS connection to the server: {error}",);
                IggyError::CannotEstablishConnection
            })?;
            connection_stream = ConnectionStreamKind::TcpTls(TcpTlsConnectionStream::new(
                client_address,
                TlsStream::Client(stream),
            ));
            break;
        }

        let now = IggyTimestamp::now();
        info!(
            "{NAME} client: {client_address} has connected to server: {remote_address} at: {now}",
        );
        self.stream.write().await.replace(connection_stream);
        self.set_state(ClientState::Connected).await;
        self.connected_at.store(Some(now));
        self.publish_event(DiagnosticEvent::Connected).await;
        match &self.config.auto_login {
            AutoLogin::Disabled => {
                info!("Automatic sign-in is disabled.");
                Ok(())
            }
            AutoLogin::Enabled(credentials) => {
                info!("{NAME} client: {client_address} is signing in...");
                self.set_state(ClientState::Authenticating).await;
                match credentials {
                    Credentials::UsernamePassword(username, password) => {
                        self.login_user(username, password).await?;
                        info!("{NAME} client: {client_address} has signed in with the user credentials, username: {username}",);
                        Ok(())
                    }
                    Credentials::PersonalAccessToken(token) => {
                        self.login_with_personal_access_token(token).await?;
                        info!("{NAME} client: {client_address} has signed in with a personal access token.",);
                        Ok(())
                    }
                }
            }
        }
    }
}
