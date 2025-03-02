use crate::error::IggyError;
use async_trait::async_trait;

#[async_trait]
pub(crate) trait ConnectionStream {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, IggyError>;
    async fn write(&mut self, buf: &[u8]) -> Result<(), IggyError>;
    async fn flush(&mut self) -> Result<(), IggyError>;
    async fn shutdown(&mut self) -> Result<(), IggyError>;
}
