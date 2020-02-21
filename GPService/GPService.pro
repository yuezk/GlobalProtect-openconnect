TARGET = gpservice

QT += dbus
QT -= gui

CONFIG += c++11 console
CONFIG -= app_bundle

include(../singleapplication/singleapplication.pri)
DEFINES += QAPPLICATION_CLASS=QCoreApplication

# The following define makes your compiler emit warnings if you use
# any Qt feature that has been marked deprecated (the exact warnings
# depend on your compiler). Please consult the documentation of the
# deprecated API in order to know how to port your code away from it.
DEFINES += QT_DEPRECATED_WARNINGS

# You can also make your code fail to compile if it uses deprecated APIs.
# In order to do so, uncomment the following line.
# You can also select to disable deprecated APIs only up to a certain version of Qt.
#DEFINES += QT_DISABLE_DEPRECATED_BEFORE=0x060000    # disables all the APIs deprecated before Qt 6.0.0

HEADERS += \
    gpservice.h \
    sigwatch.h

SOURCES += \
        gpservice.cpp \
        main.cpp \
        sigwatch.cpp

DBUS_ADAPTORS += gpservice.xml

# Default rules for deployment.
target.path = /usr/bin
INSTALLS += target

DISTFILES += \
    dbus/com.yuezk.qt.GPService.conf \
    dbus/com.yuezk.qt.GPService.service \
    systemd/gpservice.service

dbus_config.path = /usr/share/dbus-1/system.d/
dbus_config.files = dbus/com.yuezk.qt.GPService.conf

dbus_service.path = /usr/share/dbus-1/system-services/
dbus_service.files = dbus/com.yuezk.qt.GPService.service

systemd_service.path = /etc/systemd/system/
systemd_service.files = systemd/gpservice.service

INSTALLS += dbus_config dbus_service systemd_service
