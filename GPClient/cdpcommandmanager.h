#ifndef CDPCOMMANDMANAGER_H
#define CDPCOMMANDMANAGER_H

#include "cdpcommand.h"
#include <QObject>
#include <QHash>
#include <QtWebSockets>
#include <QNetworkAccessManager>

class CDPCommandManager : public QObject
{
    Q_OBJECT
public:
    explicit CDPCommandManager(QObject *parent = nullptr);
    ~CDPCommandManager();

    void initialize(QString endpoint);

    CDPCommand *sendCommand(QString cmd);
    CDPCommand *sendCommend(QString cmd, QVariantMap& params);

signals:
    void ready();
    void eventReceived(QString eventName, QJsonObject params);

private:
    QNetworkAccessManager *networkManager;
    QWebSocket *socket;

    int commandId = 0;
    QHash<int, CDPCommand*> commandPool;

private slots:
    void onTextMessageReceived(QString message);
    void onSocketDisconnected();
    void onSocketError(QAbstractSocket::SocketError error);
};

#endif // CDPCOMMANDMANAGER_H
