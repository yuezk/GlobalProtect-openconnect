cmake_minimum_required(VERSION 3.1.0)

project(GlobalProtect-openconnect VERSION 1.3.3)

set(CMAKE_CXX_STANDARD 11)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

set(CMAKE_AUTOMOC ON)
set(CMAKE_AUTORCC ON)
set(CMAKE_AUTOUIC ON)

list(APPEND CMAKE_MODULE_PATH "${CMAKE_CURRENT_SOURCE_DIR}/3rdparty")

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

CPMAddPackage(
    NAME SingleApplication
    GITHUB_REPOSITORY itay-grudev/SingleApplication
    VERSION 3.3.0
    CPM_LOCAL_PACKAGES_ONLY YES
)
CPMAddPackage(
    NAME plog
    GITHUB_REPOSITORY SergiusTheBest/plog
    GIT_TAG 1.1.5
    CPM_LOCAL_PACKAGES_ONLY YES
    # CPM_USE_LOCAL_PACKAGES YES
)

add_subdirectory(GPService)
add_subdirectory(GPClient)