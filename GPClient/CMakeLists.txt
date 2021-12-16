include("${CMAKE_SOURCE_DIR}/cmake/Add3rdParty.cmake")

project(GPClient)

set(gpclient_GENERATED_SOURCES)

configure_file(com.yuezk.qt.gpclient.desktop.in com.yuezk.qt.gpclient.desktop)
configure_file(com.yuezk.qt.gpclient.metainfo.xml.in com.yuezk.qt.gpclient.metainfo.xml)

qt5_add_dbus_interface(
    gpclient_GENERATED_SOURCES
    ${CMAKE_BINARY_DIR}/com.yuezk.qt.GPService.xml
    gpserviceinterface
)

add_executable(gpclient
    cdpcommand.cpp
    cdpcommandmanager.cpp
    enhancedwebview.cpp
    gatewayauthenticator.cpp
    gatewayauthenticatorparams.cpp
    gpgateway.cpp
    gphelper.cpp
    loginparams.cpp
    main.cpp
    normalloginwindow.cpp
    portalauthenticator.cpp
    portalconfigresponse.cpp
    preloginresponse.cpp
    samlloginwindow.cpp
    gpclient.cpp
    settingsdialog.cpp
    gpclient.ui
    normalloginwindow.ui
    settingsdialog.ui
    vpn_dbus.cpp
    vpn_json.cpp
    resources.qrc
    ${gpclient_GENERATED_SOURCES}
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
        -DQAPPLICATION_CLASS=QApplication
)

add_3rdparty(
    plog
    GIT_REPOSITORY https://github.com/SergiusTheBest/plog.git
    GIT_TAG master
    CMAKE_ARGS
        -DPLOG_BUILD_SAMPLES=OFF
        -DCMAKE_BUILD_TYPE=${CMAKE_BUILD_TYPE}
        -DCMAKE_CXX_FLAGS_RELEASE=${CMAKE_CXX_FLAGS_RELEASE}
)

ExternalProject_Get_Property(SingleApplication-${PROJECT_NAME} SOURCE_DIR BINARY_DIR)
set(SingleApplication_INCLUDE_DIR ${SOURCE_DIR})
set(SingleApplication_LIBRARY ${BINARY_DIR}/libSingleApplication.a)

ExternalProject_Get_Property(plog-${PROJECT_NAME} SOURCE_DIR)
set(plog_INCLUDE_DIR "${SOURCE_DIR}/include")

add_dependencies(gpclient SingleApplication-${PROJECT_NAME} plog-${PROJECT_NAME})

target_include_directories(gpclient PRIVATE
    ${CMAKE_BINARY_DIR}
    ${CMAKE_CURRENT_SOURCE_DIR}
    ${CMAKE_CURRENT_BINARY_DIR}
    ${SingleApplication_INCLUDE_DIR}
    ${plog_INCLUDE_DIR}
)

target_link_libraries(gpclient
    ${SingleApplication_LIBRARY}
    Qt5::Widgets
    Qt5::Network
    Qt5::WebSockets
    Qt5::WebEngine
    Qt5::WebEngineWidgets
    Qt5::DBus
    QtSignals
)

if (CMAKE_CXX_COMPILER_VERSION VERSION_GREATER 8.0)
    target_compile_options(gpclient PUBLIC "-ffile-prefix-map=${CMAKE_SOURCE_DIR}=.")
endif()

target_compile_definitions(gpclient PUBLIC QAPPLICATION_CLASS=QApplication)

install(TARGETS gpclient DESTINATION bin)
install(FILES "${CMAKE_CURRENT_BINARY_DIR}/com.yuezk.qt.gpclient.metainfo.xml" DESTINATION share/metainfo)
install(FILES "${CMAKE_CURRENT_BINARY_DIR}/com.yuezk.qt.gpclient.desktop" DESTINATION share/applications)
install(FILES "com.yuezk.qt.gpclient.svg" DESTINATION share/icons/hicolor/scalable/apps)
