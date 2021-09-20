cmake_minimum_required(VERSION 3.10.0)

set(CMAKE_CXX_STANDARD 11)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

set(CMAKE_AUTOMOC ON)
set(CMAKE_AUTORCC ON)
set(CMAKE_AUTOUIC ON)

file(READ "VERSION" ver)
project(GlobalProtect-openconnect VERSION ${ver} LANGUAGES CXX)

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
add_subdirectory(GPService)
add_subdirectory(GPClient)
add_dependencies(gpclient gpservice)
