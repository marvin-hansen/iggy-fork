use crate::error::{IggyError, IggyErrorDiscriminants};
use crate::tcp::tcp_client::TcpClient;
use crate::tcp::tcp_connection_stream_kind::ConnectionStreamKind;
use bytes::{Bytes, BytesMut};
use tracing::{error, trace};

impl TcpClient {
    pub(crate) async fn handle_response(
        &self,
        status: u32,
        length: u32,
        stream: &mut ConnectionStreamKind,
    ) -> Result<Bytes, IggyError> {
        if status != 0 {
            // TEMP: See https://github.com/iggy-rs/iggy/pull/604 for context.
            if status == IggyErrorDiscriminants::TopicIdAlreadyExists as u32
                || status == IggyErrorDiscriminants::TopicNameAlreadyExists as u32
                || status == IggyErrorDiscriminants::StreamIdAlreadyExists as u32
                || status == IggyErrorDiscriminants::StreamNameAlreadyExists as u32
                || status == IggyErrorDiscriminants::UserAlreadyExists as u32
                || status == IggyErrorDiscriminants::PersonalAccessTokenAlreadyExists as u32
                || status == IggyErrorDiscriminants::ConsumerGroupIdAlreadyExists as u32
                || status == IggyErrorDiscriminants::ConsumerGroupNameAlreadyExists as u32
            {
                tracing::debug!(
                    "Received a server resource already exists response: {} ({})",
                    status,
                    IggyError::from_code_as_string(status)
                )
            } else {
                error!(
                    "Received an invalid response with status: {} ({}).",
                    status,
                    IggyError::from_code_as_string(status),
                );
            }

            return Err(IggyError::from_code(status));
        }

        trace!("Status: OK. Response length: {}", length);
        if length <= 1 {
            return Ok(Bytes::new());
        }

        let mut response_buffer = BytesMut::with_capacity(length as usize);
        // Avoid Zeroing Buffers: Initialize the buffer without zeroing it
        unsafe {
            response_buffer.set_len(length as usize);
        }

        stream.read(&mut response_buffer).await?;
        Ok(response_buffer.freeze())
    }
}
