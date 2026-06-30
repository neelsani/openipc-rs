use serde::{ser::Serializer, Serialize};

/// Result alias used by the OpenIPC USB Tauri plugin.
pub type Result<T> = std::result::Result<T, Error>;

/// Error returned by the OpenIPC USB Tauri plugin.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Plain string error from the platform bridge.
    #[error("{0}")]
    Message(String),
    /// Android/iOS plugin invocation error from Tauri.
    #[cfg(mobile)]
    #[error(transparent)]
    PluginInvoke(#[from] tauri::plugin::mobile::PluginInvokeError),
}

impl Serialize for Error {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.to_string().as_ref())
    }
}
