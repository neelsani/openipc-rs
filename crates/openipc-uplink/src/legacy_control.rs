use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

use crate::{SshError, VirtualTcpStream};

const MAX_RESPONSE_BYTES: usize = 64 * 1024;

/// Client for the OpenIPC compatibility service on TCP port 12355.
pub struct LegacyControlClient;

impl LegacyControlClient {
    /// Send one newline-terminated command and close the write side.
    pub async fn send(mut stream: VirtualTcpStream, command: &str) -> Result<(), SshError> {
        stream.write_all(command.as_bytes()).await?;
        if !command.ends_with('\n') {
            stream.write_all(b"\n").await?;
        }
        stream.shutdown().await?;
        Ok(())
    }

    /// Send one newline-terminated command and collect its bounded response.
    pub async fn request(stream: VirtualTcpStream, command: &str) -> Result<Vec<u8>, SshError> {
        Self::send(stream.clone(), command).await?;
        let mut response = Vec::new();
        stream
            .take((MAX_RESPONSE_BYTES + 1) as u64)
            .read_to_end(&mut response)
            .await?;
        if response.len() > MAX_RESPONSE_BYTES {
            return Err(SshError::OutputLimitExceeded);
        }
        Ok(response)
    }
}
