TARGET = gpclient

QT       += core gui network websockets dbus webenginewidgets

greaterThan(QT_MAJOR_VERSION, 4): QT += widgets

CONFIG += c++11

include(../singleapplication/singleapplication.pri)
DEFINES += QAPPLICATION_CLASS=QApplication

# The following define makes your compiler emit warnings if you use
# any Qt feature that has been marked deprecated (the exact warnings
# depend on your compiler). Please consult the documentation of the
# deprecated API in order to know how to port your code away from it.
DEFINES += QT_DEPRECATED_WARNINGS

INCLUDEPATH += ../plog/include

# You can also make your code fail to compile if it uses deprecated APIs.
# In order to do so, uncomment the following line.
# You can also select to disable deprecated APIs only up to a certain version of Qt.
#DEFINES += QT_DISABLE_DEPRECATED_BEFORE=0x060000    # disables all the APIs deprecated before Qt 6.0.0
SOURCES += \
    cdpcommand.cpp \
    cdpcommandmanager.cpp \
    enhancedwebview.cpp \
    gatewayauthenticator.cpp \
    gatewayauthenticatorparams.cpp \
    gpgateway.cpp \
    gphelper.cpp \
    loginparams.cpp \
    main.cpp \
    normalloginwindow.cpp \
    portalauthenticator.cpp \
    portalconfigresponse.cpp \
    preloginresponse.cpp \
    samlloginwindow.cpp \
    gpclient.cpp \
    settingsdialog.cpp

HEADERS += \
    cdpcommand.h \
    cdpcommandmanager.h \
    enhancedwebview.h \
    gatewayauthenticator.h \
    gatewayauthenticatorparams.h \
    gpgateway.h \
    gphelper.h \
    loginparams.h \
    normalloginwindow.h \
    portalauthenticator.h \
    portalconfigresponse.h \
    preloginresponse.h \
    samlloginwindow.h \
    gpclient.h \
    settingsdialog.h

FORMS += \
    gpclient.ui \
    normalloginwindow.ui \
    settingsdialog.ui

DBUS_INTERFACES += ../GPService/gpservice.xml

# Default rules for deployment.
target.path = /usr/bin
INSTALLS += target

DISTFILES += \
    com.yuezk.qt.GPClient.svg \
    com.yuezk.qt.gpclient.desktop

desktop_entry.path = /usr/share/applications/
desktop_entry.files = com.yuezk.qt.gpclient.desktop

desktop_icon.path = /usr/share/pixmaps/
desktop_icon.files = com.yuezk.qt.GPClient.svg

INSTALLS += desktop_entry desktop_icon

RESOURCES += \
    resources.qrc
