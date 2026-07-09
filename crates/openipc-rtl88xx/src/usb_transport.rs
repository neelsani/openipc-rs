#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

#[cfg(not(target_arch = "wasm32"))]
use nusb::transfer::{ControlIn, ControlOut, TransferError};
#[cfg(not(target_arch = "wasm32"))]
use nusb::MaybeFuture;

#[cfg(not(target_arch = "wasm32"))]
use crate::regs::{REALTEK_VENDOR_READ_REQUEST, REALTEK_VENDOR_WRITE_REQUEST, USB_TIMEOUT};
#[cfg(not(target_arch = "wasm32"))]
use crate::types::DriverError;
#[cfg(not(target_arch = "wasm32"))]
use crate::usb_recovery::{
    retry_delay_ms, should_retry_transfer_error, transfer_error, CONTROL_RETRY_ATTEMPTS,
};

#[cfg(not(target_arch = "wasm32"))]
use nusb::transfer::{ControlType, Recipient};

#[cfg(not(target_arch = "wasm32"))]
pub(crate) trait UsbControlTransport {
    fn control_in_blocking(
        &self,
        request: ControlIn,
        timeout: Duration,
    ) -> Result<Vec<u8>, TransferError>;

    fn control_out_blocking(
        &self,
        request: ControlOut<'_>,
        timeout: Duration,
    ) -> Result<(), TransferError>;
}

#[cfg(not(target_arch = "wasm32"))]
impl UsbControlTransport for nusb::Interface {
    fn control_in_blocking(
        &self,
        request: ControlIn,
        timeout: Duration,
    ) -> Result<Vec<u8>, TransferError> {
        self.control_in(request, timeout).wait()
    }

    fn control_out_blocking(
        &self,
        request: ControlOut<'_>,
        timeout: Duration,
    ) -> Result<(), TransferError> {
        self.control_out(request, timeout).wait()
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn read_register_with_recovery<T: UsbControlTransport>(
    transport: &T,
    register: u16,
    len: u16,
) -> Result<Vec<u8>, DriverError> {
    for attempt in 0..CONTROL_RETRY_ATTEMPTS {
        let result = transport.control_in_blocking(
            ControlIn {
                control_type: ControlType::Vendor,
                recipient: Recipient::Device,
                request: REALTEK_VENDOR_READ_REQUEST,
                value: register,
                index: 0,
                length: len,
            },
            USB_TIMEOUT,
        );
        match result {
            Ok(bytes) => return Ok(bytes),
            Err(err) if should_retry_transfer_error(err, attempt, CONTROL_RETRY_ATTEMPTS) => {
                log::warn!(
                    target: "openipc_rtl88xx::usb",
                    "retrying failed register read register=0x{register:04x} attempt={}: {err}",
                    attempt + 1
                );
                sleep_retry(attempt);
            }
            Err(err) => {
                return Err(transfer_error(
                    format!("vendor read 0x{register:04x} failed"),
                    err,
                ));
            }
        }
    }
    unreachable!("retry loop either returns or reports the final USB error")
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn write_register_with_recovery<T: UsbControlTransport>(
    transport: &T,
    register: u16,
    bytes: &[u8],
) -> Result<(), DriverError> {
    for attempt in 0..CONTROL_RETRY_ATTEMPTS {
        let result = transport.control_out_blocking(
            ControlOut {
                control_type: ControlType::Vendor,
                recipient: Recipient::Device,
                request: REALTEK_VENDOR_WRITE_REQUEST,
                value: register,
                index: 0,
                data: bytes,
            },
            USB_TIMEOUT,
        );
        match result {
            Ok(()) => return Ok(()),
            Err(err) if should_retry_transfer_error(err, attempt, CONTROL_RETRY_ATTEMPTS) => {
                log::warn!(
                    target: "openipc_rtl88xx::usb",
                    "retrying failed register write register=0x{register:04x} attempt={}: {err}",
                    attempt + 1
                );
                sleep_retry(attempt);
            }
            Err(err) => {
                return Err(transfer_error(
                    format!("vendor write 0x{register:04x} failed"),
                    err,
                ));
            }
        }
    }
    unreachable!("retry loop either returns or reports the final USB error")
}

#[cfg(not(target_arch = "wasm32"))]
fn sleep_retry(attempt: usize) {
    std::thread::sleep(Duration::from_millis(retry_delay_ms(attempt) as u64));
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct FakeControlTransport {
        in_results: RefCell<Vec<Result<Vec<u8>, TransferError>>>,
        out_results: RefCell<Vec<Result<(), TransferError>>>,
    }

    impl FakeControlTransport {
        fn with_in_results(results: Vec<Result<Vec<u8>, TransferError>>) -> Self {
            Self {
                in_results: RefCell::new(results),
                out_results: RefCell::default(),
            }
        }

        fn with_out_results(results: Vec<Result<(), TransferError>>) -> Self {
            Self {
                in_results: RefCell::default(),
                out_results: RefCell::new(results),
            }
        }
    }

    impl UsbControlTransport for FakeControlTransport {
        fn control_in_blocking(
            &self,
            _request: ControlIn,
            _timeout: Duration,
        ) -> Result<Vec<u8>, TransferError> {
            self.in_results.borrow_mut().remove(0)
        }

        fn control_out_blocking(
            &self,
            _request: ControlOut<'_>,
            _timeout: Duration,
        ) -> Result<(), TransferError> {
            self.out_results.borrow_mut().remove(0)
        }
    }

    #[test]
    fn register_read_retries_stall_then_succeeds() {
        let fake = FakeControlTransport::with_in_results(vec![
            Err(TransferError::Stall),
            Ok(vec![0x34, 0x12]),
        ]);

        let bytes = read_register_with_recovery(&fake, 0x1234, 2).unwrap();

        assert_eq!(bytes, vec![0x34, 0x12]);
        assert!(fake.in_results.borrow().is_empty());
    }

    #[test]
    fn register_write_retries_cancelled_then_succeeds() {
        let fake =
            FakeControlTransport::with_out_results(vec![Err(TransferError::Cancelled), Ok(())]);

        write_register_with_recovery(&fake, 0x1234, &[0xab]).unwrap();

        assert!(fake.out_results.borrow().is_empty());
    }

    #[test]
    fn register_read_does_not_retry_disconnect() {
        let fake = FakeControlTransport::with_in_results(vec![
            Err(TransferError::Disconnected),
            Ok(vec![0xff]),
        ]);

        let err = read_register_with_recovery(&fake, 0x1234, 1).unwrap_err();

        assert!(err.to_string().contains("device disconnected"));
        assert_eq!(fake.in_results.borrow().len(), 1);
    }
}
