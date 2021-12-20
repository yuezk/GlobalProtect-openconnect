#ifndef GATEWAYAUTHENTICATORPARAMS_H
#define GATEWAYAUTHENTICATORPARAMS_H

#include <QtCore/QString>

#include "portalconfigresponse.h"

class GatewayAuthenticatorParams
{
public:
    GatewayAuthenticatorParams();

    static GatewayAuthenticatorParams fromPortalConfigResponse(const PortalConfigResponse &portalConfig);

    const QString &username() const;
    void setUsername(const QString &newUsername);

    const QString &password() const;
    void setPassword(const QString &newPassword);

    const QString &userAuthCookie() const;
    void setUserAuthCookie(const QString &newUserAuthCookie);

    const QString &clientos() const;
    void setClientos(const QString &newClientos);

    const QString &inputStr() const;
    void setInputStr(const QString &inputStr);

private:
    QString m_username;
    QString m_password;
    QString m_userAuthCookie;
    QString m_clientos;
    QString m_inputStr;
};

#endif // GATEWAYAUTHENTICATORPARAMS_H
