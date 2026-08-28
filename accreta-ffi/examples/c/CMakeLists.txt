cmake_minimum_required(VERSION 3.15)

project(accreta_c_example LANGUAGES C)

option(ACCRETA_LINK_STATIC "Link the example against the static accreta-ffi library" OFF)

set(ACCRETA_ROOT "${CMAKE_CURRENT_LIST_DIR}/../.." CACHE PATH
    "Path to the accreta-ffi repository root")

set(ACCRETA_INCLUDE_DIR "${ACCRETA_ROOT}/include")
set(ACCRETA_RELEASE_DIR "${ACCRETA_ROOT}/target/release")

set(ACCRETA_HEADER "${ACCRETA_INCLUDE_DIR}/accreta_ffi.h")

if(NOT EXISTS "${ACCRETA_HEADER}")
    message(FATAL_ERROR
        "accreta_ffi.h was not found at ${ACCRETA_HEADER}. "
        "Run 'cargo build --release' from the accreta-ffi repository root first.")
endif()

add_executable(accreta_c_example example.c)

target_compile_features(accreta_c_example PRIVATE c_std_11)

target_compile_options(accreta_c_example PRIVATE
    $<$<C_COMPILER_ID:GNU,Clang,AppleClang>:-Wall;-Wextra;-Wpedantic>
    $<$<C_COMPILER_ID:MSVC>:/W4>
)

target_include_directories(accreta_c_example PRIVATE
    "${ACCRETA_INCLUDE_DIR}"
)

if(ACCRETA_LINK_STATIC)
    if(WIN32)
        set(ACCRETA_STATIC_LIBRARY "${ACCRETA_RELEASE_DIR}/accreta_ffi.lib")
    else()
        set(ACCRETA_STATIC_LIBRARY "${ACCRETA_RELEASE_DIR}/libaccreta_ffi.a")
    endif()

    if(NOT EXISTS "${ACCRETA_STATIC_LIBRARY}")
        message(FATAL_ERROR
            "Static accreta-ffi library was not found at "
            "${ACCRETA_STATIC_LIBRARY}. Run 'cargo build --release' first.")
    endif()

    add_library(accreta_ffi STATIC IMPORTED GLOBAL)
    set_target_properties(accreta_ffi PROPERTIES
        IMPORTED_LOCATION "${ACCRETA_STATIC_LIBRARY}"
        INTERFACE_INCLUDE_DIRECTORIES "${ACCRETA_INCLUDE_DIR}"
    )

    if(WIN32)
        # Rust's static library may require the Windows system libraries
        # normally supplied by the Rust/MSVC toolchain.
        target_link_libraries(accreta_c_example PRIVATE
            accreta_ffi
            ws2_32
            userenv
            bcrypt
            ntdll
        )
    else()
        target_link_libraries(accreta_c_example PRIVATE accreta_ffi)
    endif()
else()
    if(WIN32)
        set(ACCRETA_RUNTIME_LIBRARY "${ACCRETA_RELEASE_DIR}/accreta_ffi.dll")

        if(MSVC)
            set(ACCRETA_IMPORT_LIBRARY "${ACCRETA_RELEASE_DIR}/accreta_ffi.dll.lib")
        else()
            set(ACCRETA_IMPORT_LIBRARY "${ACCRETA_RELEASE_DIR}/libaccreta_ffi.dll.a")
        endif()

        if(NOT EXISTS "${ACCRETA_RUNTIME_LIBRARY}")
            message(FATAL_ERROR
                "accreta_ffi.dll was not found at ${ACCRETA_RUNTIME_LIBRARY}. "
                "Run 'cargo build --release' first.")
        endif()

        if(NOT EXISTS "${ACCRETA_IMPORT_LIBRARY}")
            message(FATAL_ERROR
                "The Windows import library was not found at "
                "${ACCRETA_IMPORT_LIBRARY}. Run 'cargo build --release' first.")
        endif()

        add_library(accreta_ffi SHARED IMPORTED GLOBAL)
        set_target_properties(accreta_ffi PROPERTIES
            IMPORTED_IMPLIB "${ACCRETA_IMPORT_LIBRARY}"
            IMPORTED_LOCATION "${ACCRETA_RUNTIME_LIBRARY}"
            INTERFACE_INCLUDE_DIRECTORIES "${ACCRETA_INCLUDE_DIR}"
        )

        target_link_libraries(accreta_c_example PRIVATE accreta_ffi)

        # Keep the DLL beside the executable so it can be run directly.
        add_custom_command(TARGET accreta_c_example POST_BUILD
            COMMAND ${CMAKE_COMMAND} -E copy_if_different
                "${ACCRETA_RUNTIME_LIBRARY}"
                "$<TARGET_FILE_DIR:accreta_c_example>"
        )
    elseif(APPLE)
        set(ACCRETA_SHARED_LIBRARY "${ACCRETA_RELEASE_DIR}/libaccreta_ffi.dylib")

        if(NOT EXISTS "${ACCRETA_SHARED_LIBRARY}")
            message(FATAL_ERROR
                "libaccreta_ffi.dylib was not found at "
                "${ACCRETA_SHARED_LIBRARY}. Run 'cargo build --release' first.")
        endif()

        add_library(accreta_ffi SHARED IMPORTED GLOBAL)
        set_target_properties(accreta_ffi PROPERTIES
            IMPORTED_LOCATION "${ACCRETA_SHARED_LIBRARY}"
            INTERFACE_INCLUDE_DIRECTORIES "${ACCRETA_INCLUDE_DIR}"
        )

        target_link_libraries(accreta_c_example PRIVATE accreta_ffi)

        # Allow the example to find the repository-local dylib.
        set_target_properties(accreta_c_example PROPERTIES
            BUILD_RPATH "${ACCRETA_RELEASE_DIR}"
        )
    else()
        set(ACCRETA_SHARED_LIBRARY "${ACCRETA_RELEASE_DIR}/libaccreta_ffi.so")

        if(NOT EXISTS "${ACCRETA_SHARED_LIBRARY}")
            message(FATAL_ERROR
                "libaccreta_ffi.so was not found at "
                "${ACCRETA_SHARED_LIBRARY}. Run 'cargo build --release' first.")
        endif()

        add_library(accreta_ffi SHARED IMPORTED GLOBAL)
        set_target_properties(accreta_ffi PROPERTIES
            IMPORTED_LOCATION "${ACCRETA_SHARED_LIBRARY}"
            INTERFACE_INCLUDE_DIRECTORIES "${ACCRETA_INCLUDE_DIR}"
        )

        target_link_libraries(accreta_c_example PRIVATE accreta_ffi)

        # Allow the example to find the repository-local shared library.
        set_target_properties(accreta_c_example PROPERTIES
            BUILD_RPATH "${ACCRETA_RELEASE_DIR}"
        )
    endif()
endif()
