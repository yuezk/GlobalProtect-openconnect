#include "vpn_dbus.h"

void VpnDbus::connect(const QString &preferredServer, const QList<QString> &servers, const QString &username, const QString &passwd, const QString &extraArgs) {
    inner->connect(preferredServer, username, passwd, extraArgs);
}

void VpnDbus::disconnect() {
    inner->disconnect();
}

int VpnDbus::status() {
    return inner->status();
}
