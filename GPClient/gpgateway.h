#ifndef GPGATEWAY_H
#define GPGATEWAY_H

#include <QString>
#include <QMap>

class GPGateway
{
public:
    GPGateway();

    QString name() const;
    QString address() const;

    void setName(const QString &name);
    void setAddress(const QString &address);
    void setPriorityRules(const QMap<QString, int> &priorityRules);
    int priorityOf(QString ruleName);

private:
    QString _name;
    QString _address;
    QMap<QString, int> _priorityRules;
};

#endif // GPGATEWAY_H
