project(gpclient)

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
    ${gpclient_GENERATED_SOURCES}
)

target_include_directories(gpclient PRIVATE ${CMAKE_CURRENT_SOURCE_DIR} ${CMAKE_CURRENT_BINARY_DIR})

target_link_libraries(gpclient
    plog
    SingleApplication
    Qt5::Widgets
    Qt5::WebSockets
    Qt5::WebEngine
    Qt5::WebEngineWidgets
    Qt5::DBus
)