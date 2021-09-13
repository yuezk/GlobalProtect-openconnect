cmake_minimum_required(VERSION 3.14.0)

project(GlobalProtect-openconnect VERSION 1.3.3 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 11)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

set(CMAKE_AUTOMOC ON)
set(CMAKE_AUTORCC ON)
set(CMAKE_AUTOUIC ON)

set(QAPPLICATION_CLASS QApplication CACHE STRING "Inheritance class for SingleApplication")

find_package(Qt5 REQUIRED COMPONENTS
    Core
    Widgets
    WebSockets
    WebEngine
    WebEngineWidgets
    DBus
)

include(cmake/Add3rdParty.cmake)

add_3rdparty(
    NAME SingleApplication
    GITHUB_REPOSITORY itay-grudev/SingleApplication
    VERSION 3.3.0
    EXCLUDE_FROM_ALL
)

add_3rdparty(
    NAME plog
    GITHUB_REPOSITORY SergiusTheBest/plog
    GIT_TAG 1.1.5
    EXCLUDE_FROM_ALL
)

add_subdirectory(GPService)
add_subdirectory(GPClient)

include(InstallRequiredSystemLibraries)
include(CPack)