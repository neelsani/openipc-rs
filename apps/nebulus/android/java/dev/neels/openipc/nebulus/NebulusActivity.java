package dev.neels.openipc.nebulus;

import android.app.NativeActivity;
import android.content.Intent;
import android.database.Cursor;
import android.graphics.SurfaceTexture;
import android.net.Uri;
import android.net.VpnService;
import android.os.Build;
import android.os.Bundle;
import android.provider.OpenableColumns;
import android.view.Display;
import android.view.Surface;
import android.view.WindowManager;
import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.io.OutputStream;

public final class NebulusActivity extends NativeActivity {
    private static final int OPEN_KEY_REQUEST = 0x4753;
    private static final int OPEN_VPN_REQUEST = 0x5650;
    private static final int SAVE_SUPPORT_REQUEST = 0x5352;

    private byte[] pendingSupportBundle;

    private SurfaceTexture videoSurfaceTexture;
    private Surface videoSurface;
    private final float[] videoTextureTransform = new float[16];

    private static native void nativeKeySelected(String name, byte[] bytes);
    private static native void nativeKeyError(String message);
    static native void nativeVpnOpened(int fd, String interfaceName);
    static native void nativeVpnError(String message);

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        requestFastestDisplayMode();
    }

    @SuppressWarnings("deprecation")
    private void requestFastestDisplayMode() {
        Display display = getWindowManager().getDefaultDisplay();
        Display.Mode current = display.getMode();
        Display.Mode fastest = current;
        for (Display.Mode candidate : display.getSupportedModes()) {
            boolean sameSize = candidate.getPhysicalWidth() == current.getPhysicalWidth()
                && candidate.getPhysicalHeight() == current.getPhysicalHeight();
            if (sameSize && candidate.getRefreshRate() > fastest.getRefreshRate()) {
                fastest = candidate;
            }
        }
        WindowManager.LayoutParams attributes = getWindow().getAttributes();
        attributes.preferredDisplayModeId = fastest.getModeId();
        attributes.preferredRefreshRate = fastest.getRefreshRate();
        getWindow().setAttributes(attributes);
    }

    /** Distinguish the high-latency software emulator codec from real hardware. */
    public boolean isProbablyEmulator() {
        return Build.FINGERPRINT.startsWith("generic")
            || Build.FINGERPRINT.contains("emulator")
            || Build.MODEL.contains("Emulator")
            || Build.MODEL.contains("sdk_gphone")
            || Build.HARDWARE.contains("goldfish")
            || Build.HARDWARE.contains("ranchu");
    }

    /** Create the MediaCodec producer surface for a GL_TEXTURE_EXTERNAL_OES name. */
    public synchronized Surface createVideoSurface(int textureId) {
        releaseVideoSurface();
        videoSurfaceTexture = new SurfaceTexture(textureId);
        videoSurface = new Surface(videoSurfaceTexture);
        return videoSurface;
    }

    /** Latch the newest decoder image and return SurfaceTexture's UV transform. */
    public synchronized float[] updateVideoTexture() {
        if (videoSurfaceTexture != null) {
            videoSurfaceTexture.updateTexImage();
            videoSurfaceTexture.getTransformMatrix(videoTextureTransform);
        }
        return videoTextureTransform;
    }

    public synchronized void setVideoBufferSize(int width, int height) {
        if (videoSurfaceTexture != null && width > 0 && height > 0) {
            videoSurfaceTexture.setDefaultBufferSize(width, height);
        }
    }

    public synchronized void releaseVideoSurface() {
        if (videoSurface != null) {
            videoSurface.release();
            videoSurface = null;
        }
        if (videoSurfaceTexture != null) {
            videoSurfaceTexture.release();
            videoSurfaceTexture = null;
        }
    }

    public void openKeyFile() {
        runOnUiThread(this::openKeyFileOnUiThread);
    }

    private void openKeyFileOnUiThread() {
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
        intent.addCategory(Intent.CATEGORY_OPENABLE);
        intent.setType("application/octet-stream");
        intent.putExtra(Intent.EXTRA_MIME_TYPES, new String[] {
            "application/octet-stream",
            "text/plain",
            "*/*"
        });
        startActivityForResult(intent, OPEN_KEY_REQUEST);
    }

    public void openVpn() {
        runOnUiThread(() -> {
            Intent permission = VpnService.prepare(this);
            if (permission == null) {
                startVpnService();
            } else {
                startActivityForResult(permission, OPEN_VPN_REQUEST);
            }
        });
    }

    public void saveSupportBundle(String filename, byte[] bytes) {
        runOnUiThread(() -> {
            pendingSupportBundle = bytes;
            Intent intent = new Intent(Intent.ACTION_CREATE_DOCUMENT);
            intent.addCategory(Intent.CATEGORY_OPENABLE);
            intent.setType("application/zip");
            intent.putExtra(Intent.EXTRA_TITLE, filename);
            startActivityForResult(intent, SAVE_SUPPORT_REQUEST);
        });
    }

    public void closeVpn(int fd) {
        if (NebulusVpnService.close(fd)) {
            stopService(new Intent(this, NebulusVpnService.class));
        }
    }

    private void startVpnService() {
        try {
            startService(new Intent(this, NebulusVpnService.class)
                .setAction(NebulusVpnService.ACTION_OPEN));
        } catch (Exception error) {
            nativeVpnError("Could not start Android VPN service: " + error.getMessage());
        }
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (requestCode == OPEN_VPN_REQUEST) {
            if (resultCode == RESULT_OK) {
                startVpnService();
            } else {
                nativeVpnError("Android VPN permission was not granted");
            }
            return;
        }
        if (requestCode == SAVE_SUPPORT_REQUEST) {
            byte[] bytes = pendingSupportBundle;
            pendingSupportBundle = null;
            if (resultCode == RESULT_OK && data != null && data.getData() != null && bytes != null) {
                try (OutputStream output = getContentResolver().openOutputStream(data.getData())) {
                    if (output != null) {
                        output.write(bytes);
                        output.flush();
                    }
                } catch (Exception ignored) {
                    // Rust already records that the picker opened. Android owns any I/O error UI.
                }
            }
            return;
        }
        if (requestCode != OPEN_KEY_REQUEST || resultCode != RESULT_OK || data == null) {
            return;
        }
        Uri uri = data.getData();
        if (uri == null) {
            nativeKeyError("Android file picker returned no document");
            return;
        }
        try (InputStream input = getContentResolver().openInputStream(uri);
             ByteArrayOutputStream output = new ByteArrayOutputStream()) {
            if (input == null) {
                nativeKeyError("Android could not open the selected key");
                return;
            }
            byte[] buffer = new byte[4096];
            int count;
            while ((count = input.read(buffer)) != -1) {
                output.write(buffer, 0, count);
            }
            nativeKeySelected(displayName(uri), output.toByteArray());
        } catch (Exception error) {
            nativeKeyError("Could not read selected key: " + error.getMessage());
        }
    }

    private String displayName(Uri uri) {
        try (Cursor cursor = getContentResolver().query(
                uri, new String[] {OpenableColumns.DISPLAY_NAME}, null, null, null)) {
            if (cursor != null && cursor.moveToFirst()) {
                int column = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME);
                if (column >= 0) {
                    return cursor.getString(column);
                }
            }
        }
        String segment = uri.getLastPathSegment();
        return segment == null ? "gs.key" : segment;
    }
}
