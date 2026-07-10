package dev.neels.openipc.nebulus;

import android.app.NativeActivity;
import android.content.Intent;
import android.database.Cursor;
import android.graphics.SurfaceTexture;
import android.net.Uri;
import android.net.VpnService;
import android.os.Bundle;
import android.provider.OpenableColumns;
import android.view.Display;
import android.view.Surface;
import android.view.WindowManager;
import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.HttpURLConnection;
import java.net.URL;

public final class NebulusActivity extends NativeActivity {
    private static final int OPEN_KEY_REQUEST = 0x4753;
    private static final int OPEN_PRESET_REQUEST = 0x5052;
    private static final int OPEN_VPN_REQUEST = 0x5650;
    private static final int SAVE_DOCUMENT_REQUEST = 0x5352;

    private byte[] pendingDocument;

    private SurfaceTexture videoSurfaceTexture;
    private Surface videoSurface;
    private final float[] videoTextureTransform = new float[16];

    private static native void nativeKeySelected(String name, byte[] bytes);
    private static native void nativeKeyError(String message);
    private static native void nativePresetSelected(String name, byte[] bytes);
    private static native void nativePresetError(String message);
    private static native void nativeRemotePresetDownloaded(String finalUrl, byte[] bytes);
    private static native void nativeRemotePresetError(String message);
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

    public void openPresetFile() {
        runOnUiThread(() -> {
            Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
            intent.addCategory(Intent.CATEGORY_OPENABLE);
            intent.setType("application/json");
            intent.putExtra(Intent.EXTRA_MIME_TYPES, new String[] {
                "application/json",
                "text/json",
                "text/plain",
                "*/*"
            });
            startActivityForResult(intent, OPEN_PRESET_REQUEST);
        });
    }

    public void downloadPresetUrl(String url) {
        new Thread(() -> {
            HttpURLConnection connection = null;
            try {
                URL current = new URL(url);
                for (int redirects = 0; redirects <= 5; redirects++) {
                    if (!isAllowedPresetUrl(current)) {
                        throw new IllegalArgumentException(
                            "Preset redirects require HTTPS; HTTP is allowed only on loopback");
                    }
                    connection = (HttpURLConnection) current.openConnection();
                    connection.setInstanceFollowRedirects(false);
                    connection.setConnectTimeout(10_000);
                    connection.setReadTimeout(30_000);
                    connection.setRequestProperty("Accept", "application/json");
                    connection.setRequestProperty("User-Agent", "Nebulus/Android");
                    int status = connection.getResponseCode();
                    if (status >= 300 && status < 400) {
                        String location = connection.getHeaderField("Location");
                        connection.disconnect();
                        connection = null;
                        if (location == null) {
                            throw new IllegalStateException("Preset redirect has no Location header");
                        }
                        current = new URL(current, location);
                        continue;
                    }
                    if (status < 200 || status >= 300) {
                        throw new IllegalStateException("Preset request returned HTTP " + status);
                    }
                    long length = connection.getContentLengthLong();
                    if (length > 512 * 1024) {
                        throw new IllegalArgumentException("Remote preset document is too large");
                    }
                    try (InputStream input = connection.getInputStream()) {
                        nativeRemotePresetDownloaded(
                            connection.getURL().toString(), readStream(input, 512 * 1024));
                    }
                    return;
                }
                throw new IllegalStateException("Preset request exceeded five redirects");
            } catch (Exception error) {
                nativeRemotePresetError("Remote preset request failed: " + error.getMessage());
            } finally {
                if (connection != null) {
                    connection.disconnect();
                }
            }
        }, "nebulus-preset-download").start();
    }

    /** Download a debug codec fixture on the calling Rust worker thread. */
    public byte[] downloadMockFixture(String url, int maximumBytes) throws Exception {
        URL current = new URL(url);
        HttpURLConnection connection = null;
        try {
            for (int redirects = 0; redirects <= 5; redirects++) {
                if (!"https".equalsIgnoreCase(current.getProtocol())) {
                    throw new IllegalArgumentException("Codec fixture downloads require HTTPS");
                }
                connection = (HttpURLConnection) current.openConnection();
                connection.setInstanceFollowRedirects(false);
                connection.setConnectTimeout(10_000);
                connection.setReadTimeout(60_000);
                connection.setRequestProperty("Accept", "application/octet-stream");
                connection.setRequestProperty("User-Agent", "Nebulus/Android codec fixture loader");
                int status = connection.getResponseCode();
                if (status >= 300 && status < 400) {
                    String location = connection.getHeaderField("Location");
                    connection.disconnect();
                    connection = null;
                    if (location == null) {
                        throw new IllegalStateException("Codec fixture redirect has no Location header");
                    }
                    current = new URL(current, location);
                    continue;
                }
                if (status < 200 || status >= 300) {
                    throw new IllegalStateException("Codec fixture request returned HTTP " + status);
                }
                long length = connection.getContentLengthLong();
                if (length > maximumBytes) {
                    throw new IllegalArgumentException("Codec fixture is too large");
                }
                try (InputStream input = connection.getInputStream()) {
                    return readStream(input, maximumBytes);
                }
            }
            throw new IllegalStateException("Codec fixture request exceeded five redirects");
        } finally {
            if (connection != null) {
                connection.disconnect();
            }
        }
    }

    private static boolean isAllowedPresetUrl(URL url) {
        if ("https".equalsIgnoreCase(url.getProtocol())) {
            return true;
        }
        if (!"http".equalsIgnoreCase(url.getProtocol())) {
            return false;
        }
        String host = url.getHost();
        return "localhost".equalsIgnoreCase(host)
            || "127.0.0.1".equals(host)
            || "::1".equals(host)
            || "[::1]".equals(host);
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

    public void saveDocument(String filename, String mimeType, byte[] bytes) {
        runOnUiThread(() -> {
            pendingDocument = bytes;
            Intent intent = new Intent(Intent.ACTION_CREATE_DOCUMENT);
            intent.addCategory(Intent.CATEGORY_OPENABLE);
            intent.setType(mimeType);
            intent.putExtra(Intent.EXTRA_TITLE, filename);
            startActivityForResult(intent, SAVE_DOCUMENT_REQUEST);
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
        if (requestCode == SAVE_DOCUMENT_REQUEST) {
            byte[] bytes = pendingDocument;
            pendingDocument = null;
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
        if (requestCode == OPEN_PRESET_REQUEST) {
            if (resultCode != RESULT_OK || data == null || data.getData() == null) {
                return;
            }
            Uri uri = data.getData();
            try {
                nativePresetSelected(displayName(uri), readDocument(uri, 512 * 1024));
            } catch (Exception error) {
                nativePresetError("Could not read preset: " + error.getMessage());
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
        try {
            nativeKeySelected(displayName(uri), readDocument(uri, 1024 * 1024));
        } catch (Exception error) {
            nativeKeyError("Could not read selected key: " + error.getMessage());
        }
    }

    private byte[] readDocument(Uri uri, int maximumBytes) throws Exception {
        try (InputStream input = getContentResolver().openInputStream(uri)) {
            if (input == null) {
                throw new IllegalStateException("Android could not open the selected document");
            }
            return readStream(input, maximumBytes);
        }
    }

    private static byte[] readStream(InputStream input, int maximumBytes) throws Exception {
        try (ByteArrayOutputStream output = new ByteArrayOutputStream()) {
            byte[] buffer = new byte[4096];
            int count;
            while ((count = input.read(buffer)) != -1) {
                if (output.size() + count > maximumBytes) {
                    throw new IllegalArgumentException("selected document is too large");
                }
                output.write(buffer, 0, count);
            }
            return output.toByteArray();
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
