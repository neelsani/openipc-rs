package dev.openipc.usb

import android.app.Activity
import android.content.Intent
import android.net.VpnService
import android.os.ParcelFileDescriptor
import app.tauri.plugin.Invoke
import java.util.concurrent.ConcurrentHashMap

class OpenIpcVpnService : VpnService() {
  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    when (intent?.action) {
      ACTION_OPEN -> establishForPendingInvoke()
      ACTION_CLOSE_ALL -> closeAll()
    }
    return START_NOT_STICKY
  }

  override fun onDestroy() {
    closeAll()
    super.onDestroy()
  }

  private fun establishForPendingInvoke() {
    val invoke = synchronized(lock) {
      val pending = pendingInvoke
      pendingInvoke = null
      pending
    }
    if (invoke == null) {
      stopSelf()
      return
    }

    try {
      val vpnInterface = Builder()
        .setSession(INTERFACE_NAME)
        .addAddress(OPENIPC_VPN_ADDRESS, OPENIPC_VPN_PREFIX)
        .addRoute(OPENIPC_VPN_ROUTE, OPENIPC_VPN_PREFIX)
        .setMtu(OPENIPC_VPN_MTU)
        .establish()

      if (vpnInterface == null) {
        invoke.reject("Android did not create the OpenIPC VPN interface")
        stopSelf()
        return
      }

      val fd = vpnInterface.fd
      openInterfaces[fd] = vpnInterface
      invoke.resolveObject(
        linkedMapOf(
          "fd" to fd,
          "interfaceName" to INTERFACE_NAME,
          "address" to OPENIPC_VPN_ADDRESS,
          "prefixLength" to OPENIPC_VPN_PREFIX,
        ),
      )
    } catch (error: Exception) {
      invoke.reject("Could not create OpenIPC VPN interface", error)
      stopSelf()
    }
  }

  private fun closeAll() {
    openInterfaces.values.forEach { descriptor ->
      try {
        descriptor.close()
      } catch (_: Exception) {
      }
    }
    openInterfaces.clear()
  }

  companion object {
    private const val ACTION_OPEN = "dev.openipc.usb.OPEN_VPN"
    private const val ACTION_CLOSE_ALL = "dev.openipc.usb.CLOSE_ALL_VPN"
    private const val INTERFACE_NAME = "OpenIPC VPN"
    private const val OPENIPC_VPN_ADDRESS = "10.5.0.3"
    private const val OPENIPC_VPN_ROUTE = "10.5.0.0"
    private const val OPENIPC_VPN_PREFIX = 24
    private const val OPENIPC_VPN_MTU = 1500

    private val lock = Any()
    private var pendingInvoke: Invoke? = null
    private val openInterfaces = ConcurrentHashMap<Int, ParcelFileDescriptor>()

    fun open(activity: Activity, invoke: Invoke) {
      synchronized(lock) {
        if (pendingInvoke != null) {
          invoke.reject("Another VPN request is already pending")
          return
        }
        pendingInvoke = invoke
      }

      try {
        activity.startService(Intent(activity, OpenIpcVpnService::class.java).setAction(ACTION_OPEN))
      } catch (error: Exception) {
        synchronized(lock) {
          if (pendingInvoke === invoke) {
            pendingInvoke = null
          }
        }
        invoke.reject("Could not start OpenIPC VPN service", error)
      }
    }

    fun close(fd: Int) {
      openInterfaces.remove(fd)?.let { descriptor ->
        try {
          descriptor.close()
        } catch (_: Exception) {
        }
      }
    }

    fun closeAll() {
      closeAllDescriptors()
    }

    private fun closeAllDescriptors() {
      openInterfaces.values.forEach { descriptor ->
        try {
          descriptor.close()
        } catch (_: Exception) {
        }
      }
      openInterfaces.clear()
    }
  }
}
