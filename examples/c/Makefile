CC ?= cc
CFLAGS ?= -std=c11 -Wall -Wextra -Wpedantic
INCLUDE := ../../include
LIBDIR := ../../target/debug

example: example.c
	$(CC) $(CFLAGS) -I$(INCLUDE) example.c -L$(LIBDIR) -Wl,-rpath,$(LIBDIR) -laccreta_ffi -o example

run: example
	LD_LIBRARY_PATH=$(LIBDIR) ./example

clean:
	rm -f example

.PHONY: run clean
