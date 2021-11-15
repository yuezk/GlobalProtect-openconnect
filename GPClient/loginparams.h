#ifndef LOGINPARAMS_H
#define LOGINPARAMS_H

#include <QtCore/QUrlQuery>

class LoginParams
{
public:
    LoginParams(const QString clientos);
    ~LoginParams();

    void setUser(const QString user);
    void setServer(const QString server);
    void setPassword(const QString password);
    void setUserAuthCookie(const QString cookie);
    void setPrelogonAuthCookie(const QString cookie);
    void setPreloginCookie(const QString cookie);
    void setInputStr(const QString inputStr);

    QByteArray toUtf8() const;

private:
    QUrlQuery params;

    void updateQueryItem(const QString key, const QString value);
};

#endif // LOGINPARAMS_H
