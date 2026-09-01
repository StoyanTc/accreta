cmake_minimum_required(VERSION 3.15)

project(accreta_c_examples LANGUAGES C)

option(ACCRETA_LINK_STATIC
    "Link examples against the static accreta-ffi library"
    OFF
)

# =============================================================================
# Paths
#
# Current directory:
#   accreta/accreta-ffi/examples/c
#
# FFI crate:
#   accreta/accreta-ffi
#
# Workspace root:
#   accreta
# =============================================================================

set(ACCRETA_FFI_ROOT
    "${CMAKE_CURRENT_LIST_DIR}/../.."
    CACHE PATH
    "Path to the accreta-ffi crate"
)

set(ACCRETA_WORKSPACE_ROOT
    "${ACCRETA_FFI_ROOT}/.."
    CACHE PATH
    "Path to the accreta workspace root"
)

# Generated C header belongs to the FFI crate.
set(ACCRETA_INCLUDE_DIR
    "${ACCRETA_FFI_ROOT}/include"
)

# Cargo workspace artifacts are placed in the workspace target directory.
set(ACCRETA_RELEASE_DIR
    "${ACCRETA_WORKSPACE_ROOT}/target/release"
)

set(ACCRETA_HEADER
    "${ACCRETA_INCLUDE_DIR}/accreta_ffi.h"
)

if(NOT EXISTS "${ACCRETA_HEADER}")
    message(FATAL_ERROR
        "accreta_ffi.h was not found at ${ACCRETA_HEADER}. "
        "Run 'cargo build --release -p accreta-ffi' "
        "from the workspace root first."
    )
endif()

# =============================================================================
# Examples
# =============================================================================

add_executable(accreta_c_example example.c)

add_executable(tdigest_quantiles tdigest_quantiles.c)

set(ACCRETA_EXAMPLES
    accreta_c_example
    tdigest_quantiles
)

# =============================================================================
# Common compiler configuration
# =============================================================================

foreach(example IN LISTS ACCRETA_EXAMPLES)

    target_compile_features(${example} PRIVATE c_std_11)

    target_compile_options(${example} PRIVATE
        $<$<C_COMPILER_ID:GNU,Clang,AppleClang>:-Wall;-Wextra;-Wpedantic>
        $<$<C_COMPILER_ID:MSVC>:/W4>
    )

    target_include_directories(${example} PRIVATE
        "${ACCRETA_INCLUDE_DIR}"
    )

endforeach()

# =============================================================================
# Library linking
# =============================================================================

if(ACCRETA_LINK_STATIC)

    # -------------------------------------------------------------------------
    # Static library
    # -------------------------------------------------------------------------

    if(WIN32)
        set(ACCRETA_STATIC_LIBRARY
            "${ACCRETA_RELEASE_DIR}/accreta_ffi.lib"
        )
    else()
        set(ACCRETA_STATIC_LIBRARY
            "${ACCRETA_RELEASE_DIR}/libaccreta_ffi.a"
        )
    endif()

    if(NOT EXISTS "${ACCRETA_STATIC_LIBRARY}")
        message(FATAL_ERROR
            "Static accreta-ffi library was not found at "
            "${ACCRETA_STATIC_LIBRARY}. "
            "Run 'cargo build --release -p accreta-ffi' first."
        )
    endif()

    add_library(accreta_ffi STATIC IMPORTED GLOBAL)

    set_target_properties(accreta_ffi PROPERTIES
        IMPORTED_LOCATION "${ACCRETA_STATIC_LIBRARY}"
        INTERFACE_INCLUDE_DIRECTORIES "${ACCRETA_INCLUDE_DIR}"
    )

    # Link all examples.
    foreach(example IN LISTS ACCRETA_EXAMPLES)

        if(WIN32)
            # Rust static libraries may require Windows system libraries.
            target_link_libraries(${example} PRIVATE
                accreta_ffi
                ws2_32
                userenv
                bcrypt
                ntdll
            )
        else()
            target_link_libraries(${example} PRIVATE
                accreta_ffi
            )
        endif()

    endforeach()

else()

    # -------------------------------------------------------------------------
    # Shared library
    # -------------------------------------------------------------------------

    if(WIN32)

        set(ACCRETA_RUNTIME_LIBRARY
            "${ACCRETA_RELEASE_DIR}/accreta_ffi.dll"
        )

        if(MSVC)
            set(ACCRETA_IMPORT_LIBRARY
                "${ACCRETA_RELEASE_DIR}/accreta_ffi.dll.lib"
            )
        else()
            set(ACCRETA_IMPORT_LIBRARY
                "${ACCRETA_RELEASE_DIR}/libaccreta_ffi.dll.a"
            )
        endif()

        if(NOT EXISTS "${ACCRETA_RUNTIME_LIBRARY}")
            message(FATAL_ERROR
                "accreta_ffi.dll was not found at "
                "${ACCRETA_RUNTIME_LIBRARY}. "
                "Run 'cargo build --release -p accreta-ffi' first."
            )
        endif()

        if(NOT EXISTS "${ACCRETA_IMPORT_LIBRARY}")
            message(FATAL_ERROR
                "The Windows import library was not found at "
                "${ACCRETA_IMPORT_LIBRARY}. "
                "Run 'cargo build --release -p accreta-ffi' first."
            )
        endif()

        add_library(accreta_ffi SHARED IMPORTED GLOBAL)

        set_target_properties(accreta_ffi PROPERTIES
            IMPORTED_IMPLIB "${ACCRETA_IMPORT_LIBRARY}"
            IMPORTED_LOCATION "${ACCRETA_RUNTIME_LIBRARY}"
            INTERFACE_INCLUDE_DIRECTORIES "${ACCRETA_INCLUDE_DIR}"
        )

        foreach(example IN LISTS ACCRETA_EXAMPLES)

            target_link_libraries(${example} PRIVATE
                accreta_ffi
            )

            # Keep the DLL beside each executable.
            add_custom_command(TARGET ${example} POST_BUILD
                COMMAND ${CMAKE_COMMAND} -E copy_if_different
                    "${ACCRETA_RUNTIME_LIBRARY}"
                    "$<TARGET_FILE_DIR:${example}>"
            )

        endforeach()

    elseif(APPLE)

        set(ACCRETA_SHARED_LIBRARY
            "${ACCRETA_RELEASE_DIR}/libaccreta_ffi.dylib"
        )

        if(NOT EXISTS "${ACCRETA_SHARED_LIBRARY}")
            message(FATAL_ERROR
                "libaccreta_ffi.dylib was not found at "
                "${ACCRETA_SHARED_LIBRARY}. "
                "Run 'cargo build --release -p accreta-ffi' first."
            )
        endif()

        add_library(accreta_ffi SHARED IMPORTED GLOBAL)

        set_target_properties(accreta_ffi PROPERTIES
            IMPORTED_LOCATION "${ACCRETA_SHARED_LIBRARY}"
            INTERFACE_INCLUDE_DIRECTORIES "${ACCRETA_INCLUDE_DIR}"
        )

        foreach(example IN LISTS ACCRETA_EXAMPLES)
            target_link_libraries(${example} PRIVATE
                accreta_ffi
            )
        endforeach()

        # Allow examples to find the repository-local dylib.
        set_target_properties(${ACCRETA_EXAMPLES} PROPERTIES
            BUILD_RPATH "${ACCRETA_RELEASE_DIR}"
        )

    else()

        set(ACCRETA_SHARED_LIBRARY
            "${ACCRETA_RELEASE_DIR}/libaccreta_ffi.so"
        )

        if(NOT EXISTS "${ACCRETA_SHARED_LIBRARY}")
            message(FATAL_ERROR
                "libaccreta_ffi.so was not found at "
                "${ACCRETA_SHARED_LIBRARY}. "
                "Run 'cargo build --release -p accreta-ffi' first."
            )
        endif()

        add_library(accreta_ffi SHARED IMPORTED GLOBAL)

        set_target_properties(accreta_ffi PROPERTIES
            IMPORTED_LOCATION "${ACCRETA_SHARED_LIBRARY}"
            INTERFACE_INCLUDE_DIRECTORIES "${ACCRETA_INCLUDE_DIR}"
        )

        foreach(example IN LISTS ACCRETA_EXAMPLES)
            target_link_libraries(${example} PRIVATE
                accreta_ffi
            )
        endforeach()

        # Allow examples to find the repository-local shared library.
        set_target_properties(${ACCRETA_EXAMPLES} PROPERTIES
            BUILD_RPATH "${ACCRETA_RELEASE_DIR}"
        )

    endif()

endif()
