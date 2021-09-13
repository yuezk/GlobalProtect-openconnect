project(GPService)

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

install(TARGETS gpservice DESTINATION "/usr/bin")
install(FILES "dbus/com.yuezk.qt.GPService.conf" DESTINATION "/usr/share/dbus-1/system.d" )
install(FILES "dbus/com.yuezk.qt.GPService.service" DESTINATION "/usr/share/dbus-1/system-services")
install(FILES "systemd/gpservice.service" DESTINATION "/etc/systemd/system")
