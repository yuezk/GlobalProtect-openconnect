#include "loginparams.h"

#include <QUrlQuery>

LoginParams::LoginParams()
{
}

LoginParams::~LoginParams()
{
}

void LoginParams::setUser(const QString &user)
{
    updateQueryItem("user", user);
}

void LoginParams::setServer(const QString &server)
{
    updateQueryItem("server", server);
}

void LoginParams::setPassword(const QString &password)
{
    updateQueryItem("passwd", password);
}

void LoginParams::setUserAuthCookie(const QString &cookie)
{
    updateQueryItem("portal-userauthcookie", cookie);
}

void LoginParams::setPrelogonAuthCookie(const QString &cookie)
{
    updateQueryItem("portal-prelogonuserauthcookie", cookie);
}

void LoginParams::setPreloginCookie(const QString &cookie)
{
    updateQueryItem("prelogin-cookie", cookie);
}

QByteArray LoginParams::toUtf8() const
{
    return params.toString().toUtf8();
}

void LoginParams::updateQueryItem(const QString &key, const QString &value)
{
    if (params.hasQueryItem(key)) {
        params.removeQueryItem(key);
    }
    params.addQueryItem(key, QUrl::toPercentEncoding(value));
}
