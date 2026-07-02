use windows::{
    core::Owned,
    Win32::{
        Media::MediaFoundation::{MFShutdown, MFStartup, MFSTARTUP_FULL, MF_VERSION},
        System::Com::{CoIncrementMTAUsage, CO_MTA_USAGE_COOKIE},
    },
};

use crate::VideoError;

pub(crate) struct MediaFoundationRuntime {
    _mta: Owned<CO_MTA_USAGE_COOKIE>,
}

impl MediaFoundationRuntime {
    pub(crate) fn new() -> Result<Self, VideoError> {
        // SAFETY: The returned cookie is uniquely owned by `Owned` and keeps
        // the process MTA alive until this runtime is dropped.
        let cookie = unsafe { CoIncrementMTAUsage() }
            .map_err(|error| platform_error("CoIncrementMTAUsage", error))?;
        // SAFETY: Startup is process-wide, reference counted, and balanced by
        // this type's Drop implementation.
        if let Err(error) = unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) } {
            // SAFETY: `cookie` was returned as an owned MTA usage reference.
            drop(unsafe { Owned::new(cookie) });
            return Err(platform_error("MFStartup", error));
        }
        Ok(Self {
            // SAFETY: The cookie is owned and has not been transferred.
            _mta: unsafe { Owned::new(cookie) },
        })
    }
}

impl Drop for MediaFoundationRuntime {
    fn drop(&mut self) {
        // SAFETY: Balances the successful MFStartup call in `new`.
        let _ = unsafe { MFShutdown() };
    }
}

pub(crate) fn platform_error(api: &'static str, error: windows::core::Error) -> VideoError {
    VideoError::Platform {
        api,
        status: error.code().0,
    }
}
