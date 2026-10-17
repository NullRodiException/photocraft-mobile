package dev.photocraftmobile.app;

import android.app.ActivityManager;
import android.content.ActivityNotFoundException;
import android.content.ClipData;
import android.content.Context;
import android.content.Intent;
import android.database.Cursor;
import android.net.Uri;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.os.SystemClock;
import android.provider.DocumentsContract;
import android.provider.OpenableColumns;
import android.system.ErrnoException;
import android.system.Os;
import android.view.MotionEvent;
import android.view.View;
import android.webkit.MimeTypeMap;
import android.widget.Toast;

import androidx.core.view.WindowCompat;
import androidx.core.view.WindowInsetsCompat;
import androidx.core.view.WindowInsetsControllerCompat;

import com.google.androidgamesdk.GameActivity;
import com.google.androidgamesdk.gametextinput.State;

import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

/**
 * GameActivity running the Rust library (libphotocraft_mobile.so), plus what a drawing app on a
 * pen tablet needs and the Rust side cannot do through winit:
 *
 * <ul>
 *   <li><b>Immersive full screen.</b> The status bar, the navigation bar and the One UI taskbar
 *       are hidden, so they never cover the editor. A swipe from the edge shows them for a moment.
 *   <li><b>Palm rejection</b> ({@link PalmFilter}), contact by contact. While a stylus (S Pen)
 *       touches or hovers over the screen, and for a moment after it leaves, contacts that start
 *       with a finger or palm are dropped before they reach the app, so a hand resting on the
 *       screen doesn't paint. When the pen touches down, the fingers the app had are cancelled.
 *       With the pen away, fingers work as usual.
 *   <li><b>Tool type.</b> Each new contact's tool (finger or pen) goes to the Rust side, which
 *       turns fingers on the canvas into navigation (pan, pinch, two/three-finger tap).
 *   <li><b>The pen itself.</b> Every stylus event (touching or hovering) goes to the Rust side
 *       with its pressure, tilt, orientation and buttons: tilt for the brushes, the side button as
 *       the eraser, and the hover position for the brush outline.
 *   <li><b>Files.</b> Open and Save use the system file picker (Storage Access Framework). The
 *       Rust side queues requests that this activity polls. Picked documents are copied into the
 *       app cache to open, and saves are written there and copied to the picked document. Each
 *       copy stays linked to its document (with the permission kept across restarts), so saving
 *       an opened document writes it back.
 * </ul>
 */
public class MainActivity extends GameActivity {
    /** How often the Rust side's file requests are checked (ms). */
    private static final long POLL_MS = 150;
    /** How long a pause waits for the Rust side to take the autosave request (ms). */
    private static final long BACKGROUND_WAIT_MS = 250;
    private static final int GRANT_READ_WRITE = Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION;
    private static final int REQUEST_OPEN = 7101;
    private static final int REQUEST_SAVE = 7102;
    /** The device's RAM in MB, for PhotoCraft's GPU memory budgets (see {@link #tellRam}). */
    private static final String RAM_ENV = "PHOTOCRAFT_RAM_MB";
    /** The app's cache folder, for the Rust side's clean-up (see {@link #tellCacheDir}). */
    private static final String CACHE_ENV = "PHOTOCRAFT_MOBILE_CACHE";
    /** The build's versionName, which crash reports name. */
    private static final String VERSION_ENV = "PHOTOCRAFT_MOBILE_VERSION";
    /** The most of a crash report the share sheet gets (the Rust side keeps 64 KB). */
    private static final int SHARE_MAX_BYTES = 64 * 1024;

    private final PalmFilter palm = new PalmFilter();

    private final Handler handler = new Handler(Looper.getMainLooper());
    private final ExecutorService io = Executors.newSingleThreadExecutor();
    private long openRequest = -1;
    private long saveRequest = -1;
    private String saveName = "";

    private final Runnable poll = new Runnable() {
        @Override
        public void run() {
            pollRequests();
            handler.postDelayed(this, POLL_MS);
        }
    };

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        // Before super.onCreate, which loads the Rust library and starts it.
        tellRam();
        tellCacheDir();
        tellVersion();
        super.onCreate(savedInstanceState);
        hideSystemBars();
    }

    /**
     * PhotoCraft sizes its GPU memory budgets from the device's RAM (a document's display textures
     * may take a quarter of it), but it can't read the RAM on Android and assumes 16 GB there: on a
     * tablet with 6 or 8 GB, where the GPU shares that RAM, a large image could take far more than
     * the system can spare. It reads the real amount from PHOTOCRAFT_RAM_MB, set here once; a value
     * already set is kept.
     */
    private void tellRam() {
        if (Os.getenv(RAM_ENV) != null) {
            return;
        }
        ActivityManager am = (ActivityManager) getSystemService(Context.ACTIVITY_SERVICE);
        if (am == null) {
            return;
        }
        ActivityManager.MemoryInfo info = new ActivityManager.MemoryInfo();
        am.getMemoryInfo(info);
        long mb = info.totalMem / (1024 * 1024);
        if (mb <= 0) {
            return;
        }
        try {
            Os.setenv(RAM_ENV, Long.toString(mb), false);
        } catch (ErrnoException e) {
            // PhotoCraft keeps its own assumption.
        }
    }

    /**
     * The Rust side cleans up the picked documents' copies in the app cache at start-up
     * (files.rs). android-activity only gives it the files and OBB folders, so the cache folder
     * is passed here.
     */
    private void tellCacheDir() {
        try {
            Os.setenv(CACHE_ENV, getCacheDir().getAbsolutePath(), true);
        } catch (ErrnoException e) {
            // The copies stay; nothing else depends on it.
        }
    }

    /** The build's versionName (0.1.n), which the Rust side writes into crash reports. */
    private void tellVersion() {
        try {
            String name = getPackageManager().getPackageInfo(getPackageName(), 0).versionName;
            if (name != null) {
                Os.setenv(VERSION_ENV, name, true);
            }
        } catch (android.content.pm.PackageManager.NameNotFoundException | ErrnoException e) {
            // The report names the library's version instead.
        }
    }

    @Override
    protected void onResume() {
        super.onResume();
        handler.removeCallbacks(poll);
        handler.post(poll);
    }

    @Override
    protected void onPause() {
        handler.removeCallbacks(poll);
        autosaveBeforePause();
        super.onPause();
    }

    /**
     * Android may end the app once it is in the background, so the Rust side autosaves the
     * documents with unsaved changes for crash recovery (the write itself goes on in the
     * background). That happens in a frame, and once GameActivity has paused, the Rust side draws
     * no more frames: wait, briefly, for a frame to take the request.
     */
    private void autosaveBeforePause() {
        try {
            nativeBackground();
            long until = SystemClock.uptimeMillis() + BACKGROUND_WAIT_MS;
            while (!nativeBackgroundHandled() && SystemClock.uptimeMillis() < until) {
                SystemClock.sleep(5);
            }
        } catch (UnsatisfiedLinkError e) {
            // The library isn't loaded: there is nothing to save.
        }
    }

    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        if (hasFocus) {
            hideSystemBars();
        }
    }

    private void hideSystemBars() {
        WindowCompat.setDecorFitsSystemWindows(getWindow(), false);
        View decor = getWindow().getDecorView();
        WindowInsetsControllerCompat controller = WindowCompat.getInsetsController(getWindow(), decor);
        controller.setSystemBarsBehavior(WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE);
        controller.hide(WindowInsetsCompat.Type.systemBars());
    }

    // ---- On-screen keyboard -----------------------------------------------------------------

    /**
     * The keyboard edited GameActivity's text field. winit drops these edits, so the Rust side reads
     * the field itself (ime.rs): this only asks it for a frame.
     */
    @Override
    public void stateChanged(State newState, boolean dismissed) {
        super.stateChanged(newState, dismissed);
        try {
            nativeImeChanged();
        } catch (UnsatisfiedLinkError e) {
            // Not loaded yet: nothing has a text field.
        }
    }

    /** The keyboard's action key (Done, Go, Search…), which winit also drops: Enter on the Rust side. */
    @Override
    public void onEditorAction(int action) {
        super.onEditorAction(action);
        try {
            nativeImeAction();
        } catch (UnsatisfiedLinkError e) {
            // Not loaded yet.
        }
    }

    // ---- Input ----------------------------------------------------------------------------

    @Override
    protected boolean processMotionEvent(MotionEvent event) {
        int n = event.getPointerCount();
        int[] ids = new int[n];
        int[] tools = new int[n];
        float[] majorsMm = new float[n];
        float pixelsPerMm = getResources().getDisplayMetrics().xdpi / 25.4f;
        for (int i = 0; i < n; i++) {
            ids[i] = event.getPointerId(i);
            tools[i] = event.getToolType(i);
            majorsMm[i] = pixelsPerMm > 0 ? event.getTouchMajor(i) / pixelsPerMm : 0;
        }
        // Android 13+ marks a contact it recognised as a palm when it lifts.
        boolean canceled = (event.getFlags() & MotionEvent.FLAG_CANCELED) != 0;
        PalmFilter.Decision decision =
                palm.onEvent(event.getActionMasked(), event.getActionIndex(), ids, tools, majorsMm, event.getEventTime(), canceled);
        if (event.getActionMasked() == MotionEvent.ACTION_DOWN && PalmFilter.isPen(tools[event.getActionIndex()]) && mSurfaceView != null) {
            // The pen's samples arrive as they happen, not batched until the next frame: the stroke
            // follows the pen sooner (each sample still reaches the app, see pen.rs).
            mSurfaceView.requestUnbufferedDispatch(event);
        }
        // Every pen on the screen, whatever contact the event is about.
        reportPen(event, tools);
        if (decision.cancel.length > 0) {
            // Fingers the app had lose their contact first, so the pen gets egui's pointer.
            MotionEvent cancel = subset(event, decision.cancel, MotionEvent.ACTION_CANCEL);
            super.processMotionEvent(cancel);
            cancel.recycle();
        }
        if (decision.keep == null) {
            reportTool(event);
            return super.processMotionEvent(event);
        }
        if (decision.dropped()) {
            return true;
        }
        MotionEvent kept = subset(event, decision.keep, decision.action);
        reportTool(kept);
        boolean handled = super.processMotionEvent(kept);
        kept.recycle();
        return handled;
    }

    /** {@code event} with only the contacts at {@code indices}, and {@code action}. */
    private static MotionEvent subset(MotionEvent event, int[] indices, int action) {
        int n = indices.length;
        MotionEvent.PointerProperties[] properties = new MotionEvent.PointerProperties[n];
        MotionEvent.PointerCoords[] coords = new MotionEvent.PointerCoords[n];
        for (int j = 0; j < n; j++) {
            properties[j] = new MotionEvent.PointerProperties();
            event.getPointerProperties(indices[j], properties[j]);
            coords[j] = new MotionEvent.PointerCoords();
            event.getPointerCoords(indices[j], coords[j]);
        }
        return MotionEvent.obtain(event.getDownTime(), event.getEventTime(), action, n, properties, coords, event.getMetaState(),
                event.getButtonState(), event.getXPrecision(), event.getYPrecision(), event.getDeviceId(), event.getEdgeFlags(),
                event.getSource(), event.getFlags());
    }

    /**
     * Tells the Rust side what a new contact is (winit drops the tool type), before the event
     * reaches it: fingers on the canvas then pan, pinch and tap undo/redo instead of painting.
     */
    private static void reportTool(MotionEvent event) {
        int action = event.getActionMasked();
        if (action != MotionEvent.ACTION_DOWN && action != MotionEvent.ACTION_POINTER_DOWN) {
            return;
        }
        int index = event.getActionIndex();
        try {
            nativeTouchTool(event.getPointerId(index), event.getToolType(index));
        } catch (UnsatisfiedLinkError e) {
            // The library isn't loaded yet: the contact reaches the app as a plain touch.
        }
    }

    /**
     * Sends the pressure, tilt, orientation and buttons of every pen in the event (winit drops all
     * but pressure). A pen that isn't the contact the event is about is still touching: it moved.
     * A move also sends the samples Android batched into the event (its history) first, oldest
     * first: winit keeps only the event's last position (see pen.rs).
     */
    private static void reportPen(MotionEvent event, int[] tools) {
        int masked = event.getActionMasked();
        int actor = event.getActionIndex();
        int buttons = event.getButtonState();
        for (int i = 0; i < tools.length; i++) {
            if (!PalmFilter.isPen(tools[i])) {
                continue;
            }
            int action = i == actor || masked == MotionEvent.ACTION_CANCEL ? masked : MotionEvent.ACTION_MOVE;
            int id = event.getPointerId(i);
            try {
                if (action == MotionEvent.ACTION_MOVE) {
                    for (int h = 0; h < event.getHistorySize(); h++) {
                        nativePenSample(id, action, 1, event.getHistoricalX(i, h), event.getHistoricalY(i, h), event.getHistoricalPressure(i, h),
                                event.getHistoricalAxisValue(MotionEvent.AXIS_TILT, i, h), event.getHistoricalOrientation(i, h), buttons, tools[i]);
                    }
                }
                nativePenSample(id, action, 0, event.getX(i), event.getY(i), event.getPressure(i), event.getAxisValue(MotionEvent.AXIS_TILT, i),
                        event.getOrientation(i), buttons, tools[i]);
            } catch (UnsatisfiedLinkError e) {
                // Not loaded yet: the pen paints with winit's pressure only.
                return;
            }
        }
    }

    // ---- Files ----------------------------------------------------------------------------

    /** Handles the Rust side's queued file requests (see apps/android/src/files.rs). */
    private void pollRequests() {
        for (int n = 0; n < 8; n++) {
            String request;
            try {
                request = nativeNextRequest();
            } catch (UnsatisfiedLinkError e) {
                return;
            }
            if (request == null) {
                return;
            }
            String[] f = request.split("\t", -1);
            try {
                switch (f[0]) {
                    case "open":
                        openDocuments(Long.parseLong(f[1]), "1".equals(f[2]));
                        break;
                    case "save":
                        createDocument(Long.parseLong(f[1]), f[2]);
                        break;
                    case "export":
                        export(f[1], f[2]);
                        break;
                    case "release":
                        release(f[1]);
                        break;
                    case "share":
                        shareText(f[1]);
                        break;
                    default:
                        break;
                }
            } catch (RuntimeException e) {
                // A malformed line: skip it rather than stop the app.
            }
        }
    }

    private void openDocuments(long id, boolean multiple) {
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
        intent.addCategory(Intent.CATEGORY_OPENABLE);
        intent.setType("*/*");
        intent.putExtra(Intent.EXTRA_ALLOW_MULTIPLE, multiple);
        // Saving an opened document writes it back, also after a restart.
        intent.addFlags(GRANT_READ_WRITE | Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION);
        openRequest = id;
        try {
            startActivityForResult(intent, REQUEST_OPEN);
        } catch (ActivityNotFoundException e) {
            openRequest = -1;
            nativeOpenResult(id, "");
        }
    }

    private void createDocument(long id, String name) {
        Intent intent = new Intent(Intent.ACTION_CREATE_DOCUMENT);
        intent.addCategory(Intent.CATEGORY_OPENABLE);
        intent.setType(mimeFor(name));
        intent.putExtra(Intent.EXTRA_TITLE, name);
        saveRequest = id;
        saveName = name;
        try {
            startActivityForResult(intent, REQUEST_SAVE);
        } catch (ActivityNotFoundException e) {
            saveRequest = -1;
            nativeSaveResult(id, "");
        }
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        boolean ok = resultCode == RESULT_OK && data != null;
        int grant = ok ? data.getFlags() & GRANT_READ_WRITE : 0;
        if (requestCode == REQUEST_OPEN && openRequest >= 0) {
            long id = openRequest;
            openRequest = -1;
            List<Uri> uris = new ArrayList<>();
            if (ok) {
                ClipData clip = data.getClipData();
                if (clip != null) {
                    for (int i = 0; i < clip.getItemCount(); i++) {
                        uris.add(clip.getItemAt(i).getUri());
                    }
                } else if (data.getData() != null) {
                    uris.add(data.getData());
                }
            }
            // Copying a large photo takes a moment: off the UI thread.
            io.execute(() -> {
                StringBuilder paths = new StringBuilder();
                File dir = new File(getCacheDir(), "saf-open/" + id);
                for (Uri uri : uris) {
                    File target = new File(dir, safeName(displayName(uri)));
                    if (copy(uri, target)) {
                        // Linked before PhotoCraft opens it, so its first Save already goes back.
                        boolean writable = keepAccess(uri, grant) && supportsWrite(uri);
                        nativeLink(target.getAbsolutePath(), uri.toString(), writable ? 1 : 0);
                        paths.append(target.getAbsolutePath()).append('\n');
                    }
                }
                nativeOpenResult(id, paths.toString());
            });
        } else if (requestCode == REQUEST_SAVE && saveRequest >= 0) {
            long id = saveRequest;
            saveRequest = -1;
            Uri uri = ok ? data.getData() : null;
            if (uri == null) {
                nativeSaveResult(id, "");
                return;
            }
            File dir = new File(getCacheDir(), "saf-save/" + id);
            //noinspection ResultOfMethodCallIgnored
            dir.mkdirs();
            File target = new File(dir, safeName(saveName));
            // The picker created this document for us: the app may write it.
            keepAccess(uri, grant | GRANT_READ_WRITE);
            nativeLink(target.getAbsolutePath(), uri.toString(), 1);
            nativeSaveResult(id, target.getAbsolutePath());
        }
    }

    /**
     * Keeps the permission the picker granted to {@code uri} across restarts, so a later save still
     * reaches the document. Returns whether that permission includes writing.
     */
    private boolean keepAccess(Uri uri, int grant) {
        int modes = grant & GRANT_READ_WRITE;
        try {
            getContentResolver().takePersistableUriPermission(uri, modes);
            return (modes & Intent.FLAG_GRANT_WRITE_URI_PERMISSION) != 0;
        } catch (SecurityException e) {
            // Not persistable with writing: keep reading at least.
        }
        try {
            getContentResolver().takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION);
        } catch (SecurityException e) {
            // Only for this session.
        }
        return (modes & Intent.FLAG_GRANT_WRITE_URI_PERMISSION) != 0;
    }

    /** Offers a text file of the app's (a crash report) to the share sheet, as text. */
    private void shareText(String path) {
        io.execute(() -> {
            String text;
            try (InputStream in = new FileInputStream(path)) {
                byte[] bytes = new byte[SHARE_MAX_BYTES];
                int n = 0;
                int read;
                while (n < bytes.length && (read = in.read(bytes, n, bytes.length - n)) > 0) {
                    n += read;
                }
                text = new String(bytes, 0, n, java.nio.charset.StandardCharsets.UTF_8);
            } catch (IOException e) {
                return;
            }
            Intent send = new Intent(Intent.ACTION_SEND);
            send.setType("text/plain");
            send.putExtra(Intent.EXTRA_SUBJECT, text("PhotoCraft Mobile crash report", "Relatório de falha do PhotoCraft Mobile"));
            send.putExtra(Intent.EXTRA_TEXT, text);
            runOnUiThread(() -> {
                try {
                    startActivity(Intent.createChooser(send, text("Send crash report", "Enviar relatório de falha")));
                } catch (ActivityNotFoundException e) {
                    Toast.makeText(this, text("No app can receive it", "Nenhum app pode recebê-lo"), Toast.LENGTH_SHORT).show();
                }
            });
        });
    }

    /** Gives back the kept permission to a document no cache copy stands for any more. */
    private void release(String uriText) {
        Uri uri = Uri.parse(uriText);
        for (int modes : new int[] {GRANT_READ_WRITE, Intent.FLAG_GRANT_READ_URI_PERMISSION}) {
            try {
                getContentResolver().releasePersistableUriPermission(uri, modes);
                return;
            } catch (SecurityException e) {
                // Not held with these modes: try reading only.
            }
        }
    }

    /** Whether the document's provider accepts writes to it (true when it doesn't say). */
    private boolean supportsWrite(Uri uri) {
        try (Cursor c = getContentResolver().query(uri, new String[] {DocumentsContract.Document.COLUMN_FLAGS}, null, null, null)) {
            if (c != null && c.moveToFirst() && !c.isNull(0)) {
                return (c.getInt(0) & DocumentsContract.Document.FLAG_SUPPORTS_WRITE) != 0;
            }
        } catch (RuntimeException e) {
            // Not a DocumentsProvider document: the grant decides.
        }
        return true;
    }

    /** After the Rust side wrote a picked document's cache copy: copy it to the document. */
    private void export(String path, String uriText) {
        Uri uri = Uri.parse(uriText);
        String name = new File(path).getName();
        io.execute(() -> {
            boolean ok;
            try (InputStream in = new FileInputStream(path);
                 OutputStream out = getContentResolver().openOutputStream(uri, "wt")) {
                ok = out != null && pipe(in, out);
            } catch (IOException | SecurityException e) {
                ok = false;
            }
            boolean saved = ok;
            runOnUiThread(() -> Toast.makeText(this, saved ? text("Saved: ", "Salvo: ") + name : text("Couldn't save ", "Não foi possível salvar ") + name, Toast.LENGTH_SHORT).show());
        });
    }

    private boolean copy(Uri uri, File target) {
        File dir = target.getParentFile();
        if (dir != null) {
            //noinspection ResultOfMethodCallIgnored
            dir.mkdirs();
        }
        try (InputStream in = getContentResolver().openInputStream(uri); OutputStream out = new FileOutputStream(target)) {
            return in != null && pipe(in, out);
        } catch (IOException | SecurityException e) {
            return false;
        }
    }

    private static boolean pipe(InputStream in, OutputStream out) throws IOException {
        byte[] buffer = new byte[1 << 16];
        int n;
        while ((n = in.read(buffer)) > 0) {
            out.write(buffer, 0, n);
        }
        out.flush();
        return true;
    }

    private String displayName(Uri uri) {
        try (Cursor c = getContentResolver().query(uri, new String[] {OpenableColumns.DISPLAY_NAME}, null, null, null)) {
            if (c != null && c.moveToFirst()) {
                String name = c.getString(0);
                if (name != null && !name.isEmpty()) {
                    return name;
                }
            }
        } catch (RuntimeException e) {
            // Fall through to the URI's last segment.
        }
        String last = uri.getLastPathSegment();
        return last != null ? last : "document";
    }

    private static String safeName(String name) {
        String s = name.replaceAll("[/\\\\\\t\\r\\n:]", "_").trim();
        return s.isEmpty() ? "document" : s;
    }

    private static String mimeFor(String name) {
        int dot = name.lastIndexOf('.');
        String ext = dot >= 0 ? name.substring(dot + 1).toLowerCase(Locale.ROOT) : "";
        String mime = MimeTypeMap.getSingleton().getMimeTypeFromExtension(ext);
        return mime != null ? mime : "application/octet-stream";
    }

    private static String text(String en, String pt) {
        return "pt".equals(Locale.getDefault().getLanguage()) ? pt : en;
    }

    // ---- Rust (apps/android/src) ----------------------------------------------------------

    /** touch_tools.rs */
    private static native void nativeTouchTool(int pointerId, int toolType);

    /** ime.rs: the keyboard edited the text field. */
    private static native void nativeImeChanged();

    /** ime.rs: the keyboard's action key. */
    private static native void nativeImeAction();

    /** lifecycle.rs: the app is going to the background. */
    private static native void nativeBackground();

    /** lifecycle.rs: a frame took the last {@link #nativeBackground}. */
    private static native boolean nativeBackgroundHandled();

    /** pen.rs: {@code historical} isn't 0 for a sample from the event's history. */
    private static native void nativePenSample(int pointerId, int action, int historical, float x, float y, float pressure, float tilt,
            float orientation, int buttons, int toolType);

    /** files.rs: the next request line, or null. */
    private static native String nativeNextRequest();

    /** files.rs: one path per line; empty when cancelled. */
    private static native void nativeOpenResult(long id, String paths);

    /** files.rs: where to write the save; empty when cancelled. */
    private static native void nativeSaveResult(long id, String path);

    /** files.rs: the cache file {@code path} stands for the picked document {@code uri}. */
    private static native void nativeLink(String path, String uri, int writable);
}
