use nusb::transfer::TransferError;

use crate::types::DriverError;

pub(crate) const CONTROL_RETRY_ATTEMPTS: usize = 3;
pub(crate) const BULK_RETRY_ATTEMPTS: usize = 2;
pub(crate) const FIRMWARE_BULK_RETRY_ATTEMPTS: usize = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UsbFailureClass {
    Transient,
    EndpointStall,
    Disconnected,
    InvalidRequest,
    Fatal,
}

pub(crate) fn classify_transfer_error(error: TransferError) -> UsbFailureClass {
    match error {
        TransferError::Cancelled => UsbFailureClass::Transient,
        TransferError::Stall => UsbFailureClass::EndpointStall,
        TransferError::Disconnected => UsbFailureClass::Disconnected,
        TransferError::InvalidArgument => UsbFailureClass::InvalidRequest,
        TransferError::Fault | TransferError::Unknown(_) => UsbFailureClass::Fatal,
    }
}

pub(crate) fn should_retry_transfer_error(
    error: TransferError,
    attempt: usize,
    max_attempts: usize,
) -> bool {
    attempt + 1 < max_attempts
        && matches!(
            classify_transfer_error(error),
            UsbFailureClass::Transient | UsbFailureClass::EndpointStall
        )
}

pub(crate) fn retry_delay_ms(attempt: usize) -> u32 {
    match attempt {
        0 => 5,
        1 => 20,
        _ => 50,
    }
}

pub(crate) fn transfer_error(context: impl std::fmt::Display, error: TransferError) -> DriverError {
    DriverError::Nusb(format!("{context}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_recoverable_transfer_errors() {
        assert_eq!(
            classify_transfer_error(TransferError::Cancelled),
            UsbFailureClass::Transient
        );
        assert_eq!(
            classify_transfer_error(TransferError::Stall),
            UsbFailureClass::EndpointStall
        );
    }

    #[test]
    fn classifies_non_recoverable_transfer_errors() {
        assert_eq!(
            classify_transfer_error(TransferError::Disconnected),
            UsbFailureClass::Disconnected
        );
        assert_eq!(
            classify_transfer_error(TransferError::InvalidArgument),
            UsbFailureClass::InvalidRequest
        );
        assert_eq!(
            classify_transfer_error(TransferError::Fault),
            UsbFailureClass::Fatal
        );
        assert_eq!(
            classify_transfer_error(TransferError::Unknown(0xdead)),
            UsbFailureClass::Fatal
        );
    }

    #[test]
    fn retries_only_transient_errors_before_last_attempt() {
        assert!(should_retry_transfer_error(TransferError::Stall, 0, 2));
        assert!(should_retry_transfer_error(TransferError::Cancelled, 0, 2));
        assert!(!should_retry_transfer_error(TransferError::Stall, 1, 2));
        assert!(!should_retry_transfer_error(
            TransferError::Disconnected,
            0,
            2
        ));
        assert!(!should_retry_transfer_error(TransferError::Fault, 0, 2));
    }

    #[test]
    fn retry_delay_caps_after_second_retry() {
        assert_eq!(retry_delay_ms(0), 5);
        assert_eq!(retry_delay_ms(1), 20);
        assert_eq!(retry_delay_ms(2), 50);
        assert_eq!(retry_delay_ms(20), 50);
    }
}
