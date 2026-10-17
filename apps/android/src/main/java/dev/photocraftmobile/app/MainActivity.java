package dev.photocraftmobile.app;

import android.app.ActivityManager;
import android.content.ActivityNotFoundException;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.ContentResolver;
import android.content.ContentValues;
import android.content.Context;
import android.content.Intent;
import android.database.Cursor;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.Environment;
import android.os.Handler;
import android.os.Looper;
import android.os.Parcelable;
import android.os.SystemClock;
import android.provider.DocumentsContract;
import android.provider.MediaStore;
import android.provider.OpenableColumns;
import android.system.ErrnoException;
import android.system.Os;
import android.view.MotionEvent;
import android.view.View;
import android.webkit.MimeTypeMap;
import android.widget.Toast;

import androidx.annotation.Keep;
import androidx.core.content.FileProvider;
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
 *   <li><b>Other apps.</b> "Open with" and "Share" from another app open the documents the same
 *       way, and the Rust side's exported images go to the share sheet or the photo gallery.
 *       Links from the app open in the browser.
 *   <li><b>The clipboard.</b> Images copied in the app go on the clipboard as files other apps
 *       paste, and images copied in other apps paste in the app. The Rust side calls these
 *       directly, from its own threads.
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
    /** The gallery folder images are saved to (under Pictures). */
    private static final String GALLERY_FOLDER = "PhotoCraft Mobile";
    /** The cache folder copies and pastes go through (res/xml/file_paths.xml names it too). */
    private static final String CLIPBOARD_DIR = "clipboard";
    /** The name an image copied in another app is pasted from (with its type's extension). */
    private static final String PASTE_NAME = "paste";

    /** The application, for the clipboard calls the Rust side makes from its own threads. */
    private static volatile Context appContext;

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
        appContext = getApplicationContext();
        super.onCreate(savedInstanceState);
        try {
            nativeClipboardInit();
        } catch (UnsatisfiedLinkError e) {
            // The library isn't loaded: nothing will copy or paste.
        }
        hideSystemBars();
        // A recreated activity already took its intent.
        if (savedInstanceState == null) {
            openIncoming(getIntent());
        }
    }

    /** The app is running (singleTask) and another app asks it to open something. */
    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        openIncoming(intent);
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
                    case "restore":
                        restore(f[1], f[2]);
                        break;
                    case "release":
                        release(f[1]);
                        break;
                    case "share":
                        shareText(f[1]);
                        break;
                    case "image":
                        if ("gallery".equals(f[1])) {
                            saveToGallery(new File(f[2]));
                        } else {
                            sendImage(new File(f[2]));
                        }
                        break;
                    case "url":
                        openUrl(f[1]);
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
            io.execute(() -> nativeOpenResult(id, copyIn(uris, grant, id)));
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
     * Copies the documents into the cache folder {@code saf-open/<id>} and links each copy to its
     * document; returns the copies' paths, one per line. Off the UI thread.
     */
    private String copyIn(List<Uri> uris, int grant, long id) {
        StringBuilder paths = new StringBuilder();
        File dir = new File(getCacheDir(), "saf-open/" + id);
        for (Uri uri : uris) {
            File target = unusedFile(dir, safeName(displayName(uri)));
            if (copy(uri, target)) {
                // Linked before PhotoCraft opens it, so its first Save already goes back.
                boolean writable = keepAccess(uri, grant) && supportsWrite(uri);
                nativeLink(target.getAbsolutePath(), uri.toString(), writable ? 1 : 0);
                paths.append(target.getAbsolutePath()).append('\n');
            }
        }
        return paths.toString();
    }

    /** {@code dir/name}, or {@code dir/name (2)}… when two documents share a name. */
    private static File unusedFile(File dir, String name) {
        File file = new File(dir, name);
        int dot = name.lastIndexOf('.');
        String stem = dot > 0 ? name.substring(0, dot) : name;
        String ext = dot > 0 ? name.substring(dot) : "";
        for (int n = 2; file.exists(); n++) {
            file = new File(dir, stem + " (" + n + ")" + ext);
        }
        return file;
    }

    /**
     * "Open with" and "Share" from another app: the documents are copied into the cache like the
     * picker's, and the Rust side opens them (files.rs). Another app's grant usually ends with
     * this activity, so a copy is writable only when the permission could be kept.
     */
    private void openIncoming(Intent intent) {
        if (intent == null || (intent.getFlags() & Intent.FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY) != 0) {
            return;
        }
        List<Uri> uris = incomingUris(intent);
        if (uris.isEmpty()) {
            return;
        }
        int grant = intent.getFlags() & GRANT_READ_WRITE;
        io.execute(() -> {
            try {
                String paths = copyIn(uris, grant, nativeNextId());
                if (paths.isEmpty()) {
                    toast(text("Couldn't open the file", "Não foi possível abrir o arquivo"));
                } else {
                    nativeOpenFiles(paths);
                }
            } catch (UnsatisfiedLinkError e) {
                // The library isn't loaded: the app couldn't open anything anyway.
            }
        });
    }

    /** The documents an "Open with" (VIEW) or "Share" (SEND, SEND_MULTIPLE) intent carries. */
    @SuppressWarnings("deprecation")
    private static List<Uri> incomingUris(Intent intent) {
        List<Uri> uris = new ArrayList<>();
        String action = intent.getAction();
        if (Intent.ACTION_VIEW.equals(action) && intent.getData() != null) {
            uris.add(intent.getData());
        } else if (Intent.ACTION_SEND.equals(action)) {
            Parcelable stream = intent.getParcelableExtra(Intent.EXTRA_STREAM);
            if (stream instanceof Uri) {
                uris.add((Uri) stream);
            }
        } else if (Intent.ACTION_SEND_MULTIPLE.equals(action)) {
            ArrayList<Parcelable> streams = intent.getParcelableArrayListExtra(Intent.EXTRA_STREAM);
            if (streams != null) {
                for (Parcelable stream : streams) {
                    if (stream instanceof Uri) {
                        uris.add((Uri) stream);
                    }
                }
            }
        }
        return uris;
    }

    /**
     * Keeps the permission granted to {@code uri} across restarts, so a later save still reaches
     * the document. Returns whether writing was kept: a write grant that can't be kept ends with
     * this activity, and a save after that wouldn't reach the document.
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
        return false;
    }

    /** An image the Rust side exported (share.rs), offered to the share sheet. */
    private void sendImage(File file) {
        Uri uri;
        try {
            uri = FileProvider.getUriForFile(this, getPackageName() + ".files", file);
        } catch (IllegalArgumentException e) {
            toast(text("Couldn't share ", "Não foi possível compartilhar ") + file.getName());
            return;
        }
        Intent send = new Intent(Intent.ACTION_SEND);
        send.setType("image/png");
        send.putExtra(Intent.EXTRA_STREAM, uri);
        // The ClipData carries the read grant to the app picked in the chooser (and its preview).
        send.setClipData(ClipData.newRawUri(file.getName(), uri));
        send.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
        Intent chooser = Intent.createChooser(send, text("Share image", "Compartilhar imagem"));
        // Sending the image back to this app would only open a copy of it.
        chooser.putExtra(Intent.EXTRA_EXCLUDE_COMPONENTS, new android.content.ComponentName[] {new android.content.ComponentName(this, MainActivity.class)});
        try {
            startActivity(chooser);
        } catch (ActivityNotFoundException e) {
            toast(text("No app can receive it", "Nenhum app pode recebê-lo"));
        }
    }

    /**
     * An image the Rust side exported (share.rs), saved into the gallery (Pictures/PhotoCraft
     * Mobile). Android 10 and later only: MediaStore then needs no storage permission.
     */
    private void saveToGallery(File file) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) {
            toast(text("Saving to the gallery needs Android 10 or later. Use Share image… instead.",
                    "Salvar na Galeria precisa do Android 10 ou mais novo. Use Compartilhar imagem…"));
            return;
        }
        io.execute(() -> {
            ContentResolver resolver = getContentResolver();
            ContentValues values = new ContentValues();
            values.put(MediaStore.MediaColumns.DISPLAY_NAME, file.getName());
            values.put(MediaStore.MediaColumns.MIME_TYPE, "image/png");
            values.put(MediaStore.MediaColumns.RELATIVE_PATH, Environment.DIRECTORY_PICTURES + "/" + GALLERY_FOLDER);
            // Hidden from the gallery until it is whole.
            values.put(MediaStore.MediaColumns.IS_PENDING, 1);
            Uri item = null;
            boolean ok = false;
            try {
                item = resolver.insert(MediaStore.Images.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY), values);
                if (item != null) {
                    try (InputStream in = new FileInputStream(file); OutputStream out = resolver.openOutputStream(item)) {
                        ok = out != null && pipe(in, out);
                    }
                    ContentValues done = new ContentValues();
                    done.put(MediaStore.MediaColumns.IS_PENDING, 0);
                    resolver.update(item, done, null, null);
                }
            } catch (IOException | RuntimeException e) {
                ok = false;
            }
            if (!ok && item != null) {
                try {
                    resolver.delete(item, null, null);
                } catch (RuntimeException e) {
                    // Left pending: the system removes it.
                }
            }
            toast(ok ? text("Saved to the gallery: ", "Salvo na Galeria: ") + file.getName()
                    : text("Couldn't save to the gallery", "Não foi possível salvar na Galeria"));
        });
    }

    /** A link from the app (Help, the start screen) in the browser, or the app that handles it. */
    private void openUrl(String url) {
        Intent view = new Intent(Intent.ACTION_VIEW, Uri.parse(url));
        view.addCategory(Intent.CATEGORY_BROWSABLE);
        try {
            startActivity(view);
        } catch (ActivityNotFoundException e) {
            toast(text("No app can open ", "Nenhum app abre ") + url);
        }
    }

    /** A short message, from any thread. */
    private void toast(String message) {
        runOnUiThread(() -> Toast.makeText(this, message, Toast.LENGTH_SHORT).show());
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

    /**
     * A recent file's cache copy that Android deleted with the cache: copied again from its
     * document, so the recent file opens. A document that is gone, or whose permission was taken
     * back, stays missing, and opening it says so.
     */
    private void restore(String path, String uriText) {
        Uri uri = Uri.parse(uriText);
        File target = new File(path);
        // Under another name until it is whole, so an early open never reads half a file.
        File part = new File(path + ".part");
        io.execute(() -> {
            if (!copy(uri, part) || !part.renameTo(target)) {
                android.util.Log.w("photocraft-mobile", "couldn't copy back " + target.getName() + " from its document");
                //noinspection ResultOfMethodCallIgnored
                part.delete();
            }
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
        return copy(getContentResolver(), uri, target);
    }

    private static boolean copy(ContentResolver resolver, Uri uri, File target) {
        File dir = target.getParentFile();
        if (dir != null) {
            //noinspection ResultOfMethodCallIgnored
            dir.mkdirs();
        }
        try (InputStream in = resolver.openInputStream(uri); OutputStream out = new FileOutputStream(target)) {
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

    // ---- Clipboard (clipboard.rs) -----------------------------------------------------------

    /**
     * Puts the PNG at {@code path} (an image copied in the app, which the Rust side wrote into the
     * clipboard folder) on the clipboard, as a content URI other apps paste from. Called on the
     * Rust side's worker thread.
     */
    @Keep
    private static boolean setClipboardFile(String path) {
        Context context = appContext;
        if (context == null) {
            return false;
        }
        try {
            ClipboardManager clipboard = context.getSystemService(ClipboardManager.class);
            if (clipboard == null) {
                return false;
            }
            Uri uri = FileProvider.getUriForFile(context, context.getPackageName() + ".files", new File(path));
            clipboard.setPrimaryClip(ClipData.newUri(context.getContentResolver(), "PhotoCraft Mobile", uri));
            return true;
        } catch (RuntimeException e) {
            return false;
        }
    }

    /**
     * For a paste: a file holding the clipboard's image, or null when it holds none. That is the
     * app's own copy itself, or the image another app put there, copied into the clipboard folder.
     * Called on the Rust side's thread.
     */
    @Keep
    private static String clipboardFile() {
        Context context = appContext;
        if (context == null) {
            return null;
        }
        try {
            ClipboardManager clipboard = context.getSystemService(ClipboardManager.class);
            ClipData clip = clipboard != null ? clipboard.getPrimaryClip() : null;
            if (clip == null) {
                return null;
            }
            ContentResolver resolver = context.getContentResolver();
            File dir = new File(context.getCacheDir(), CLIPBOARD_DIR);
            for (int i = 0; i < clip.getItemCount(); i++) {
                Uri uri = clip.getItemAt(i).getUri();
                if (uri == null) {
                    continue;
                }
                List<String> segments = uri.getPathSegments();
                if ((context.getPackageName() + ".files").equals(uri.getAuthority()) && segments.size() == 2
                        && CLIPBOARD_DIR.equals(segments.get(0))) {
                    File own = new File(dir, segments.get(1));
                    if (own.isFile()) {
                        return own.getAbsolutePath();
                    }
                    continue;
                }
                String type = resolver.getType(uri);
                if (type == null || !type.startsWith("image/")) {
                    continue;
                }
                File[] earlier = dir.listFiles((d, name) -> name.startsWith(PASTE_NAME));
                if (earlier != null) {
                    for (File f : earlier) {
                        //noinspection ResultOfMethodCallIgnored
                        f.delete();
                    }
                }
                String ext = MimeTypeMap.getSingleton().getExtensionFromMimeType(type);
                File target = new File(dir, ext != null ? PASTE_NAME + "." + ext : PASTE_NAME);
                if (copy(resolver, uri, target)) {
                    return target.getAbsolutePath();
                }
            }
        } catch (RuntimeException e) {
            // Not allowed to read the clipboard (the app isn't in front), or the image is gone.
        }
        return null;
    }

    // ---- Rust (apps/android/src) ----------------------------------------------------------

    /** touch_tools.rs */
    private static native void nativeTouchTool(int pointerId, int toolType);

    /** ime.rs: the keyboard edited the text field. */
    private static native void nativeImeChanged();

    /** ime.rs: the keyboard's action key. */
    private static native void nativeImeAction();

    /** clipboard.rs: lets the Rust side call {@link #clipboardFile} and {@link #setClipboardFile}. */
    private static native void nativeClipboardInit();

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

    /** files.rs: a new cache folder id for opened documents. */
    private static native long nativeNextId();

    /** files.rs: open these copies (one path per line), sent by another app. */
    private static native void nativeOpenFiles(String paths);
}
