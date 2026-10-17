package dev.photocraftmobile.app;

import static org.junit.Assert.assertArrayEquals;
import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertNull;
import static org.junit.Assert.assertSame;
import static org.junit.Assert.assertTrue;

import android.view.MotionEvent;

import org.junit.Test;

public class PalmFilterTest {
    private static final int FINGER = MotionEvent.TOOL_TYPE_FINGER;
    private static final int PEN = MotionEvent.TOOL_TYPE_STYLUS;
    private static final int DOWN = MotionEvent.ACTION_DOWN;
    private static final int UP = MotionEvent.ACTION_UP;
    private static final int MOVE = MotionEvent.ACTION_MOVE;
    private static final int POINTER_DOWN = MotionEvent.ACTION_POINTER_DOWN;
    private static final int POINTER_UP = MotionEvent.ACTION_POINTER_UP;
    private static final int HOVER_MOVE = MotionEvent.ACTION_HOVER_MOVE;
    private static final int HOVER_EXIT = MotionEvent.ACTION_HOVER_EXIT;

    private static int[] ids(int... v) {
        return v;
    }

    private static int[] tools(int... v) {
        return v;
    }

    private static int pointerAction(int masked, int index) {
        return masked | (index << MotionEvent.ACTION_POINTER_INDEX_SHIFT);
    }

    @Test
    public void fingersWorkWithThePenAway() {
        PalmFilter f = new PalmFilter();
        assertSame(PalmFilter.Decision.PASS, f.onEvent(DOWN, 0, ids(0), tools(FINGER), 1000));
        assertSame(PalmFilter.Decision.PASS, f.onEvent(POINTER_DOWN, 1, ids(0, 1), tools(FINGER, FINGER), 1010));
        assertSame(PalmFilter.Decision.PASS, f.onEvent(MOVE, 0, ids(0, 1), tools(FINGER, FINGER), 1020));
    }

    @Test
    public void aPalmLandingWhileThePenHoversIsDroppedAndThePenStrokeKeepsOnlyThePen() {
        PalmFilter f = new PalmFilter();
        f.onEvent(HOVER_MOVE, 0, ids(5), tools(PEN), 1000);
        // The palm lands: dropped.
        assertTrue(f.onEvent(DOWN, 0, ids(0), tools(FINGER), 1010).dropped());
        assertTrue(f.onEvent(MOVE, 0, ids(0), tools(FINGER), 1020).dropped());
        // The pen touches down beside it: the app sees the pen alone, starting its stream.
        PalmFilter.Decision down = f.onEvent(POINTER_DOWN, 1, ids(0, 5), tools(FINGER, PEN), 1030);
        assertArrayEquals(new int[] {1}, down.keep);
        assertEquals(DOWN, down.action);
        assertEquals(0, down.cancel.length);
        // Moves keep only the pen, though the palm is pointer 0 (the event's action index).
        PalmFilter.Decision move = f.onEvent(MOVE, 0, ids(0, 5), tools(FINGER, PEN), 1040);
        assertArrayEquals(new int[] {1}, move.keep);
        assertEquals(MOVE, move.action);
        // The pen lifts: the app's stream ends.
        PalmFilter.Decision up = f.onEvent(POINTER_UP, 1, ids(0, 5), tools(FINGER, PEN), 1050);
        assertArrayEquals(new int[] {1}, up.keep);
        assertEquals(UP, up.action);
        // The palm lifts later: nothing.
        assertTrue(f.onEvent(UP, 0, ids(0), tools(FINGER), 1060).dropped());
    }

    @Test
    public void aFingerTheAppHasIsCancelledWhenThePenTouches() {
        PalmFilter f = new PalmFilter();
        assertSame(PalmFilter.Decision.PASS, f.onEvent(DOWN, 0, ids(0), tools(FINGER), 1000));
        PalmFilter.Decision pen = f.onEvent(POINTER_DOWN, 1, ids(0, 5), tools(FINGER, PEN), 2000);
        assertArrayEquals(new int[] {0}, pen.cancel);
        assertArrayEquals(new int[] {1}, pen.keep);
        assertEquals(DOWN, pen.action);
        // The cancelled finger stays out until it lifts.
        assertArrayEquals(new int[] {1}, f.onEvent(MOVE, 0, ids(0, 5), tools(FINGER, PEN), 2010).keep);
        assertTrue(f.onEvent(POINTER_UP, 0, ids(0, 5), tools(FINGER, PEN), 2020).dropped());
        assertSame(PalmFilter.Decision.PASS, f.onEvent(MOVE, 0, ids(5), tools(PEN), 2030));
    }

    @Test
    public void aFingerLandingBesideAKeptOneKeepsTheRightIndex() {
        PalmFilter f = new PalmFilter();
        f.onEvent(HOVER_MOVE, 0, ids(5), tools(PEN), 1000);
        f.onEvent(DOWN, 0, ids(0), tools(FINGER), 1010); // palm: dropped
        f.onEvent(POINTER_DOWN, 1, ids(0, 5), tools(FINGER, PEN), 1020); // pen
        // A second pen-like contact lands third: the app's event has two contacts, it is the second.
        PalmFilter.Decision eraser = f.onEvent(POINTER_DOWN, 2, ids(0, 5, 6), tools(FINGER, PEN, MotionEvent.TOOL_TYPE_ERASER), 1030);
        assertArrayEquals(new int[] {1, 2}, eraser.keep);
        assertEquals(pointerAction(POINTER_DOWN, 1), eraser.action);
    }

    @Test
    public void theGraceTimeEndsTheRejection() {
        PalmFilter f = new PalmFilter();
        f.onEvent(HOVER_MOVE, 0, ids(5), tools(PEN), 1000);
        f.onEvent(HOVER_EXIT, 0, ids(5), tools(PEN), 1100);
        assertTrue("just after the pen left", f.onEvent(DOWN, 0, ids(0), tools(FINGER), 1100 + PalmFilter.GRACE_MS - 1).dropped());
        f.onEvent(UP, 0, ids(0), tools(FINGER), 1600);
        assertSame("well after", PalmFilter.Decision.PASS, f.onEvent(DOWN, 0, ids(1), tools(FINGER), 1100 + PalmFilter.GRACE_MS + 1));
    }

    @Test
    public void aMouseIsNeverTakenForAPalm() {
        PalmFilter f = new PalmFilter();
        f.onEvent(HOVER_MOVE, 0, ids(5), tools(PEN), 1000);
        assertSame(PalmFilter.Decision.PASS, f.onEvent(DOWN, 0, ids(0), tools(MotionEvent.TOOL_TYPE_MOUSE), 1010));
    }

    @Test
    public void passingTheWholeEventLeavesNoSubset() {
        PalmFilter.Decision pass = new PalmFilter().onEvent(DOWN, 0, ids(5), tools(PEN), 1000);
        assertNull(pass.keep);
    }
}
