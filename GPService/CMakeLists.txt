project(gpservice)

set(gpservice_GENERATED_SOURCES)

# generate the dbus xml definition
qt5_generate_dbus_interface(gpservice.h ${CMAKE_BINARY_DIR}/com.yuezk.qt.GPService.xml)

# generate dbus adaptor
qt5_add_dbus_adaptor(
    gpservice_GENERATED_SOURCES
    ${CMAKE_BINARY_DIR}/com.yuezk.qt.GPService.xml
    gpservice.h
    GPService
)

add_executable(gpservice
    gpservice.cpp
    main.cpp
    sigwatch.cpp
    ${gpservice_GENERATED_SOURCES}
)

target_include_directories(gpservice PRIVATE ${CMAKE_CURRENT_SOURCE_DIR} ${CMAKE_CURRENT_BINARY_DIR})

target_link_libraries(gpservice
    SingleApplication
    Qt5::Core
    Qt5::DBus
)
