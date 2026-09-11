package com.accreta;

/** Mirrors {@code accreta::errors::SchemaError} (e.g. no dimensions/measures registered). */
public class SchemaException extends Exception {
    public SchemaException(String message) {
        super(message);
    }
}
