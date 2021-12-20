#ifndef VPN_JSON_H
#define VPN_JSON_H
#include "vpn.h"

class VpnJson : public QObject, public IVpn
{
  Q_OBJECT
  Q_INTERFACES(IVpn)

public:
  VpnJson(QObject *parent) : QObject(parent) {}

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
