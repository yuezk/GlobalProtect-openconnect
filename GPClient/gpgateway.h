#ifndef GPGATEWAY_H
#define GPGATEWAY_H

#include <QtCore/QString>
#include <QtCore/QMap>
#include <QtCore/QJsonObject>

class GPGateway
{
public:
    GPGateway();

    QString name() const;
    QString address() const;

    void setName(const QString &name);
    void setAddress(const QString &address);
    void setPriorityRules(const QMap<QString, int> &priorityRules);
    int priorityOf(QString ruleName) const;
    QJsonObject toJsonObject() const;
    QString toString() const;

    static QString serialize(QList<GPGateway> &gateways);
    static QList<GPGateway> fromJson(const QString &jsonString);
    static GPGateway fromJsonObject(const QJsonObject &jsonObj);

private:
    QString _name;
    QString _address;
    QMap<QString, int> _priorityRules;
};

#endif // GPGATEWAY_H
