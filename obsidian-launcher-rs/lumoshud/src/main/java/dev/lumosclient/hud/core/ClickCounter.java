package dev.lumosclient.hud.core;

import java.util.ArrayDeque;

/** Clicks in the last second. */
public final class ClickCounter {
    private final ArrayDeque<Long> times = new ArrayDeque<>();

    public synchronized void click(long nowMs) {
        times.addLast(nowMs);
        if (times.size() > 200) times.removeFirst();
    }

    public synchronized int perSecond(long nowMs) {
        while (!times.isEmpty() && nowMs - times.peekFirst() >= 1000) times.removeFirst();
        return times.size();
    }
}
