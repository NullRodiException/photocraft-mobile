package dev.photocraftmobile.app;

import android.os.Bundle;
import android.view.MotionEvent;
import android.view.View;

import androidx.core.view.WindowCompat;
import androidx.core.view.WindowInsetsCompat;
import androidx.core.view.WindowInsetsControllerCompat;

import com.google.androidgamesdk.GameActivity;

/**
 * GameActivity running the Rust library (libphotocraft_mobile.so), plus two things a drawing app
 * on a pen tablet needs and the Rust side cannot do through winit:
 *
 * <ul>
 *   <li><b>Immersive full screen.</b> The status bar, the navigation bar and the One UI taskbar
 *       are hidden, so they never cover the editor. A swipe from the edge shows them for a moment.
 *   <li><b>Palm rejection.</b> While a stylus (S Pen) touches or hovers over the screen, and for
 *       {@link #PEN_GRACE_MS} after it leaves, touches that start with a finger or palm are dropped
 *       before they reach the app, so a hand resting on the screen doesn't paint. With the pen
 *       away, fingers work as usual.
 * </ul>
 */
public class MainActivity extends GameActivity {
    /** How long after the pen leaves the screen a new finger touch is still taken for the palm. */
    private static final long PEN_GRACE_MS = 500;

    /** The pen is touching or hovering. */
    private boolean penInRange;
    /** When the pen last left (event time, ms). */
    private long penLeftAt = Long.MIN_VALUE / 2;
    /** The finger gesture in progress is being dropped (decided at its first contact). */
    private boolean rejectingGesture;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        hideSystemBars();
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

    @Override
    protected boolean processMotionEvent(MotionEvent event) {
        if (isPen(event)) {
            trackPen(event);
            return super.processMotionEvent(event);
        }
        int action = event.getActionMasked();
        if (action == MotionEvent.ACTION_DOWN) {
            // The whole gesture is kept or dropped together, so the app never sees half of one.
            rejectingGesture = penInRange || event.getEventTime() - penLeftAt < PEN_GRACE_MS;
        }
        boolean drop = rejectingGesture && isFingerTouch(event);
        if (action == MotionEvent.ACTION_UP || action == MotionEvent.ACTION_CANCEL) {
            rejectingGesture = false;
        }
        return drop || super.processMotionEvent(event);
    }

    private void trackPen(MotionEvent event) {
        switch (event.getActionMasked()) {
            case MotionEvent.ACTION_DOWN:
            case MotionEvent.ACTION_MOVE:
            case MotionEvent.ACTION_HOVER_ENTER:
            case MotionEvent.ACTION_HOVER_MOVE:
                penInRange = true;
                break;
            case MotionEvent.ACTION_UP:
            case MotionEvent.ACTION_CANCEL:
            case MotionEvent.ACTION_HOVER_EXIT:
                // A pen touching the screen sends HOVER_EXIT just before DOWN; DOWN sets it back.
                penInRange = false;
                penLeftAt = event.getEventTime();
                break;
            default:
                break;
        }
    }

    private static boolean isPen(MotionEvent event) {
        int tool = event.getToolType(event.getActionIndex());
        return tool == MotionEvent.TOOL_TYPE_STYLUS || tool == MotionEvent.TOOL_TYPE_ERASER;
    }

    private static boolean isFingerTouch(MotionEvent event) {
        for (int i = 0; i < event.getPointerCount(); i++) {
            if (event.getToolType(i) != MotionEvent.TOOL_TYPE_FINGER) {
                return false;
            }
        }
        return event.getPointerCount() > 0;
    }
}
