package dev.neels.openipc.nebulus;

import android.content.Intent;
import android.net.VpnService;
import android.os.ParcelFileDescriptor;
import java.util.concurrent.ConcurrentHashMap;

public final class NebulusVpnService extends VpnService {
    static final String ACTION_OPEN = "dev.neels.openipc.nebulus.OPEN_VPN";
    private static final ConcurrentHashMap<Integer, ParcelFileDescriptor> OPEN =
        new ConcurrentHashMap<>();

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        if (intent != null && ACTION_OPEN.equals(intent.getAction())) {
            establishTunnel();
        }
        return START_NOT_STICKY;
    }

    private void establishTunnel() {
        try {
            ParcelFileDescriptor descriptor = new Builder()
                .setSession("OpenIPC VPN")
                .addAddress("10.5.0.3", 24)
                .addRoute("10.5.0.0", 24)
                .setMtu(1500)
                .establish();
            if (descriptor == null) {
                NebulusActivity.nativeVpnError("Android did not create the OpenIPC VPN interface");
                stopSelf();
                return;
            }
            int fd = descriptor.getFd();
            OPEN.put(fd, descriptor);
            NebulusActivity.nativeVpnOpened(fd, "OpenIPC VPN");
        } catch (Exception error) {
            NebulusActivity.nativeVpnError(
                "Could not create OpenIPC VPN interface: " + error.getMessage());
            stopSelf();
        }
    }

    static boolean close(int fd) {
        ParcelFileDescriptor descriptor = OPEN.remove(fd);
        if (descriptor != null) {
            try {
                descriptor.close();
            } catch (Exception ignored) {
            }
        }
        return OPEN.isEmpty();
    }

    @Override
    public void onDestroy() {
        for (ParcelFileDescriptor descriptor : OPEN.values()) {
            try {
                descriptor.close();
            } catch (Exception ignored) {
            }
        }
        OPEN.clear();
        super.onDestroy();
    }
}
