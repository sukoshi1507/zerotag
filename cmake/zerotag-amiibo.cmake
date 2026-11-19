# SPDX-License-Identifier: MIT
#
# ZeroTag amiibo support: image generation/verification (src/amiibo) on top of the vendored amiitool
# (third_party/amiitool, MIT) and mbedtls. Also builds the hardware/host tools.
#
# mbedtls: a system install is used when found (CMake package "MbedTLS"), otherwise 3.6.2 is fetched
# and built as a static library (no programs, no tests). Set ZEROTAG_FETCH_MBEDTLS=OFF to forbid the fetch.

option(ZEROTAG_FETCH_MBEDTLS "Fetch and build mbedtls when it is not installed" ON)
option(ZEROTAG_BUILD_TOOLS "Build the amiibo / ST25R3916 command-line tools" ON)

set(_ZT_MBEDCRYPTO_TARGET "")
find_package(MbedTLS CONFIG QUIET)
if(TARGET MbedTLS::mbedcrypto)
    set(_ZT_MBEDCRYPTO_TARGET MbedTLS::mbedcrypto)
elseif(ZEROTAG_FETCH_MBEDTLS)
    include(FetchContent)
    set(ENABLE_PROGRAMS OFF CACHE BOOL "" FORCE)
    set(ENABLE_TESTING OFF CACHE BOOL "" FORCE)
    set(USE_SHARED_MBEDTLS_LIBRARY OFF CACHE BOOL "" FORCE)
    set(USE_STATIC_MBEDTLS_LIBRARY ON CACHE BOOL "" FORCE)
    set(MBEDTLS_FATAL_WARNINGS OFF CACHE BOOL "" FORCE)
    FetchContent_Declare(
        mbedtls
        GIT_REPOSITORY https://github.com/Mbed-TLS/mbedtls.git
        GIT_TAG        mbedtls-3.6.2
        GIT_SHALLOW    TRUE
    )
    FetchContent_MakeAvailable(mbedtls)
    set(_ZT_MBEDCRYPTO_TARGET mbedcrypto)
else()
    message(FATAL_ERROR "mbedtls not found and ZEROTAG_FETCH_MBEDTLS=OFF")
endif()

set(_AMIITOOL_DIR "${CMAKE_CURRENT_LIST_DIR}/../third_party/amiitool")

add_library(${PROJECT_NAME}_amiibo STATIC
    "${CMAKE_CURRENT_LIST_DIR}/../src/amiibo/amiibo_image.cpp"
    "${CMAKE_CURRENT_LIST_DIR}/../src/amiibo/amiibo_db.cpp"
    "${CMAKE_CURRENT_LIST_DIR}/../src/amiibo/amiibo_library.cpp"
    "${_AMIITOOL_DIR}/amiibo.c"
    "${_AMIITOOL_DIR}/drbg.c"
    "${_AMIITOOL_DIR}/keygen.c"
    "${_AMIITOOL_DIR}/util.c"
)
target_include_directories(${PROJECT_NAME}_amiibo
    PUBLIC
        "${CMAKE_CURRENT_LIST_DIR}/../src/amiibo"
    PRIVATE
        "${_AMIITOOL_DIR}"
        "${_AMIITOOL_DIR}/include"
)
target_link_libraries(${PROJECT_NAME}_amiibo PUBLIC ${_ZT_MBEDCRYPTO_TARGET})

if(ZEROTAG_BUILD_TOOLS)
    set(_TOOLS "${CMAKE_CURRENT_LIST_DIR}/../tools")
    set(_PLATFORM_SRC "${CMAKE_CURRENT_LIST_DIR}/../src/platform")

    # Host tools: they only need the amiibo library and the keys file given on the command line.
    foreach(tool amiibo_gen amiibo_verify amiibo_gen_test amiibo_library_test)
        add_executable(zerotag_${tool} "${_TOOLS}/${tool}.cpp")
        target_link_libraries(zerotag_${tool} PRIVATE ${PROJECT_NAME}_amiibo)
    endforeach()

    # Device tools (Linux only): need /dev/spidev and the Cap, run as root on the CardputerZero.
    if(CMAKE_SYSTEM_NAME STREQUAL "Linux")
        foreach(tool st25r3916_probe st25r3916_emulate)
            add_executable(zerotag_${tool}
                "${_TOOLS}/${tool}.cpp"
                "${_PLATFORM_SRC}/st25r3916.cpp"
                "${_PLATFORM_SRC}/st25r3916_emulation.cpp")
            target_include_directories(zerotag_${tool} PRIVATE "${_PLATFORM_SRC}")
        endforeach()
    endif()
endif()
