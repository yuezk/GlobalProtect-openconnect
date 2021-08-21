#include "gatewayauthenticator.h"
#include "gphelper.h"
#include "loginparams.h"
#include "preloginresponse.h"

#include <QNetworkReply>
#include <plog/Log.h>

using namespace gpclient::helper;

GatewayAuthenticator::GatewayAuthenticator(const QString& gateway, const GatewayAuthenticatorParams& params)
    : QObject()
    , gateway(gateway)
    , params(params)
    , preloginUrl("https://" + gateway + "/ssl-vpn/prelogin.esp?tmp=tmp&kerberos-support=yes&ipv6-support=yes&clientVer=4100")
    , loginUrl("https://" + gateway + "/ssl-vpn/login.esp")
{
    if (!params.clientos().isEmpty()) {
        preloginUrl = preloginUrl + "&clientos=" + params.clientos();
    }
}

GatewayAuthenticator::~GatewayAuthenticator()
{
    delete normalLoginWindow;
}

void GatewayAuthenticator::authenticate()
{
    PLOGI << "Start gateway authentication...";

    LoginParams loginParams;
    loginParams.setUser(params.username());
    loginParams.setPassword(params.password());
    loginParams.setUserAuthCookie(params.userAuthCookie());

    if (!params.clientos().isEmpty()) {
        loginParams.setClientos(params.clientos());
    }

    login(loginParams);
}

void GatewayAuthenticator::login(const LoginParams &params)
{
    PLOGI << "Trying to login the gateway at " << loginUrl << " with " << params.toUtf8();

    QNetworkReply *reply = createRequest(loginUrl, params.toUtf8());
    connect(reply, &QNetworkReply::finished, this, &GatewayAuthenticator::onLoginFinished);
}

void GatewayAuthenticator::onLoginFinished()
{
    QNetworkReply *reply = qobject_cast<QNetworkReply*>(sender());
    QByteArray response;

    if (reply->error() || (response = reply->readAll()).contains("Authentication failure")) {
        PLOGE << QString("Failed to login the gateway at %1, %2").arg(loginUrl).arg(reply->errorString());

        if (normalLoginWindow) {
            normalLoginWindow->setProcessing(false);
            openMessageBox("Gateway login failed.", "Please check your credentials and try again.");
        } else {
            doAuth();
        }
        return;
    }

    if (normalLoginWindow) {
        normalLoginWindow->close();
    }

    const QUrlQuery params = gpclient::helper::parseGatewayResponse(response);
    emit success(params.toString());
}

void GatewayAuthenticator::doAuth()
{
    PLOGI << "Perform the gateway prelogin at " << preloginUrl;

    QNetworkReply *reply = createRequest(preloginUrl);
    connect(reply, &QNetworkReply::finished, this, &GatewayAuthenticator::onPreloginFinished);
}

void GatewayAuthenticator::onPreloginFinished()
{
    QNetworkReply *reply = qobject_cast<QNetworkReply*>(sender());

    if (reply->error()) {
        PLOGE << QString("Failed to prelogin the gateway at %1, %2").arg(preloginUrl).arg(reply->errorString());

        emit fail("Error occurred on the gateway prelogin interface.");
        return;
    }

    PLOGI << "Gateway prelogin succeeded.";

    PreloginResponse response = PreloginResponse::parse(reply->readAll());

    if (response.hasSamlAuthFields()) {
        samlAuth(response.samlMethod(), response.samlRequest(), reply->url().toString());
    } else if (response.hasNormalAuthFields()) {
        normalAuth(response.labelUsername(), response.labelPassword(), response.authMessage());
    } else {
        PLOGE << QString("Unknown prelogin response for %1, got %2").arg(preloginUrl).arg(QString::fromUtf8(response.rawResponse()));
        emit fail("Unknown response for gateway prelogin interface.");
    }

    delete reply;
}

void GatewayAuthenticator::normalAuth(QString labelUsername, QString labelPassword, QString authMessage)
{
    PLOGI << QString("Trying to perform the normal login with %1 / %2 credentials").arg(labelUsername).arg(labelPassword);

    normalLoginWindow = new NormalLoginWindow;
    normalLoginWindow->setPortalAddress(gateway);
    normalLoginWindow->setAuthMessage(authMessage);
    normalLoginWindow->setUsernameLabel(labelUsername);
    normalLoginWindow->setPasswordLabel(labelPassword);

    // Do login
    connect(normalLoginWindow, &NormalLoginWindow::performLogin, this, &GatewayAuthenticator::onPerformNormalLogin);
    connect(normalLoginWindow, &NormalLoginWindow::rejected, this, &GatewayAuthenticator::onLoginWindowRejected);
    connect(normalLoginWindow, &NormalLoginWindow::finished, this, &GatewayAuthenticator::onLoginWindowFinished);

    normalLoginWindow->show();
}

void GatewayAuthenticator::onPerformNormalLogin(const QString &username, const QString &password)
{
    PLOGI << "Start to perform normal login...";

    normalLoginWindow->setProcessing(true);
    LoginParams params;
    params.setUser(username);
    params.setPassword(password);
    login(params);
}

void GatewayAuthenticator::onLoginWindowRejected()
{
    emit fail();
}

void GatewayAuthenticator::onLoginWindowFinished()
{
    delete normalLoginWindow;
    normalLoginWindow = nullptr;
}

void GatewayAuthenticator::samlAuth(QString samlMethod, QString samlRequest, QString preloginUrl)
{
    PLOGI << "Trying to perform SAML login with saml-method " << samlMethod;

    SAMLLoginWindow *loginWindow = new SAMLLoginWindow;

    connect(loginWindow, &SAMLLoginWindow::success, this, &GatewayAuthenticator::onSAMLLoginSuccess);
    connect(loginWindow, &SAMLLoginWindow::fail, this, &GatewayAuthenticator::onSAMLLoginFail);
    connect(loginWindow, &SAMLLoginWindow::rejected, this, &GatewayAuthenticator::onLoginWindowRejected);

    loginWindow->login(samlMethod, samlRequest, preloginUrl);
}

void GatewayAuthenticator::onSAMLLoginSuccess(const QMap<QString, QString> &samlResult)
{
    if (samlResult.contains("preloginCookie")) {
        PLOGI << "SAML login succeeded, got the prelogin-cookie " << samlResult.value("preloginCookie");
    } else {
        PLOGI << "SAML login succeeded, got the portal-userauthcookie " << samlResult.value("userAuthCookie");
    }

    LoginParams params;
    params.setUser(samlResult.value("username"));
    params.setPreloginCookie(samlResult.value("preloginCookie"));
    params.setUserAuthCookie(samlResult.value("userAuthCookie"));

    login(params);
}

void GatewayAuthenticator::onSAMLLoginFail(const QString msg)
{
    emit fail(msg);
}
