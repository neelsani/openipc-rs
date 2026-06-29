package dev.openipc.usb

import android.app.Activity
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.hardware.usb.UsbDevice
import android.hardware.usb.UsbDeviceConnection
import android.hardware.usb.UsbManager
import android.os.Build
import androidx.appcompat.app.AppCompatActivity
import app.tauri.annotation.Command
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.Plugin
import java.util.Locale
import java.util.concurrent.ConcurrentHashMap

@TauriPlugin
class OpenIpcUsbPlugin(private val stationActivity: Activity) : Plugin(stationActivity) {
  private val usbManager = stationActivity.getSystemService(Context.USB_SERVICE) as UsbManager
  private val permissionAction = "${stationActivity.packageName}.OPENIPC_USB_PERMISSION"
  private val openConnections = ConcurrentHashMap<Int, UsbDeviceConnection>()

  private var pendingInvoke: Invoke? = null
  private var pendingDevice: UsbDevice? = null

  private val permissionReceiver =
    object : BroadcastReceiver() {
      override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != permissionAction) {
          return
        }

        val invoke: Invoke?
        val fallbackDevice: UsbDevice?
        synchronized(this@OpenIpcUsbPlugin) {
          invoke = pendingInvoke
          fallbackDevice = pendingDevice
          pendingInvoke = null
          pendingDevice = null
        }

        val device = deviceFromIntent(intent) ?: fallbackDevice
        if (invoke == null || device == null) {
          return
        }

        if (intent.getBooleanExtra(UsbManager.EXTRA_PERMISSION_GRANTED, false)) {
          resolveOpenedDevice(invoke, device)
        } else {
          invoke.reject("USB permission denied for ${deviceLabel(device)}")
        }
      }
    }

  init {
    val filter = IntentFilter(permissionAction)
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
      stationActivity.registerReceiver(permissionReceiver, filter, Context.RECEIVER_NOT_EXPORTED)
    } else {
      @Suppress("DEPRECATION")
      stationActivity.registerReceiver(permissionReceiver, filter)
    }
  }

  @Command
  fun listDevices(invoke: Invoke) {
    invoke.resolveObject(matchingDevices().map { devicePayload(it) })
  }

  @Command
  fun openDevice(invoke: Invoke) {
    val args = invoke.getArgs()
    val deviceId = args.getString("deviceId", null)?.takeIf { it.isNotBlank() }
    val vendorId = args.getInteger("vendorId")
    val productId = args.getInteger("productId")
    val device = findDevice(deviceId, vendorId, productId)

    if (device == null) {
      invoke.reject("No supported Realtek USB adapter is attached")
      return
    }

    if (usbManager.hasPermission(device)) {
      resolveOpenedDevice(invoke, device)
      return
    }

    requestPermission(invoke, device)
  }

  @Command
  fun closeDevice(invoke: Invoke) {
    val fd = invoke.getArgs().getInteger("fd")
    if (fd == null) {
      invoke.reject("closeDevice requires an fd")
      return
    }
    openConnections.remove(fd)?.close()
    invoke.resolve()
  }

  override fun onDestroy(activity: AppCompatActivity) {
    try {
      activity.unregisterReceiver(permissionReceiver)
    } catch (_: IllegalArgumentException) {
      // Already unregistered during process teardown.
    }
    openConnections.values.forEach { it.close() }
    openConnections.clear()
  }

  @Synchronized
  private fun requestPermission(invoke: Invoke, device: UsbDevice) {
    if (pendingInvoke != null) {
      invoke.reject("Another USB permission request is already pending")
      return
    }

    pendingInvoke = invoke
    pendingDevice = device

    val intent = Intent(permissionAction).setPackage(stationActivity.packageName)
    val flags =
      PendingIntent.FLAG_UPDATE_CURRENT or
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
          PendingIntent.FLAG_MUTABLE
        } else {
          0
        }
    val permissionIntent = PendingIntent.getBroadcast(stationActivity, 0, intent, flags)
    try {
      usbManager.requestPermission(device, permissionIntent)
    } catch (error: Exception) {
      pendingInvoke = null
      pendingDevice = null
      invoke.reject("Could not request USB permission for ${deviceLabel(device)}", error)
    }
  }

  private fun resolveOpenedDevice(invoke: Invoke, device: UsbDevice) {
    val connection = usbManager.openDevice(device)
    if (connection == null) {
      invoke.reject("Could not open ${deviceLabel(device)}")
      return
    }

    val fd = connection.fileDescriptor
    if (fd < 0) {
      connection.close()
      invoke.reject("Android returned an invalid file descriptor for ${deviceLabel(device)}")
      return
    }

    openConnections.put(fd, connection)?.close()
    val payload = devicePayload(device).toMutableMap()
    payload["fd"] = fd
    invoke.resolveObject(payload)
  }

  private fun findDevice(deviceId: String?, vendorId: Int?, productId: Int?): UsbDevice? {
    val devices = matchingDevices()
    if (!deviceId.isNullOrBlank()) {
      devices.firstOrNull { it.deviceName == deviceId || usbId(it) == deviceId }?.let { return it }
    }
    if (vendorId != null && productId != null) {
      devices.firstOrNull { it.vendorId == vendorId && it.productId == productId }?.let { return it }
    }
    return devices.firstOrNull()
  }

  private fun matchingDevices(): List<UsbDevice> {
    return usbManager.deviceList.values
      .filter { device -> supportedRealtekDevices.any { it.matches(device) } }
      .sortedWith(compareBy<UsbDevice> { it.vendorId }.thenBy { it.productId }.thenBy { it.deviceName })
  }

  private fun devicePayload(device: UsbDevice): Map<String, Any?> {
    val supported = supportedRealtekDevices.firstOrNull { it.matches(device) }
    return linkedMapOf(
      "deviceId" to device.deviceName,
      "vendorId" to device.vendorId,
      "productId" to device.productId,
      "product" to (safeProductName(device) ?: supported?.label),
      "manufacturer" to safeManufacturerName(device),
    )
  }

  private fun deviceLabel(device: UsbDevice): String {
    val product = safeProductName(device) ?: supportedRealtekDevices.firstOrNull { it.matches(device) }?.label
    return if (product.isNullOrBlank()) usbId(device) else "$product (${usbId(device)})"
  }

  private fun usbId(device: UsbDevice): String {
    return String.format(Locale.US, "%04x:%04x", device.vendorId, device.productId)
  }

  private fun safeProductName(device: UsbDevice): String? {
    return try {
      device.productName
    } catch (_: Throwable) {
      null
    }
  }

  private fun safeManufacturerName(device: UsbDevice): String? {
    return try {
      device.manufacturerName
    } catch (_: Throwable) {
      null
    }
  }

  private fun deviceFromIntent(intent: Intent): UsbDevice? {
    return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
      intent.getParcelableExtra(UsbManager.EXTRA_DEVICE, UsbDevice::class.java)
    } else {
      @Suppress("DEPRECATION")
      intent.getParcelableExtra(UsbManager.EXTRA_DEVICE)
    }
  }

}
