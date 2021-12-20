#ifndef VPN_H
#define VPN_H
#include <QtCore/QObject>
#include <QtCore/QString>

class IVpn
{
public:
    virtual ~IVpn() = default;

    virtual void connect(const QString &preferredServer, const QList<QString> &servers, const QString &username, const QString &passwd, const QString &extraArgs) = 0;
    virtual void disconnect() = 0;
    virtual int status() = 0;

// signals: // SIGNALS
//     virtual void connected();
//     virtual void disconnected();
//     virtual void error(const QString &errorMessage);
//     virtual void logAvailable(const QString &log);
};

Q_DECLARE_INTERFACE(IVpn, "IVpn") // define this out of namespace scope

#endif
