#ifndef PORTALAUTHENTICATOR_H
#define PORTALAUTHENTICATOR_H

#include "portalconfigresponse.h"
#include "normalloginwindow.h"
#include "samlloginwindow.h"
#include "preloginresponse.h"

#include <QObject>

class PortalAuthenticator : public QObject
{
    Q_OBJECT
public:
    explicit PortalAuthenticator(const QString& portal);
    ~PortalAuthenticator();

    void authenticate();

signals:
    void success(const PortalConfigResponse, const GPGateway, QList<GPGateway> allGateways);
    void fail(const QString& msg);
    void preloginFailed(const QString& msg);

private slots:
    void onPreloginFinished();
    void onPerformNormalLogin(const QString &username, const QString &password);
    void onLoginWindowRejected();
    void onLoginWindowFinished();
    void onSAMLLoginSuccess(const QMap<QString, QString> samlResult);
    void onSAMLLoginFail(const QString msg);
    void onFetchConfigFinished();

private:
    QString portal;
    QString preloginUrl;
    QString configUrl;
    QString username;
    QString password;

    PreloginResponse preloginResponse;

    bool isAutoLogin { false };

    NormalLoginWindow *normalLoginWindow{ nullptr };

    void tryAutoLogin();
    void normalAuth();
    void samlAuth();
    void fetchConfig(QString username, QString password, QString preloginCookie = "");
    void emitFail(const QString& msg = "");
};

#endif // PORTALAUTHENTICATOR_H
