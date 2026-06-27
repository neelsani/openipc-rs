---
sidebar_position: 9
---

# Realtek Driver

`openipc-rtl88xx` is the shared Rust Realtek USB/HAL driver.

## Supported Operations

- descriptor-driven endpoint discovery,
- vendor-control register reads and writes through request `0x05`,
- firmware download for supported Jaguar-family chips,
- LLT/page setup and queue/FIFO setup,
- MAC/BB/RF table loading,
- monitor filters,
- channel and channel-width setup,
- RX bulk reads,
- TX bulk writes for adaptive-link feedback.

## Native And WebUSB Sharing

The HAL is async and transport-oriented. Native builds use `nusb` for desktop
USB. Browser builds use the WebUSB-capable `nusb-webusb` package after the user
grants the device in JavaScript.

## Validation Boundary

The driver is intended to be standalone and does not build against devourer.
However, hardware bring-up still needs register-trace comparison and live
adapter tests before each supported chip should be marked final.

Current status:

- RTL8812/RTL8821 cold initialization is implemented and needs live validation.
- RTL8814 reserved-page/DDMA firmware download is implemented and needs live
  validation.
- EFUSE/RFE parsing is still conservative and should be expanded with hardware
  fixtures.
