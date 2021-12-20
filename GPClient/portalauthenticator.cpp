#include <QtNetwork/QNetworkReply>
#include <plog/Log.h>

#include "portalauthenticator.h"
#include "gphelper.h"
#include "normalloginwindow.h"
#include "samlloginwindow.h"
#include "loginparams.h"
#include "preloginresponse.h"
#include "portalconfigresponse.h"
#include "gpgateway.h"

using namespace gpclient::helper;

PortalAuthenticator::PortalAuthenticator(const QString& portal, const QString& clientos) : QObject()
  , portal(portal)
  , clientos(clientos)
  , preloginUrl("https://" + portal + "/global-protect/prelogin.esp?tmp=tmp&kerberos-support=yes&ipv6-support=yes&clientVer=4100")
  , configUrl("https://" + portal + "/global-protect/getconfig.esp")
{
    if (!clientos.isEmpty()) {
        preloginUrl = preloginUrl + "&clientos=" + clientos;
    }
}

PortalAuthenticator::~PortalAuthenticator()
{
    delete normalLoginWindow;
}

void PortalAuthenticator::authenticate()
{
    PLOGI << "Preform portal prelogin at " << preloginUrl;

    QNetworkReply *reply = createRequest(preloginUrl);
    connect(reply, &QNetworkReply::finished, this, &PortalAuthenticator::onPreloginFinished);
}

void PortalAuthenticator::onPreloginFinished()
{
    QNetworkReply *reply = qobject_cast<QNetworkReply*>(sender());

    if (reply->error()) {
        PLOGE << QString("Error occurred while accessing %1, %2").arg(preloginUrl, reply->errorString());
        emit preloginFailed("Error occurred on the portal prelogin interface.");
        delete reply;
        return;
    }

    PLOGI << "Portal prelogin succeeded.";

    preloginResponse = PreloginResponse::parse(reply->readAll());

    PLOGI << "Finished parsing the prelogin response. The region field is: " << preloginResponse.region();

    if (preloginResponse.hasSamlAuthFields()) {
        // Do SAML authentication
        samlAuth();
    } else if (preloginResponse.hasNormalAuthFields()) {
        // Do normal username/password authentication
        tryAutoLogin();
    } else {
        PLOGE << QString("Unknown prelogin response for %1 got %2").arg(preloginUrl).arg(QString::fromUtf8(preloginResponse.rawResponse()));
        emit preloginFailed("Unknown response for portal prelogin interface.");
    }

    delete reply;
}

void PortalAuthenticator::tryAutoLogin()
{
    const QString username = settings::get("username").toString();
    const QString password = settings::get("password").toString();

    if (!username.isEmpty() && !password.isEmpty()) {
        PLOGI << "Trying auto login using the saved credentials";
        isAutoLogin = true;
        fetchConfig(settings::get("username").toString(), settings::get("password").toString());
    } else {
        normalAuth();
    }
}

void PortalAuthenticator::normalAuth()
{
    PLOGI << "Trying to launch the normal login window...";

    normalLoginWindow = new NormalLoginWindow;
    normalLoginWindow->setPortalAddress(portal);
    normalLoginWindow->setAuthMessage(preloginResponse.authMessage());
    normalLoginWindow->setUsernameLabel(preloginResponse.labelUsername());
    normalLoginWindow->setPasswordLabel(preloginResponse.labelPassword());

    // Do login
    connect(normalLoginWindow, &NormalLoginWindow::performLogin, this, &PortalAuthenticator::onPerformNormalLogin);
    connect(normalLoginWindow, &NormalLoginWindow::rejected, this, &PortalAuthenticator::onLoginWindowRejected);
    connect(normalLoginWindow, &NormalLoginWindow::finished, this, &PortalAuthenticator::onLoginWindowFinished);

    normalLoginWindow->show();
}

void PortalAuthenticator::onPerformNormalLogin(const QString &username, const QString &password)
{
    normalLoginWindow->setProcessing(true);
    fetchConfig(username, password);
}

void PortalAuthenticator::onLoginWindowRejected()
{
    emitFail();
}

void PortalAuthenticator::onLoginWindowFinished()
{
    delete normalLoginWindow;
    normalLoginWindow = nullptr;
}

void PortalAuthenticator::samlAuth()
{
    PLOGI << "Trying to perform SAML login with saml-method " << preloginResponse.samlMethod();

    SAMLLoginWindow *loginWindow = new SAMLLoginWindow;

    connect(loginWindow, &SAMLLoginWindow::success, this, &PortalAuthenticator::onSAMLLoginSuccess);
    connect(loginWindow, &SAMLLoginWindow::fail, this, &PortalAuthenticator::onSAMLLoginFail);
    connect(loginWindow, &SAMLLoginWindow::rejected, this, &PortalAuthenticator::onLoginWindowRejected);

    loginWindow->login(preloginResponse.samlMethod(), preloginResponse.samlRequest(), preloginUrl);
}

void PortalAuthenticator::onSAMLLoginSuccess(const QMap<QString, QString> samlResult)
{
    if (samlResult.contains("preloginCookie")) {
        PLOGI << "SAML login succeeded, got the prelogin-cookie " << samlResult.value("preloginCookie");
    } else {
        PLOGI << "SAML login succeeded, got the portal-userauthcookie " << samlResult.value("userAuthCookie");
    }

    fetchConfig(samlResult.value("username"), "", samlResult.value("preloginCookie"), samlResult.value("userAuthCookie"));
}

void PortalAuthenticator::onSAMLLoginFail(const QString msg)
{
    emitFail(msg);
}

void PortalAuthenticator::fetchConfig(QString username, QString password, QString preloginCookie, QString userAuthCookie)
{
    LoginParams loginParams { clientos };
    loginParams.setServer(portal);
    loginParams.setUser(username);
    loginParams.setPassword(password);
    loginParams.setPreloginCookie(preloginCookie);
    loginParams.setUserAuthCookie(userAuthCookie);

    // Save the username and password for future use.
    this->username = username;
    this->password = password;

    PLOGI << "Fetching the portal config from " << configUrl << " for user: " << username;

    QNetworkReply *reply = createRequest(configUrl, loginParams.toUtf8());
    connect(reply, &QNetworkReply::finished, this, &PortalAuthenticator::onFetchConfigFinished);
}

void PortalAuthenticator::onFetchConfigFinished()
{
    QNetworkReply *reply = qobject_cast<QNetworkReply*>(sender());

    if (reply->error()) {
        PLOGE << QString("Failed to fetch the portal config from %1, %2").arg(configUrl).arg(reply->errorString());

        // Login failed, enable the fields of the normal login window
        if (normalLoginWindow) {
            normalLoginWindow->setProcessing(false);
            openMessageBox("Portal login failed.", "Please check your credentials and try again.");
        } else if (isAutoLogin) {
            isAutoLogin = false;
            normalAuth();
        } else {
            emit portalConfigFailed("Failed to fetch the portal config.");
        }
        return;
    }

    PLOGI << "Fetch the portal config succeeded.";
    PortalConfigResponse response = PortalConfigResponse::parse(reply->readAll());

    // Add the username & password to the response object
    response.setUsername(username);
    response.setPassword(password);

    // Close the login window
    if (normalLoginWindow) {
        PLOGI << "Closing the NormalLoginWindow...";

        normalLoginWindow->close();
    }

    emit success(response, preloginResponse.region());
}

void PortalAuthenticator::emitFail(const QString& msg)
{
    emit fail(msg);
}
