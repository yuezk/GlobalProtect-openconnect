#include "vpn_dbus.h"

void VpnDbus::connect(const QString &preferredServer, const QList<QString> &servers, const QString &username, const QString &passwd) {
    inner->connect(preferredServer, username, passwd);
}

void VpnDbus::disconnect() {
    inner->disconnect();
}

int VpnDbus::status() {
    return inner->status();
}
