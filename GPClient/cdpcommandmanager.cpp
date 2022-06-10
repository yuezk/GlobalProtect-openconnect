#include <QtCore/QVariantMap>
#include <plog/Log.h>

#include "cdpcommandmanager.h"

CDPCommandManager::CDPCommandManager(QObject *parent)
    : QObject(parent)
    , networkManager(new QNetworkAccessManager)
    , socket(new QWebSocket)
{
    // WebSocket setup
    QObject::connect(socket, &QWebSocket::connected, this, &CDPCommandManager::ready);
    QObject::connect(socket, &QWebSocket::textMessageReceived, this, &CDPCommandManager::onTextMessageReceived);
    QObject::connect(socket, &QWebSocket::disconnected, this, &CDPCommandManager::onSocketDisconnected);
    QObject::connect(socket, QOverload<QAbstractSocket::SocketError>::of(&QWebSocket::error), this, &CDPCommandManager::onSocketError);
}

CDPCommandManager::~CDPCommandManager()
{
    delete networkManager;
    delete socket;
}

void CDPCommandManager::initialize(QString endpoint)
{
    QNetworkReply *reply = networkManager->get(QNetworkRequest(endpoint));

    QObject::connect(
        reply, &QNetworkReply::finished,
        [reply, this]() {
            if (reply->error()) {
                LOGE << "CDP request error";
                return;
            }

            QJsonDocument doc = QJsonDocument::fromJson(reply->readAll());
            QJsonArray pages = doc.array();
            QJsonObject page = pages.first().toObject();
            QString wsUrl = page.value("webSocketDebuggerUrl").toString();

            socket->open(wsUrl);
        }
    );
}

CDPCommand *CDPCommandManager::sendCommand(QString cmd)
{
    QVariantMap emptyParams;
    return sendCommend(cmd, emptyParams);
}

CDPCommand *CDPCommandManager::sendCommend(QString cmd, QVariantMap &params)
{
    int id = ++commandId;
    CDPCommand *command = new CDPCommand(id, cmd, params);
    socket->sendTextMessage(command->toJson());
    commandPool.insert(id, command);

    return command;
}

void CDPCommandManager::onTextMessageReceived(QString message)
{
    QJsonDocument responseDoc = QJsonDocument::fromJson(message.toUtf8());
    QJsonObject response = responseDoc.object();

    // Response for method
    if (response.contains("id")) {
        int id = response.value("id").toInt();
        if (commandPool.contains(id)) {
            CDPCommand *command = commandPool.take(id);
            command->finished();
        }
    } else { // Response for event
        emit eventReceived(response.value("method").toString(), response.value("params").toObject());
    }
}

void CDPCommandManager::onSocketDisconnected()
{
    LOGI << "WebSocket disconnected";
}

void CDPCommandManager::onSocketError(QAbstractSocket::SocketError error)
{
    LOGE << "WebSocket error" << error;
}
