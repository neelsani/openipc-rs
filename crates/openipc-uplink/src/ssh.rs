use std::{fmt, sync::Arc};

use russh::{
    client,
    keys::ssh_key::{HashAlg, PublicKey},
    ChannelMsg, Disconnect,
};
use tokio::io::AsyncWriteExt as _;

use crate::VirtualTcpStream;

/// Username used by stock OpenIPC FPV images.
pub const DEFAULT_SSH_USERNAME: &str = "root";
/// Password used by stock OpenIPC FPV images.
pub const DEFAULT_SSH_PASSWORD: &str = "12345";
const MAX_COMMAND_OUTPUT: usize = 2 * 1024 * 1024;

/// Host-key verification policy for the embedded VTX SSH client.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum HostKeyPolicy {
    /// Match PixelPilot's compatibility behavior and accept the VTX host key.
    #[default]
    AcceptAny,
    /// Require the SHA-256 fingerprint emitted by OpenSSH tools.
    Sha256(String),
}

/// Password credentials for an existing OpenIPC firmware image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshCredentials {
    pub username: String,
    pub password: String,
    pub host_key: HostKeyPolicy,
}

impl Default for SshCredentials {
    fn default() -> Self {
        Self {
            username: DEFAULT_SSH_USERNAME.to_owned(),
            password: DEFAULT_SSH_PASSWORD.to_owned(),
            host_key: HostKeyPolicy::AcceptAny,
        }
    }
}

/// Captured result of one command executed on the VTX.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommandOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_status: Option<u32>,
}

impl CommandOutput {
    /// Return stdout as UTF-8 with invalid sequences replaced.
    pub fn stdout_lossy(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    /// Whether the remote command reported success.
    pub fn success(&self) -> bool {
        self.exit_status == Some(0)
    }
}

/// SSH connection or command failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SshError {
    Protocol(String),
    AuthenticationRejected,
    HostKeyRejected { observed: String },
    OutputLimitExceeded,
    RemoteCommandFailed(CommandOutput),
}

impl fmt::Display for SshError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Protocol(error) => write!(formatter, "SSH protocol error: {error}"),
            Self::AuthenticationRejected => formatter.write_str("VTX rejected SSH credentials"),
            Self::HostKeyRejected { observed } => {
                write!(
                    formatter,
                    "VTX SSH host key does not match (observed {observed})"
                )
            }
            Self::OutputLimitExceeded => formatter.write_str("VTX command output exceeded 2 MiB"),
            Self::RemoteCommandFailed(output) => write!(
                formatter,
                "VTX command failed with status {:?}: {}",
                output.exit_status,
                String::from_utf8_lossy(&output.stderr)
            ),
        }
    }
}

impl std::error::Error for SshError {}

impl From<russh::Error> for SshError {
    fn from(error: russh::Error) -> Self {
        Self::Protocol(error.to_string())
    }
}

impl From<std::io::Error> for SshError {
    fn from(error: std::io::Error) -> Self {
        Self::Protocol(error.to_string())
    }
}

#[derive(Clone)]
struct ClientHandler {
    host_key: HostKeyPolicy,
}

impl client::Handler for ClientHandler {
    type Error = SshError;

    async fn check_server_key(&mut self, key: &PublicKey) -> Result<bool, Self::Error> {
        let observed = key.fingerprint(HashAlg::Sha256).to_string();
        match &self.host_key {
            HostKeyPolicy::AcceptAny => Ok(true),
            HostKeyPolicy::Sha256(expected) if expected.trim() == observed => Ok(true),
            HostKeyPolicy::Sha256(_) => Err(SshError::HostKeyRejected { observed }),
        }
    }
}

/// Authenticated SSH session over a [`VirtualTcpStream`].
pub struct SshClient {
    session: client::Handle<ClientHandler>,
}

impl SshClient {
    /// Complete SSH key exchange and password authentication over userspace TCP.
    pub async fn connect(
        stream: VirtualTcpStream,
        credentials: SshCredentials,
    ) -> Result<Self, SshError> {
        #[cfg(target_os = "android")]
        let client_id = russh::SshId::Standard("SSH-2.0-openipc-rs".into());
        #[cfg(not(target_os = "android"))]
        let client_id = russh::SshId::Standard("SSH-2.0-openipc-rs".to_owned());
        let config = Arc::new(client::Config {
            client_id,
            inactivity_timeout: None,
            keepalive_interval: None,
            ..client::Config::default()
        });
        let handler = ClientHandler {
            host_key: credentials.host_key,
        };
        let mut session = client::connect_stream(config, stream, handler).await?;
        let authentication = session
            .authenticate_password(credentials.username, credentials.password)
            .await?;
        if !authentication.success() {
            return Err(SshError::AuthenticationRejected);
        }
        Ok(Self { session })
    }

    /// Execute a shell command and capture stdout, stderr, and exit status.
    pub async fn execute(&self, command: &str) -> Result<CommandOutput, SshError> {
        let mut channel = self.session.channel_open_session().await?;
        channel.exec(true, command.as_bytes()).await?;
        collect_command_output(&mut channel).await
    }

    /// Execute a command and require a zero exit status.
    pub async fn execute_checked(&self, command: &str) -> Result<CommandOutput, SshError> {
        let output = self.execute(command).await?;
        if output.success() {
            Ok(output)
        } else {
            Err(SshError::RemoteCommandFailed(output))
        }
    }

    /// Read a remote file without interpreting its contents as text.
    pub async fn read_file(&self, path: &str) -> Result<Vec<u8>, SshError> {
        let output = self
            .execute_checked(&format!("cat -- {}", shell_quote(path)))
            .await?;
        Ok(output.stdout)
    }

    /// Read an optional firmware file, returning an empty vector when absent.
    pub async fn read_optional_file(&self, path: &str) -> Result<Vec<u8>, SshError> {
        let output = self
            .execute(&format!("cat -- {} 2>/dev/null || true", shell_quote(path)))
            .await?;
        Ok(output.stdout)
    }

    /// Replace a remote file with binary-safe channel data.
    pub async fn write_file(&self, path: &str, contents: &[u8]) -> Result<(), SshError> {
        let mut channel = self.session.channel_open_session().await?;
        channel
            .exec(true, format!("cat > {}", shell_quote(path)).as_bytes())
            .await?;
        let mut writer = channel.make_writer();
        writer.write_all(contents).await?;
        writer.shutdown().await?;
        drop(writer);
        channel.eof().await?;
        let output = collect_command_output(&mut channel).await?;
        if output.success() {
            Ok(())
        } else {
            Err(SshError::RemoteCommandFailed(output))
        }
    }

    /// Gracefully disconnect the SSH transport.
    pub async fn disconnect(&self) -> Result<(), SshError> {
        self.session
            .disconnect(Disconnect::ByApplication, "", "en")
            .await?;
        Ok(())
    }
}

async fn collect_command_output(
    channel: &mut russh::Channel<client::Msg>,
) -> Result<CommandOutput, SshError> {
    let mut output = CommandOutput::default();
    while let Some(message) = channel.wait().await {
        match message {
            ChannelMsg::Data { data } => append_limited(&mut output.stdout, &data)?,
            ChannelMsg::ExtendedData { data, .. } => append_limited(&mut output.stderr, &data)?,
            ChannelMsg::ExitStatus { exit_status } => output.exit_status = Some(exit_status),
            _ => {}
        }
    }
    Ok(output)
}

fn append_limited(destination: &mut Vec<u8>, bytes: &[u8]) -> Result<(), SshError> {
    if destination.len().saturating_add(bytes.len()) > MAX_COMMAND_OUTPUT {
        return Err(SshError::OutputLimitExceeded);
    }
    destination.extend_from_slice(bytes);
    Ok(())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::{shell_quote, CommandOutput};

    #[test]
    fn shell_quote_handles_single_quotes() {
        assert_eq!(shell_quote("/tmp/a'b"), "'/tmp/a'\\''b'");
    }

    #[test]
    fn command_success_requires_explicit_zero() {
        assert!(!CommandOutput::default().success());
        assert!(CommandOutput {
            exit_status: Some(0),
            ..CommandOutput::default()
        }
        .success());
    }
}
