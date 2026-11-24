#
# SPDX-FileCopyrightText: 2026 M5Stack Technology CO LTD
#
# SPDX-License-Identifier: MIT
# Packaging and install layout for CardputerZero Debian builds.

include(GNUInstallDirs)

set(APP_DISPLAY_NAME "ZeroTag" CACHE STRING "Human-readable application name used by launchers and package filename")
set(APP_DEBIAN_REVISION "rev1" CACHE STRING "Debian package revision/vendor suffix")
set(APP_DEBIAN_ARCHITECTURE "arm64" CACHE STRING "Debian package architecture")
# Contact used in the .deb control file (GitHub noreply address, no personal e-mail).
set(APP_MAINTAINER "sukoshi1507 <17269210+sukoshi1507@users.noreply.github.com>" CACHE STRING "Debian package maintainer")
set(APP_PACKAGE_DESCRIPTION "Amiibo emulator for CardputerZero (ST25R3916 NFC)" CACHE STRING "Debian package summary")
# Off: store apps must not ship systemd services that run as root, and ZeroTag runs from APPLaunch.
set(APP_INSTALL_SYSTEMD_SERVICE OFF CACHE BOOL "Install a systemd service file for embedded deployments")

set(APP_GENERATED_DIR "${CMAKE_CURRENT_BINARY_DIR}/generated/package")
configure_file(
    "${CMAKE_CURRENT_LIST_DIR}/templates/app.desktop.in"
    "${APP_GENERATED_DIR}/${PROJECT_NAME}.desktop"
    @ONLY
)

if(APP_INSTALL_SYSTEMD_SERVICE)
    configure_file(
        "${CMAKE_CURRENT_LIST_DIR}/templates/app.service.in"
        "${APP_GENERATED_DIR}/${PROJECT_NAME}.service"
        @ONLY
    )
endif()

install(TARGETS ${PROJECT_NAME}
    RUNTIME DESTINATION ${CMAKE_INSTALL_BINDIR}
)

install(FILES "${ZEROTAG_ROOT}/config/zerotag.conf"
    DESTINATION "/etc"
)

install(DIRECTORY "${ZEROTAG_ROOT}/assets/audio/"
    DESTINATION "${CMAKE_INSTALL_DATADIR}/${APP_NAME}/audio"
    PATTERN ".DS_Store" EXCLUDE
)
# The amiibo thumbnails (artwork owned by Nintendo, fetched by tools/fetch_amiibo_images.py) are not part of the
# source repository. They are packaged in the .deb whenever the folder exists on the build machine, so installed
# apps show them; a clone without the folder builds a package without thumbnails.
if(IS_DIRECTORY "${ZEROTAG_ROOT}/assets/amiibo/images")
    set(_ZT_BUNDLE_IMAGES_DEFAULT ON)
else()
    set(_ZT_BUNDLE_IMAGES_DEFAULT OFF)
endif()
option(ZEROTAG_BUNDLE_AMIIBO_IMAGES "Package assets/amiibo/images" ${_ZT_BUNDLE_IMAGES_DEFAULT})
if(ZEROTAG_BUNDLE_AMIIBO_IMAGES)
    install(DIRECTORY "${ZEROTAG_ROOT}/assets/amiibo/"
        DESTINATION "${CMAKE_INSTALL_DATADIR}/${APP_NAME}/amiibo"
        PATTERN ".DS_Store" EXCLUDE
    )
else()
    install(DIRECTORY "${ZEROTAG_ROOT}/assets/amiibo/"
        DESTINATION "${CMAKE_INSTALL_DATADIR}/${APP_NAME}/amiibo"
        PATTERN ".DS_Store" EXCLUDE
        PATTERN "images" EXCLUDE
    )
endif()
install(DIRECTORY "${ZEROTAG_ROOT}/assets/fonts/"
    DESTINATION "${CMAKE_INSTALL_DATADIR}/${APP_NAME}/fonts"
    PATTERN ".DS_Store" EXCLUDE
)
install(DIRECTORY "${ZEROTAG_ROOT}/assets/images/"
    DESTINATION "${CMAKE_INSTALL_DATADIR}/${APP_NAME}/images"
    PATTERN ".DS_Store" EXCLUDE
)
install(DIRECTORY "${ZEROTAG_ROOT}/assets/images/"
    DESTINATION "${CMAKE_INSTALL_DATADIR}/APPLaunch/share/images"
    FILES_MATCHING
    PATTERN "zerotag*.png"
)

install(FILES "${APP_GENERATED_DIR}/${PROJECT_NAME}.desktop"
    DESTINATION "${CMAKE_INSTALL_DATADIR}/APPLaunch/applications"
)

if(APP_INSTALL_SYSTEMD_SERVICE)
    install(FILES "${APP_GENERATED_DIR}/${PROJECT_NAME}.service"
        DESTINATION "${CMAKE_INSTALL_LIBDIR}/systemd/system"
    )
endif()

# Helper that loads the SPI overlay the Cap needs; the user runs it with sudo after each boot.
install(PROGRAMS "${ZEROTAG_ROOT}/packaging/cap-setup"
    DESTINATION "${CMAKE_INSTALL_LIBDIR}/zerotag"
)

install(FILES "${ZEROTAG_ROOT}/README.md"
    DESTINATION "${CMAKE_INSTALL_DOCDIR}"
)
install(FILES "${ZEROTAG_ROOT}/assets/fonts/LICENSE.txt"
    DESTINATION "${CMAKE_INSTALL_DOCDIR}"
    RENAME "third-party-assets-license.txt"
)

set(CPACK_GENERATOR "DEB")
set(CPACK_PACKAGING_INSTALL_PREFIX "/usr")
set(CPACK_OUTPUT_FILE_PREFIX "${ZEROTAG_ROOT}/dist")
set(CPACK_PACKAGE_NAME "${APP_DISPLAY_NAME}")
set(CPACK_PACKAGE_VENDOR "${APP_MAINTAINER}")
set(CPACK_PACKAGE_CONTACT "${APP_MAINTAINER}")
set(CPACK_PACKAGE_DESCRIPTION_SUMMARY "${APP_PACKAGE_DESCRIPTION}")
set(CPACK_PACKAGE_VERSION "${PROJECT_VERSION}")
set(CPACK_PACKAGE_FILE_NAME "${APP_DISPLAY_NAME}_${PROJECT_VERSION}_${APP_DEBIAN_REVISION}_${APP_DEBIAN_ARCHITECTURE}")
set(CPACK_RESOURCE_FILE_LICENSE "${ZEROTAG_ROOT}/assets/fonts/LICENSE.txt")

string(TOLOWER "${APP_DISPLAY_NAME}" APP_DEBIAN_PACKAGE_NAME)
string(REGEX REPLACE "[^a-z0-9+.-]" "-" APP_DEBIAN_PACKAGE_NAME "${APP_DEBIAN_PACKAGE_NAME}")
set(CPACK_DEBIAN_PACKAGE_NAME "${APP_DEBIAN_PACKAGE_NAME}")
set(CPACK_DEBIAN_PACKAGE_VERSION "${PROJECT_VERSION}")
set(CPACK_DEBIAN_PACKAGE_RELEASE "${APP_DEBIAN_REVISION}")
set(CPACK_DEBIAN_PACKAGE_ARCHITECTURE "${APP_DEBIAN_ARCHITECTURE}")
set(CPACK_DEBIAN_PACKAGE_MAINTAINER "${APP_MAINTAINER}")
set(CPACK_DEBIAN_PACKAGE_SECTION "utils")
set(CPACK_DEBIAN_PACKAGE_PRIORITY "optional")
set(APP_DEBIAN_DEPENDS "libc6, libstdc++6, libgcc-s1, libfreetype6, libpng16-16, libjpeg62-turbo, zlib1g")
if(FMT_INCLUDE_DIR AND FMT_LIBRARY)
    string(APPEND APP_DEBIAN_DEPENDS ", libfmt10")
endif()
set(CPACK_DEBIAN_PACKAGE_DEPENDS "${APP_DEBIAN_DEPENDS}")
set(CPACK_DEBIAN_PACKAGE_SHLIBDEPS OFF)
set(CPACK_DEBIAN_PACKAGE_CONTROL_STRICT_PERMISSION TRUE)

configure_file(
    "${CMAKE_CURRENT_LIST_DIR}/templates/conffiles.in"
    "${APP_GENERATED_DIR}/conffiles"
    @ONLY
)
# No maintainer scripts: the store rejects scripts matching its "dangerous pattern" list (e.g. rm under /etc).
set(CPACK_DEBIAN_PACKAGE_CONTROL_EXTRA "${APP_GENERATED_DIR}/conffiles")

include(CPack)
