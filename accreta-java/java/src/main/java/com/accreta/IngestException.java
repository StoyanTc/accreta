package com.accreta;

/** Mirrors {@code accreta::errors::IngestError} (measure/dimension count or type mismatch). */
public class IngestException extends Exception {
    public IngestException(String message) {
        super(message);
    }
}
