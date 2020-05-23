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

int GPGateway::priorityOf(QString ruleName)
{
    if (_priorityRules.contains(ruleName)) {
        return _priorityRules.value(ruleName);
    }
    return 0;
}
