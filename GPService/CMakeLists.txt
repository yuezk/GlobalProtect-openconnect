include("${CMAKE_SOURCE_DIR}/cmake/Add3rdParty.cmake")

project(GPService)

set(gpservice_GENERATED_SOURCES)

execute_process(COMMAND logname OUTPUT_VARIABLE CMAKE_LOGNAME)
string(STRIP "${CMAKE_LOGNAME}" CMAKE_LOGNAME)

message(STATUS "CMAKE_LOGNAME: ${CMAKE_LOGNAME}")

configure_file(dbus/com.yuezk.qt.GPService.conf.in dbus/com.yuezk.qt.GPService.conf)
configure_file(dbus/com.yuezk.qt.GPService.service.in dbus/com.yuezk.qt.GPService.service)
configure_file(systemd/gpservice.service.in systemd/gpservice.service)

# generate the dbus xml definition
qt5_generate_dbus_interface(
    gpservice.h
    ${CMAKE_BINARY_DIR}/com.yuezk.qt.GPService.xml
)

# generate dbus adaptor
qt5_add_dbus_adaptor(
    gpservice_GENERATED_SOURCES
    ${CMAKE_BINARY_DIR}/com.yuezk.qt.GPService.xml
    gpservice.h
    GPService
)

add_executable(gpservice
    gpservice.h
    gpservice.cpp
    main.cpp
    ${gpservice_GENERATED_SOURCES}
)

add_3rdparty(
    SingleApplication
    GIT_REPOSITORY https://github.com/itay-grudev/SingleApplication.git
    GIT_TAG v3.3.0
    CMAKE_ARGS
        -DCMAKE_BUILD_TYPE=${CMAKE_BUILD_TYPE}
        -DCMAKE_CXX_FLAGS_RELEASE=${CMAKE_CXX_FLAGS_RELEASE}
        -DCMAKE_FIND_ROOT_PATH=${CMAKE_FIND_ROOT_PATH}
        -DCMAKE_PREFIX_PATH=$ENV{CMAKE_PREFIX_PATH}
        -DQAPPLICATION_CLASS=QCoreApplication
)

ExternalProject_Get_Property(SingleApplication-${PROJECT_NAME} SOURCE_DIR BINARY_DIR)

set(SingleApplication_INCLUDE_DIR ${SOURCE_DIR})
set(SingleApplication_LIBRARY ${BINARY_DIR}/libSingleApplication.a)

add_dependencies(gpservice SingleApplication-${PROJECT_NAME})

target_include_directories(gpservice PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}
    ${CMAKE_CURRENT_BINARY_DIR}
    ${SingleApplication_INCLUDE_DIR}
)

target_link_libraries(gpservice
    ${SingleApplication_LIBRARY}
    Qt5::Core
    Qt5::Network
    Qt5::DBus
    QtSignals
    inih
)

target_compile_definitions(gpservice PUBLIC QAPPLICATION_CLASS=QCoreApplication)

install(TARGETS gpservice DESTINATION bin)
install(FILES "${CMAKE_CURRENT_BINARY_DIR}/dbus/com.yuezk.qt.GPService.conf" DESTINATION share/dbus-1/system.d )
install(FILES "${CMAKE_CURRENT_BINARY_DIR}/dbus/com.yuezk.qt.GPService.service" DESTINATION share/dbus-1/system-services)
install(FILES "gp.conf" DESTINATION /etc/gpservice)

if("$ENV{DEBIAN_PACKAGE}")
    # Install the systemd unit files to /lib/systemd/system for debian package
    install(FILES "${CMAKE_CURRENT_BINARY_DIR}/systemd/gpservice.service" DESTINATION /lib/systemd/system)
else()
    install(FILES "${CMAKE_CURRENT_BINARY_DIR}/systemd/gpservice.service" DESTINATION lib/systemd/system)
endif()
