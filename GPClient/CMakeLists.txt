include("${CMAKE_SOURCE_DIR}/cmake/Add3rdParty.cmake")

project(GPClient)

set(gpclient_GENERATED_SOURCES)

qt5_add_dbus_interface(gpclient_GENERATED_SOURCES ${CMAKE_BINARY_DIR}/com.yuezk.qt.GPService.xml gpserviceinterface)

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
    resources.qrc
    ${gpclient_GENERATED_SOURCES}
)

add_3rdparty(
    SingleApplication
    GIT_REPOSITORY https://github.com/itay-grudev/SingleApplication.git
    GIT_TAG v3.3.0
    CMAKE_ARGS -DQAPPLICATION_CLASS=QApplication
)

add_3rdparty(
    plog
    GIT_REPOSITORY https://github.com/SergiusTheBest/plog.git
    GIT_TAG 1.1.5
    CMAKE_ARGS -DPLOG_BUILD_SAMPLES=OFF
)

ExternalProject_Get_Property(SingleApplication-${PROJECT_NAME} SOURCE_DIR BINARY_DIR)
set(SingleApplication_INCLUDE_DIR ${SOURCE_DIR})
set(SingleApplication_LIBRARY ${BINARY_DIR}/libSingleApplication.a)

ExternalProject_Get_Property(plog-${PROJECT_NAME} SOURCE_DIR)
set(plog_INCLUDE_DIR "${SOURCE_DIR}/include")

add_dependencies(gpclient SingleApplication-${PROJECT_NAME} plog-${PROJECT_NAME})

target_include_directories(gpclient PRIVATE
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
)

# target_compile_definitions(gpclient PUBLIC QAPPLICATION_CLASS=QApplication)

install(TARGETS gpclient DESTINATION "/usr/bin")
install(FILES com.yuezk.qt.gpclient.desktop DESTINATION "/usr/share/applications")
install(FILES com.yuezk.qt.GPClient.svg DESTINATION "/usr/share/pixmaps")