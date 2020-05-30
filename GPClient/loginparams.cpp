#include "loginparams.h"

#include <QUrlQuery>

LoginParams::LoginParams()
{
    params.addQueryItem("prot", QUrl::toPercentEncoding("https:"));
    params.addQueryItem("server", "");
    params.addQueryItem("inputSrc", "");
    params.addQueryItem("jnlpReady", "jnlpReady");
    params.addQueryItem("user", "");
    params.addQueryItem("passwd", "");
    params.addQueryItem("computer", QUrl::toPercentEncoding(QSysInfo::machineHostName()));
    params.addQueryItem("ok", "Login");
    params.addQueryItem("direct", "yes");
    params.addQueryItem("clientVer", "4100");
    params.addQueryItem("os-version", QUrl::toPercentEncoding(QSysInfo::prettyProductName()));
    params.addQueryItem("clientos", "Linux");
    params.addQueryItem("portal-userauthcookie", "");
    params.addQueryItem("portal-prelogonuserauthcookie", "");
    params.addQueryItem("prelogin-cookie", "");
    params.addQueryItem("ipv6-support", "yes");
}

LoginParams::~LoginParams()
{
}

void LoginParams::setUser(const QString user)
{
    updateQueryItem("user", user);
}

void LoginParams::setServer(const QString server)
{
    updateQueryItem("server", server);
}

void LoginParams::setPassword(const QString password)
{
    updateQueryItem("passwd", password);
}

void LoginParams::setUserAuthCookie(const QString cookie)
{
    updateQueryItem("portal-userauthcookie", cookie);
}

void LoginParams::setPrelogonAuthCookie(const QString cookie)
{
    updateQueryItem("portal-prelogonuserauthcookie", cookie);
}

void LoginParams::setPreloginCookie(const QString cookie)
{
    updateQueryItem("prelogin-cookie", cookie);
}

QByteArray LoginParams::toUtf8() const
{
    return params.toString().toUtf8();
}

void LoginParams::updateQueryItem(const QString key, const QString value)
{
    if (params.hasQueryItem(key)) {
        params.removeQueryItem(key);
    }
    params.addQueryItem(key, QUrl::toPercentEncoding(value));
}
