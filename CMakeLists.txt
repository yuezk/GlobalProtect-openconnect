cmake_minimum_required(VERSION 3.10.0)

set(CMAKE_MODULE_PATH "${CMAKE_SOURCE_DIR}/cmake;${CMAKE_MODULE_PATH}")
set(CMAKE_CXX_STANDARD 11)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

set(CMAKE_AUTOMOC ON)
set(CMAKE_AUTORCC ON)
set(CMAKE_AUTOUIC ON)

file(STRINGS "VERSION" version)
project(GlobalProtect-openconnect LANGUAGES CXX)

# Set the CMAKE_INSTALL_PREFIX to /usr if not specified
if (CMAKE_INSTALL_PREFIX_INITIALIZED_TO_DEFAULT)
    set(CMAKE_INSTALL_PREFIX "/usr" CACHE PATH "The default install prefix" FORCE)
endif()

message(STATUS "CMAKE_INSTALL_PREFIX was set to: ${CMAKE_INSTALL_PREFIX}")

configure_file(version.h.in version.h)

find_package(Qt5 REQUIRED COMPONENTS
    Core
    Widgets
    Network
    WebSockets
    WebEngine
    WebEngineWidgets
    DBus
)

add_subdirectory(3rdparty/qt-unix-signals)
add_subdirectory(3rdparty/inih)
add_subdirectory(GPService)
add_subdirectory(GPClient)
add_dependencies(gpclient gpservice)
