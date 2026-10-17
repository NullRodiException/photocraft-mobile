package dev.photocraftmobile.app;

import android.view.MotionEvent;

import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Set;

/**
 * Palm rejection, decided for each contact on its own.
 *
 * <ul>
 *   <li>A stylus always reaches the app.
 *   <li>A finger (or palm) that lands while the pen touches or hovers over the screen, or within
 *       {@link #GRACE_MS} after the pen left, is dropped until it lifts, so a hand resting on the
 *       screen never paints or navigates.
 *   <li>A finger the app already has is cancelled when the pen touches down. Otherwise egui would
 *       keep its pointer on that finger, and the pen couldn't paint.
 * </ul>
 *
 * A {@link MotionEvent} carries every contact on the screen, so an event that mixes the pen with
 * dropped fingers reaches the app rebuilt with only the contacts it keeps (see
 * {@link Decision}). This class sees only ids, tool types and actions, without Android types, so
 * {@code PalmFilterTest} runs it on the JVM.
 */
final class PalmFilter {
    /** How long after the pen leaves the screen a new finger touch is still taken for the palm. */
    static final long GRACE_MS = 500;

    /** What reaches the app for one event. */
    static final class Decision {
        /** Pass the event unchanged. */
        static final Decision PASS = new Decision(null, 0, new int[0]);

        /**
         * The indices (in the event) of the contacts the app gets, in order. {@code null} means the
         * whole event goes on unchanged, and an empty array means it is dropped.
         */
        final int[] keep;
        /** The action of the rebuilt event (with the pointer index shifted in), when {@link #keep} is a subset. */
        final int action;
        /** Indices of contacts the app had and must now lose: an ACTION_CANCEL with just them goes first. */
        final int[] cancel;

        Decision(int[] keep, int action, int[] cancel) {
            this.keep = keep;
            this.action = action;
            this.cancel = cancel;
        }

        boolean dropped() {
            return keep != null && keep.length == 0;
        }
    }

    private boolean penInRange;
    private long penLeftAt = Long.MIN_VALUE / 2;
    /** Contacts dropped until they lift, by pointer id. */
    private final Set<Integer> rejected = new HashSet<>();
    /** Finger contacts the app has, by pointer id. */
    private final Set<Integer> fingers = new HashSet<>();

    static boolean isPen(int toolType) {
        return toolType == MotionEvent.TOOL_TYPE_STYLUS || toolType == MotionEvent.TOOL_TYPE_ERASER;
    }

    /** A finger or palm, or a contact the device doesn't name (never a mouse). */
    private static boolean isFinger(int toolType) {
        return toolType == MotionEvent.TOOL_TYPE_FINGER || toolType == MotionEvent.TOOL_TYPE_UNKNOWN;
    }

    private boolean penNear(long eventTime) {
        return penInRange || eventTime - penLeftAt < GRACE_MS;
    }

    private void trackPen(int actionMasked, int actionIndex, int[] tools, long eventTime) {
        boolean actorIsPen = actionIndex < tools.length && isPen(tools[actionIndex]);
        switch (actionMasked) {
            case MotionEvent.ACTION_HOVER_ENTER:
            case MotionEvent.ACTION_HOVER_MOVE:
                if (actorIsPen) {
                    penInRange = true;
                }
                break;
            case MotionEvent.ACTION_HOVER_EXIT:
                // A pen touching the screen sends HOVER_EXIT just before DOWN; DOWN sets it back.
                if (actorIsPen) {
                    penInRange = false;
                    penLeftAt = eventTime;
                }
                break;
            case MotionEvent.ACTION_DOWN:
            case MotionEvent.ACTION_POINTER_DOWN:
                if (actorIsPen) {
                    penInRange = true;
                }
                break;
            case MotionEvent.ACTION_UP:
            case MotionEvent.ACTION_POINTER_UP:
                if (actorIsPen) {
                    penInRange = false;
                    penLeftAt = eventTime;
                }
                break;
            case MotionEvent.ACTION_CANCEL:
                for (int tool : tools) {
                    if (isPen(tool)) {
                        penInRange = false;
                        penLeftAt = eventTime;
                    }
                }
                break;
            default:
                break;
        }
    }

    /**
     * Decides what of an event reaches the app: {@code ids} and {@code tools} are each contact's
     * pointer id and tool type, by index in the event.
     */
    Decision onEvent(int actionMasked, int actionIndex, int[] ids, int[] tools, long eventTime) {
        trackPen(actionMasked, actionIndex, tools, eventTime);
        if (actionMasked == MotionEvent.ACTION_DOWN) {
            // A new stream: nothing is down from before.
            rejected.clear();
            fingers.clear();
        }
        int actor = actionIndex < ids.length ? ids[actionIndex] : -1;
        boolean lands = actionMasked == MotionEvent.ACTION_DOWN || actionMasked == MotionEvent.ACTION_POINTER_DOWN;
        boolean lifts = actionMasked == MotionEvent.ACTION_UP || actionMasked == MotionEvent.ACTION_POINTER_UP;
        List<Integer> cancel = new ArrayList<>();
        if (lands && actionIndex < tools.length) {
            int tool = tools[actionIndex];
            if (isPen(tool)) {
                // The pen takes over: the fingers the app has are cancelled, and stay out until they lift.
                for (int i = 0; i < ids.length; i++) {
                    if (fingers.remove(ids[i])) {
                        rejected.add(ids[i]);
                        cancel.add(i);
                    }
                }
            } else if (isFinger(tool)) {
                if (penNear(eventTime)) {
                    rejected.add(actor);
                } else {
                    fingers.add(actor);
                }
            }
        }
        Decision decision = decide(actionMasked, actionIndex, ids, actor, lands || lifts, toArray(cancel));
        if (lifts) {
            rejected.remove(actor);
            fingers.remove(actor);
        }
        if (actionMasked == MotionEvent.ACTION_UP || actionMasked == MotionEvent.ACTION_CANCEL) {
            rejected.clear();
            fingers.clear();
        }
        return decision;
    }

    private Decision decide(int actionMasked, int actionIndex, int[] ids, int actor, boolean actorAction, int[] cancel) {
        List<Integer> keep = new ArrayList<>();
        for (int i = 0; i < ids.length; i++) {
            if (!rejected.contains(ids[i])) {
                keep.add(i);
            }
        }
        if (keep.size() == ids.length) {
            return cancel.length == 0 ? Decision.PASS : new Decision(null, 0, cancel);
        }
        if (keep.isEmpty() || (actorAction && rejected.contains(actor))) {
            // Nothing the app keeps, or only a dropped contact landing or lifting.
            return new Decision(new int[0], 0, cancel);
        }
        int action;
        switch (actionMasked) {
            case MotionEvent.ACTION_DOWN:
            case MotionEvent.ACTION_POINTER_DOWN:
                action = keep.size() == 1 ? MotionEvent.ACTION_DOWN : withIndex(MotionEvent.ACTION_POINTER_DOWN, keep.indexOf(actionIndex));
                break;
            case MotionEvent.ACTION_UP:
            case MotionEvent.ACTION_POINTER_UP:
                action = keep.size() == 1 ? MotionEvent.ACTION_UP : withIndex(MotionEvent.ACTION_POINTER_UP, keep.indexOf(actionIndex));
                break;
            default:
                action = actionMasked;
                break;
        }
        return new Decision(toArray(keep), action, cancel);
    }

    private static int withIndex(int actionMasked, int index) {
        return actionMasked | (index << MotionEvent.ACTION_POINTER_INDEX_SHIFT);
    }

    private static int[] toArray(List<Integer> list) {
        int[] out = new int[list.size()];
        for (int i = 0; i < out.length; i++) {
            out[i] = list.get(i);
        }
        return out;
    }
}
