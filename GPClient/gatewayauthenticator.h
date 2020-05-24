#ifndef GATEWAYAUTHENTICATOR_H
#define GATEWAYAUTHENTICATOR_H

#include "portalconfigresponse.h"
#include "normalloginwindow.h"
#include "loginparams.h"
#include <QObject>

class GatewayAuthenticator : public QObject
{
    Q_OBJECT
public:
    explicit GatewayAuthenticator(const QString& gateway, const PortalConfigResponse& portalConfig);
    ~GatewayAuthenticator();

    void authenticate();

signals:
    void success(const QString& authCookie);
    void fail(const QString& msg = "");

private slots:
    void onLoginFinished();
    void onPreloginFinished();
    void onPerformNormalLogin(const QString &username, const QString &password);
    void onLoginWindowRejected();
    void onLoginWindowFinished();
    void onSAMLLoginSuccess(const QMap<QString, QString> &samlResult);
    void onSAMLLoginFail(const QString msg);

private:
    QString gateway;
    QString preloginUrl;
    QString loginUrl;

    const PortalConfigResponse& portalConfig;

    NormalLoginWindow *normalLoginWindow{ nullptr };

    void login(const LoginParams& params);
    void doAuth();
    void normalAuth(QString labelUsername, QString labelPassword, QString authMessage);
    void samlAuth(QString samlMethod, QString samlRequest, QString preloginUrl = "");
};

#endif // GATEWAYAUTHENTICATOR_H
