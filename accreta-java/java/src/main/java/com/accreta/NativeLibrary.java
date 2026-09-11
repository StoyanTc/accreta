package com.accreta;

/** Loads the {@code accreta_java} native library exactly once per JVM. */
final class NativeLibrary {
    private static volatile boolean loaded = false;

    private NativeLibrary() {}

    static synchronized void load() {
        if (!loaded) {
            System.loadLibrary("accreta_java");
            loaded = true;
        }
    }
}
