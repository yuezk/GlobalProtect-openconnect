#ifndef PORTALCONFIGRESPONSE_H
#define PORTALCONFIGRESPONSE_H

#include "gpgateway.h"

#include <QString>
#include <QList>
#include <QXmlStreamReader>

class PortalConfigResponse
{
public:
    PortalConfigResponse();

    static PortalConfigResponse parse(const QByteArray& xml);

    const QByteArray& rawResponse() const;
    QString username() const;
    QString password() const;
    QString userAuthCookie() const;
    QString prelogonUserAuthCookie() const;
    QList<GPGateway>& allGateways();

    void setUsername(const QString& username);
    void setPassword(const QString& password);

private:
    static QString xmlUserAuthCookie;
    static QString xmlPrelogonUserAuthCookie;
    static QString xmlGateways;

    QByteArray _rawResponse;
    QString _username;
    QString _password;
    QString _userAuthCookie;
    QString _prelogonAuthCookie;

    QList<GPGateway> _gateways;

    void setRawResponse(const QByteArray& response);
    void setUserAuthCookie(const QString& cookie);
    void setPrelogonUserAuthCookie(const QString& cookie);
    void setGateways(const QList<GPGateway>& gateways);

    static QList<GPGateway> parseGateways(QXmlStreamReader &xmlReader);
    static QMap<QString, int> parsePriorityRules(QXmlStreamReader &xmlReader);
    static QString parseGatewayName(QXmlStreamReader &xmlReader);
};

#endif // PORTALCONFIGRESPONSE_H
