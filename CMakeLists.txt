cmake_minimum_required(VERSION 3.19.0)
project(libchewing)

set(LIBCHEWING_VERSION 0.5.1)
set(PACKAGE_VERSION ${LIBCHEWING_VERSION})
set(LIBCHEWING_BINARY_VERSION 1.0.0)
set(CMAKE_MODULE_PATH ${PROJECT_SOURCE_DIR}/cmake)

enable_testing()

if(UNIX)
    set(CMAKE_C_FLAGS "-g -O2 -Wall -fPIC ${CMAKE_C_FLAGS}")
    add_compile_definitions(UNDER_POSIX PIC)
endif()

include(CheckCCompilerFlag)

option(BUILD_DLL "Build dynamic link library (*.dll) instead of static lib." true)

if(${CMAKE_C_COMPILER_ID} STREQUAL "GNU" OR
    ${CMAKE_C_COMPILER_ID} STREQUAL "Clang")
    set(CMAKE_C_FLAGS "-std=gnu99 ${CMAKE_C_FLAGS}")
    add_compile_definitions(_GNU_SOURCE)
    option(ENABLE_GCOV "Coverage support" false)
    if(ENABLE_GCOV)
        set(CMAKE_C_FLAGS "-coverage ${CMAKE_C_FLAGS}")
    endif()

    # Use NO_UNDEFINED=no when running with address sanitizer
    option(NO_UNDEFINED "No undefined symbol in object file" true)
    if(NO_UNDEFINED)
        set(saved_CMAKE_REQUIRED_FLAGS ${CMAKE_REQUIRED_FLAGS})
        set(CMAKE_REQUIRED_FLAGS "-Wl,--no-undefined")
        check_c_compiler_flag("" HAVE_NO_UNDEFINED)
        set(CMAKE_REQUIRED_FLAGS ${saved_CMAKE_REQUIRED_FLAGS})

        if(HAVE_NO_UNDEFINED)
            set(CMAKE_SHARED_LINKER_FLAGS "-Wl,--no-undefined ${CMAKE_SHARED_LINKER_FLAGS}")
        endif()
    endif()
elseif(MSVC)
    # /wd4819
    # Without BOM, Visual Studio does not treat source file as UTF-8
    # encoding, thus it will complain about invalid character. Use
    # /wd4819 can suppress this warning.
    set(CMAKE_C_FLAGS "/wd4819 ${CMAKE_C_FLAGS}")
    add_compile_definitions(_CRT_SECURE_NO_WARNINGS _CRT_NONSTDC_NO_DEPRECATE)
    add_compile_definitions(__func__=__FUNCTION__)

    # MSVC 2015 supports `snprintf`, so no need to redefine it
    if(MSVC_VERSION LESS 1900)
        add_compile_definitions(snprintf=_snprintf)
    endif()

    set(BUILD_DLL false)
endif()

check_c_compiler_flag(-fvisibility=hidden FVISIBILITY_HIDDEN)
if(${FVISIBILITY_HIDDEN})
    set(CMAKE_C_FLAGS "-fvisibility=hidden ${CMAKE_C_FLAGS}")
endif()

# automake compatibility
add_compile_definitions(HAVE_CONFIG_H=1)
add_custom_target(check COMMAND ${CMAKE_CTEST_COMMAND})

option(WITH_SQLITE3 "Use sqlite3 to store userphrase" true)
option(WITH_INTERNAL_SQLITE3 "Use internal sqlite3" false)
if(MSVC)
    set(WITH_INTERNAL_SQLITE3 true)
endif()

if(CMAKE_SYSTEM_NAME MATCHES "Android")
    set(WITH_INTERNAL_SQLITE3 true)
endif()

# Use valgrind when testing
option(USE_VALGRIND "Use valgrind when testing" true)

option(WITH_RUST "Use rust implemented internals (experimental)" false)
if (WITH_RUST)
    add_subdirectory(cmake/corrosion)
    corrosion_import_crate(MANIFEST_PATH Cargo.toml NO_LINKER_OVERRIDE)
    add_compile_definitions(WITH_RUST)
endif()

# Feature probe
include(CheckTypeSize)
check_type_size(uint16_t UINT16_T)

set(CURSES_NEED_WIDE true)
find_package(Curses)

if (WITH_SQLITE3)
    if (WITH_INTERNAL_SQLITE3)
        set(SQLITE3_SRC_DIR ${PROJECT_SOURCE_DIR}/thirdparty/sqlite-amalgamation)
        include_directories(
            ${SQLITE3_SRC_DIR}
        )
    else()
        find_package(SQLite3 REQUIRED)
        include_directories(SQLite3_INCLUDE_DIRS)
    endif()
endif()

include(CheckFunctionExists)
check_function_exists(strtok_r HAVE_STRTOK_R)
check_function_exists(asprintf HAVE_ASPRINTF)

include(CheckIncludeFiles)
check_include_files(unistd.h HAVE_UNISTD_H)
check_include_files(stdint.h HAVE_STDINT_H)

include(TestBigEndian)
test_big_endian(WORDS_BIGENDIAN)

set(SRC_DIR ${PROJECT_SOURCE_DIR}/src)
set(INC_DIR ${PROJECT_SOURCE_DIR}/include)
set(TOOLS_SRC_DIR ${PROJECT_SOURCE_DIR}/src/tools)
set(TOOLS_BIN_DIR ${PROJECT_BINARY_DIR}/src/tools)
set(DATA_SRC_DIR ${PROJECT_SOURCE_DIR}/data)
set(DATA_BIN_DIR ${PROJECT_BINARY_DIR}/data)
set(TEST_SRC_DIR ${PROJECT_SOURCE_DIR}/test)
set(TEST_BIN_DIR ${PROJECT_BINARY_DIR}/test)

include(GNUInstallDirs)

configure_file(
    ${PROJECT_SOURCE_DIR}/cmake/config.h.in
    ${PROJECT_BINARY_DIR}/include/config.h
)

configure_file(
    ${PROJECT_SOURCE_DIR}/cmake/version.texi.in
    ${PROJECT_BINARY_DIR}/doc/version.texi
)

set(prefix "${CMAKE_INSTALL_PREFIX}")
set(exec_prefix "\${prefix}")
set(libdir "\${exec_prefix}/lib")
set(includedir "\${prefix}/include")
set(datarootdir "\${prefix}/share")
set(datadir "\${datarootdir}")
set(sysconfdir "\${prefix}/etc")
configure_file(
    ${PROJECT_SOURCE_DIR}/chewing.pc.in
    ${PROJECT_BINARY_DIR}/chewing.pc
    @ONLY
)

include_directories(
    ${PROJECT_BINARY_DIR}/include
    ${PROJECT_SOURCE_DIR}/include
    ${PROJECT_SOURCE_DIR}/include/internal
    ${PROJECT_SOURCE_DIR}/src
    ${PROJECT_SOURCE_DIR}/src/porting_layer/include
)

set(ALL_INC
    ${INC_DIR}/chewing.h
    ${INC_DIR}/chewing-compat.h
    ${INC_DIR}/chewingio.h
    ${INC_DIR}/global.h
    ${INC_DIR}/mod_aux.h
)

if (WITH_RUST)
    include_directories(capi/chewing-public/include)
    include_directories(capi/chewing-internal/include)
    list(APPEND ALL_INC capi/chewing-public/include/chewing_rs.h)
endif()

add_subdirectory(doc)
add_subdirectory(test)
add_subdirectory(data)

# library
add_library(common OBJECT
    src/porting_layer/src/asprintf.h

    src/porting_layer/src/asprintf.c
    src/chewing.c
)
target_compile_definitions(common PRIVATE
    CHEWING_DATADIR=\"${CMAKE_INSTALL_FULL_DATADIR}/libchewing\"
)

add_library(chewing OBJECT
    ${ALL_INC}
    ${INC_DIR}/internal/chewing-private.h
    ${INC_DIR}/internal/chewingutil.h
    ${INC_DIR}/internal/choice-private.h
    ${INC_DIR}/internal/global-private.h

    ${SRC_DIR}/compat.c
    ${SRC_DIR}/chewingio.c
    ${SRC_DIR}/chewingutil.c
    ${SRC_DIR}/choice.c
    ${SRC_DIR}/mod_aux.c
    ${SRC_DIR}/private.h
)
if (NOT WITH_RUST)
target_sources(common PRIVATE
    ${SRC_DIR}/porting_layer/include/plat_mmap.h
    ${SRC_DIR}/porting_layer/include/plat_path.h
    ${SRC_DIR}/porting_layer/include/plat_types.h
    ${SRC_DIR}/porting_layer/include/sys/plat_posix.h
    ${SRC_DIR}/porting_layer/include/sys/plat_windows.h

    ${SRC_DIR}/porting_layer/src/plat_mmap_posix.c
    ${SRC_DIR}/porting_layer/src/plat_mmap_windows.c
    ${SRC_DIR}/porting_layer/src/plat_path.c
    ${SRC_DIR}/porting_layer/src/rpl_malloc.c
    ${SRC_DIR}/common/chewing-utf8-util.c
    ${SRC_DIR}/common/key2pho.c
)
target_sources(chewing PRIVATE
    ${INC_DIR}/internal/dict-private.h
    ${INC_DIR}/internal/pinyin-private.h
    ${INC_DIR}/internal/tree-private.h
    ${INC_DIR}/internal/userphrase-private.h
    ${INC_DIR}/internal/bopomofo-private.h

    ${SRC_DIR}/dict.c
    ${SRC_DIR}/pinyin.c
    ${SRC_DIR}/tree.c
    ${SRC_DIR}/userphrase.c
    ${SRC_DIR}/bopomofo.c
)
endif()
target_compile_definitions(chewing PRIVATE
    CHEWING_DATADIR=\"${CMAKE_INSTALL_FULL_DATADIR}/libchewing\"
)

if (WITH_INTERNAL_SQLITE3)
    find_package (Threads)
    add_library(sqlite3_library STATIC
        ${SQLITE3_SRC_DIR}/sqlite3.c
        ${SQLITE3_SRC_DIR}/sqlite3.h
    )
    target_link_libraries(sqlite3_library ${CMAKE_DL_LIBS} ${CMAKE_THREAD_LIBS_INIT})

    add_executable(sqlite3
        ${SQLITE3_SRC_DIR}/shell.c
    )
    target_link_libraries(sqlite3 sqlite3_library)
    set_target_properties(sqlite3 PROPERTIES
        RUNTIME_OUTPUT_DIRECTORY ${SQLITE3_SRC_DIR}
        RUNTIME_OUTPUT_DIRECTORY_DEBUG ${SQLITE3_SRC_DIR}
        RUNTIME_OUTPUT_DIRECTORY_RELEASE ${SQLITE3_SRC_DIR}
        RUNTIME_OUTPUT_DIRECTORY_RELEASE ${SQLITE3_SRC_DIR}
        RUNTIME_OUTPUT_DIRECTORY_RELWITHDEBINFO ${SQLITE3_SRC_DIR}
    )
    set(SQLite3_LIBRARIES sqlite3_library)
endif()

if (WITH_SQLITE3)
    if (NOT WITH_RUST)
        add_library(userphrase STATIC
            ${INC_DIR}/internal/chewing-sql.h

            ${SRC_DIR}/chewing-sql.c
            ${SRC_DIR}/userphrase-sql.c
        )
    endif()
else()
    add_library(userphrase STATIC
        ${INC_DIR}/internal/hash-private.h

        ${SRC_DIR}/hash.c
        ${SRC_DIR}/userphrase-hash.c
    )
endif()

if (BUILD_DLL OR NOT MSVC)
    if (MSVC)
        add_compile_definitions(CHEWINGDLL_EXPORTS)
    endif()
    add_library(chewing_shared SHARED
        $<TARGET_OBJECTS:chewing>
        $<TARGET_OBJECTS:common>
    )
    if (WITH_RUST)
        corrosion_set_env_vars(chewing-public CMAKE_BINARY_DIR=${CMAKE_BINARY_DIR})
        if (${CMAKE_C_COMPILER_ID} STREQUAL "GNU")
            target_link_options(chewing_shared PRIVATE "-Wl,-version-script,${CMAKE_BINARY_DIR}/symbols.map")
            target_link_options(chewing_shared PRIVATE "-Wl,--gc-sections")
        elseif (${CMAKE_C_COMPILER_ID} STREQUAL "AppleClang")
            target_link_options(chewing_shared PRIVATE "-Wl,-exported_symbols_list,${CMAKE_BINARY_DIR}/symbols.map")
            target_link_options(chewing_shared PRIVATE "-Wl,-dead_strip")
        elseif (MSVC)
            target_link_options(chewing_shared PRIVATE "/DEF ${CMAKE_BINARY_DIR}/symbols.map")
            set_target_properties(chewing_shared PROPERTIES MSVC_RUNTIME_LIBRARY "MultiThreadedDLL")
        endif()
    endif()

    list(APPEND LIBS chewing_shared)
    add_dependencies(check chewing_shared)
endif()

if (NOT BUILD_DLL)
    add_library(chewing_static STATIC
        $<TARGET_OBJECTS:chewing>
        $<TARGET_OBJECTS:common>
    )
    list(APPEND LIBS chewing_static)
    add_dependencies(check chewing_static)
endif()

foreach(lib ${LIBS})
    if (WITH_RUST)
        target_link_libraries(${lib} PRIVATE chewing-public)
        target_link_libraries(${lib} PRIVATE chewing-internal)
        if (WITH_SQLITE3)
            target_link_libraries(chewing-internal INTERFACE ${SQLite3_LIBRARIES})
        endif()
    else()
        target_link_libraries(${lib} userphrase)
        if (WITH_SQLITE3)
            target_link_libraries(userphrase ${SQLite3_LIBRARIES})
        endif()
    endif()
endforeach()

set_target_properties(${LIBS} PROPERTIES
    OUTPUT_NAME chewing
    SOVERSION 3
    VERSION 3.3.1
)

# install
install(FILES ${ALL_INC} DESTINATION ${CMAKE_INSTALL_INCLUDEDIR}/chewing)
install(FILES ${PROJECT_BINARY_DIR}/chewing.pc
    DESTINATION ${CMAKE_INSTALL_LIBDIR}/pkgconfig)
install(TARGETS ${LIBS} DESTINATION ${CMAKE_INSTALL_LIBDIR})
