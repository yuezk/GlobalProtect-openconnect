#include "gatewayauthenticatorparams.h"

GatewayAuthenticatorParams::GatewayAuthenticatorParams()
{

}

GatewayAuthenticatorParams GatewayAuthenticatorParams::fromPortalConfigResponse(const PortalConfigResponse &portalConfig)
{
    GatewayAuthenticatorParams params;
    params.setUsername(portalConfig.username());
    params.setPassword(portalConfig.password());
    params.setUserAuthCookie(portalConfig.userAuthCookie());

    return params;
}

const QString &GatewayAuthenticatorParams::username() const
{
    return m_username;
}

void GatewayAuthenticatorParams::setUsername(const QString &newUsername)
{
    m_username = newUsername;
}

const QString &GatewayAuthenticatorParams::password() const
{
    return m_password;
}

void GatewayAuthenticatorParams::setPassword(const QString &newPassword)
{
    m_password = newPassword;
}

const QString &GatewayAuthenticatorParams::userAuthCookie() const
{
    return m_userAuthCookie;
}

void GatewayAuthenticatorParams::setUserAuthCookie(const QString &newUserAuthCookie)
{
    m_userAuthCookie = newUserAuthCookie;
}

const QString &GatewayAuthenticatorParams::clientos() const
{
    return m_clientos;
}

void GatewayAuthenticatorParams::setClientos(const QString &newClientos)
{
    m_clientos = newClientos;
}

const QString &GatewayAuthenticatorParams::inputStr() const
{
    return m_inputStr;
}

void GatewayAuthenticatorParams::setInputStr(const QString &inputStr)
{
    m_inputStr = inputStr;
}

