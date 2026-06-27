import type { AuthorizedUsbDevice } from "./types";

export function usbId(vendorId: number, productId: number): string {
  return `${vendorId.toString(16).padStart(4, "0")}:${productId.toString(16).padStart(4, "0")}`;
}

export function webUsbDeviceId(device: Pick<USBDevice, "vendorId" | "productId">): string {
  return usbId(device.vendorId, device.productId);
}

export function authorizedDeviceId(device: AuthorizedUsbDevice): string {
  return usbId(device.vendorId, device.productId);
}

export function authorizedDeviceLabel(device: AuthorizedUsbDevice): string {
  const name = [device.manufacturer, device.product].filter(Boolean).join(" ");
  return name ? `${name} (${authorizedDeviceId(device)})` : authorizedDeviceId(device);
}

export function webUsbDeviceLabel(device: USBDevice): string {
  const name = [device.manufacturerName, device.productName].filter(Boolean).join(" ");
  return name ? `${name} (${webUsbDeviceId(device)})` : webUsbDeviceId(device);
}
