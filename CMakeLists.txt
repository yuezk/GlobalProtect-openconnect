cmake_minimum_required(VERSION 3.1.0)

project(GlobalProtect-openconnect VERSION 1.3.3)

set(CMAKE_CXX_STANDARD 11)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

set(CMAKE_AUTOMOC ON)
set(CMAKE_AUTORCC ON)
set(CMAKE_AUTOUIC ON)

set(QAPPLICATION_CLASS QApplication CACHE STRING "Inheritance class for SingleApplication")

include(cmake/CPM.cmake)

find_package(Qt5 REQUIRED COMPONENTS
    Core
    Widgets
    WebSockets
    WebEngine
    WebEngineWidgets
    DBus
)

CPMAddPackage("gh:itay-grudev/SingleApplication@3.3.0")
CPMAddPackage("gh:SergiusTheBest/plog#1.1.5")

add_subdirectory(GPService)
add_subdirectory(GPClient)