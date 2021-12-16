#include "vpn_json.h"
#include <QTextStream> 
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonArray>

void VpnJson::connect(const QString &preferredServer, const QList<QString> &servers, const QString &username, const QString &passwd, const QString &extraArgs) {
    QJsonArray sl;
    for (const QString &srv : servers) {
      sl.push_back(QJsonValue(srv));
    }
    QJsonObject j;
    j["server"] = preferredServer;
    j["availableServers"] = sl;
    j["cookie"] = passwd;
    QTextStream(stdout) << QJsonDocument(j).toJson(QJsonDocument::Compact) << "\n";
    emit connected();
}

void VpnJson::disconnect() { /* nop */ }

int VpnJson::status() {
    return 4; // disconnected
}
