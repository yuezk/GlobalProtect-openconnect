#include <QtCore/QJsonObject>
#include <QtCore/QJsonDocument>
#include <QtCore/QJsonArray>

#include "gpgateway.h"

GPGateway::GPGateway()
{
}

QString GPGateway::name() const
{
    return _name;
}

QString GPGateway::address() const
{
    return _address;
}

void GPGateway::setName(const QString &name)
{
    _name = name;
}

void GPGateway::setAddress(const QString &address)
{
    _address = address;
}

void GPGateway::setPriorityRules(const QMap<QString, int> &priorityRules)
{
    _priorityRules = priorityRules;
}

int GPGateway::priorityOf(QString ruleName) const
{
    if (_priorityRules.contains(ruleName)) {
        return _priorityRules.value(ruleName);
    }
    return 0;
}

QJsonObject GPGateway::toJsonObject() const
{
    QJsonObject obj;
    obj.insert("name", name());
    obj.insert("address", address());

    return obj;
}

QString GPGateway::toString() const
{
    QJsonDocument jsonDoc{ toJsonObject() };
    return QString::fromUtf8(jsonDoc.toJson());
}

QString GPGateway::serialize(QList<GPGateway> &gateways)
{
    QJsonArray arr;

    for (auto g : gateways) {
        arr.append(g.toJsonObject());
    }

    QJsonDocument jsonDoc{ arr };
    return QString::fromUtf8(jsonDoc.toJson());
}

QList<GPGateway> GPGateway::fromJson(const QString &jsonString)
{
    QList<GPGateway> gateways;

    if (jsonString.isEmpty()) {
        return gateways;
    }

    QJsonDocument jsonDoc = QJsonDocument::fromJson(jsonString.toUtf8());

    for (auto item : jsonDoc.array()) {
        GPGateway g = GPGateway::fromJsonObject(item.toObject());
        gateways.append(g);
    }

    return gateways;
}

GPGateway GPGateway::fromJsonObject(const QJsonObject &jsonObj)
{
    GPGateway g;

    g.setName(jsonObj.value("name").toString());
    g.setAddress(jsonObj.value("address").toString());

    return g;
}
