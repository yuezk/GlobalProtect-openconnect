#include "gatewayauthenticator.h"
#include "gphelper.h"
#include "loginparams.h"
#include "preloginresponse.h"

#include <QNetworkReply>
#include <plog/Log.h>

using namespace gpclient::helper;

GatewayAuthenticator::GatewayAuthenticator(const QString& gateway, const PortalConfigResponse& portalConfig)
    : QObject()
    , preloginUrl("https://" + gateway + "/ssl-vpn/prelogin.esp")
    , loginUrl("https://" + gateway + "/ssl-vpn/login.esp")
    , portalConfig(portalConfig)
{
}

GatewayAuthenticator::~GatewayAuthenticator()
{
    delete normalLoginWindow;
}

void GatewayAuthenticator::authenticate()
{
    LoginParams params;
    params.setUser(portalConfig.username());
    params.setPassword(portalConfig.password());
    params.setUserAuthCookie(portalConfig.userAuthCookie());

    login(params);
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

    if (reply->error()) {
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

    const QUrlQuery params = gpclient::helper::parseGatewayResponse(reply->readAll());
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

    normalLoginWindow->exec();
    delete normalLoginWindow;
    normalLoginWindow = nullptr;
}

void GatewayAuthenticator::onPerformNormalLogin(const QString &username, const QString &password)
{
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

void GatewayAuthenticator::samlAuth(QString samlMethod, QString samlRequest, QString preloginUrl)
{
    PLOGI << "Trying to perform SAML login with saml-method " << samlMethod;

    SAMLLoginWindow *loginWindow = samlLogin(samlMethod, samlRequest, preloginUrl);

    if (!loginWindow) {
        openMessageBox("SAML Login failed for gateway");
        return;
    }

    connect(loginWindow, &SAMLLoginWindow::success, this, &GatewayAuthenticator::onSAMLLoginFinished);
    connect(loginWindow, &SAMLLoginWindow::rejected, this, &GatewayAuthenticator::onLoginWindowRejected);
//    loginWindow->exec();
//    delete loginWindow;
}

void GatewayAuthenticator::onSAMLLoginFinished(const QMap<QString, QString> &samlResult)
{
    PLOGI << "SAML login succeeded, got the prelogin cookie " << samlResult.value("preloginCookie");

    LoginParams params;
    params.setUser(samlResult.value("username"));
    params.setPreloginCookie(samlResult.value("preloginCookie"));

    login(params);
}
