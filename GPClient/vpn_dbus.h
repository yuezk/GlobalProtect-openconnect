#ifndef VPN_DBUS_H
#define VPN_DBUS_H
#include "vpn.h"
#include "gpserviceinterface.h"

class VpnDbus : public QObject, public IVpn
{
  Q_OBJECT
  Q_INTERFACES(IVpn)

private:
  com::yuezk::qt::GPService *inner;

public:
  VpnDbus(QObject *parent) : QObject(parent) {
    inner = new com::yuezk::qt::GPService("com.yuezk.qt.GPService", "/", QDBusConnection::systemBus(), this);
    QObject::connect(inner, &com::yuezk::qt::GPService::connected, this, &VpnDbus::connected);
    QObject::connect(inner, &com::yuezk::qt::GPService::disconnected, this, &VpnDbus::disconnected);
    QObject::connect(inner, &com::yuezk::qt::GPService::error, this, &VpnDbus::error);
    QObject::connect(inner, &com::yuezk::qt::GPService::logAvailable, this, &VpnDbus::logAvailable);
  }

  void connect(const QString &preferredServer, const QList<QString> &servers, const QString &username, const QString &passwd, const QString &extraArgs);
  void disconnect();
  int status();

signals: // SIGNALS
  void connected();
  void disconnected();
  void error(const QString &errorMessage);
  void logAvailable(const QString &log);
};
#endif
