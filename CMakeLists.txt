cmake_minimum_required(VERSION 3.21.2)

project(GlobalProtect-openconnect VERSION 1.3.3 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 11)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

set(CMAKE_AUTOMOC ON)
set(CMAKE_AUTORCC ON)
set(CMAKE_AUTOUIC ON)

find_package(Qt5 REQUIRED COMPONENTS
    Core
    Widgets
    Network
    WebSockets
    WebEngine
    WebEngineWidgets
    DBus
)

add_subdirectory(GPService)
add_subdirectory(GPClient)
