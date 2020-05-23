#ifndef LOGINPARAMS_H
#define LOGINPARAMS_H

#include <QUrlQuery>

class LoginParams
{
public:
    LoginParams();
    ~LoginParams();

    void setUser(const QString &user);
    void setServer(const QString &server);
    void setPassword(const QString &password);
    void setUserAuthCookie(const QString &cookie);
    void setPrelogonAuthCookie(const QString &cookie);
    void setPreloginCookie(const QString &cookie);

    QByteArray toUtf8() const;

private:
    QUrlQuery params {
        {"prot", QUrl::toPercentEncoding("https:")},
        {"server", ""},
        {"inputSrc", ""},
        {"jnlpReady", "jnlpReady"},
        {"user", ""},
        {"passwd", ""},
        {"computer", QUrl::toPercentEncoding(QSysInfo::machineHostName())},
        {"ok", "Login"},
        {"direct", "yes"},
        {"clientVer", "4100"},
        {"os-version", QUrl::toPercentEncoding(QSysInfo::prettyProductName())},
        {"clientos", "Linux"},
        {"portal-userauthcookie", ""},
        {"portal-prelogonuserauthcookie", ""},
        {"prelogin-cookie", ""},
        {"ipv6-support", "yes"}
    };

    void updateQueryItem(const QString &key, const QString &value);
};

#endif // LOGINPARAMS_H
