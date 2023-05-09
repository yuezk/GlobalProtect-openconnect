#include <QtNetwork/QNetworkReply>
#include <plog/Log.h>

#include "portalauthenticator.h"
#include "gphelper.h"
#include "standardloginwindow.h"
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
    delete standardLoginWindow;
}

void PortalAuthenticator::authenticate()
{
    attempts++;

    LOGI << QString("(%1/%2) attempts").arg(attempts).arg(MAX_ATTEMPTS) << ", perform portal prelogin at " << preloginUrl;

    QNetworkReply *reply = createRequest(preloginUrl);
    connect(reply, &QNetworkReply::finished, this, &PortalAuthenticator::onPreloginFinished);
}

void PortalAuthenticator::onPreloginFinished()
{
    auto *reply = qobject_cast<QNetworkReply*>(sender());

    if (reply->error()) {
        LOGE << QString("Error occurred while accessing %1, %2").arg(preloginUrl, reply->errorString());
        emit preloginFailed("Error occurred on the portal prelogin interface.");
        delete reply;
        return;
    }

    LOGI << "Portal prelogin succeeded.";

    preloginResponse = PreloginResponse::parse(reply->readAll());

    LOGI << "Finished parsing the prelogin response. The region field is: " << preloginResponse.region();

    if (preloginResponse.hasSamlAuthFields()) {
        // Do SAML authentication
        samlAuth();
    } else if (preloginResponse.hasNormalAuthFields()) {
        // Do normal username/password authentication
        tryAutoLogin();
    } else {
        LOGE << QString("Unknown prelogin response for %1 got %2").arg(preloginUrl).arg(QString::fromUtf8(preloginResponse.rawResponse()));
        emit preloginFailed("Unknown response for portal prelogin interface.");
    }

    delete reply;
}

void PortalAuthenticator::tryAutoLogin()
{
    const QString username = settings::get("username").toString();
    const QString password = settings::get("password").toString();

    if (!username.isEmpty() && !password.isEmpty()) {
        LOGI << "Trying auto login using the saved credentials";
        isAutoLogin = true;
        fetchConfig(settings::get("username").toString(), settings::get("password").toString());
    } else {
        normalAuth();
    }
}

void PortalAuthenticator::normalAuth()
{
    LOGI << "Trying to launch the normal login window...";

    standardLoginWindow = new StandardLoginWindow {portal, preloginResponse.labelUsername(), preloginResponse.labelPassword(), preloginResponse.authMessage() };

    // Do login
    connect(standardLoginWindow, &StandardLoginWindow::performLogin, this, &PortalAuthenticator::onPerformNormalLogin);
    connect(standardLoginWindow, &StandardLoginWindow::rejected, this, &PortalAuthenticator::onLoginWindowRejected);
    connect(standardLoginWindow, &StandardLoginWindow::finished, this, &PortalAuthenticator::onLoginWindowFinished);

    standardLoginWindow->show();
}

void PortalAuthenticator::onPerformNormalLogin(const QString &username, const QString &password)
{
    standardLoginWindow->setProcessing(true);
    fetchConfig(username, password);
}

void PortalAuthenticator::onLoginWindowRejected()
{
    emitFail();
}

void PortalAuthenticator::onLoginWindowFinished()
{
    delete standardLoginWindow;
    standardLoginWindow = nullptr;
}

void PortalAuthenticator::samlAuth()
{
    LOGI << "Trying to perform SAML login with saml-method " << preloginResponse.samlMethod();

    auto *loginWindow = new SAMLLoginWindow;

    connect(loginWindow, &SAMLLoginWindow::success, [this, loginWindow](const QMap<QString, QString> samlResult) {
        this->onSAMLLoginSuccess(samlResult);
        loginWindow->deleteLater();
    });
    connect(loginWindow, &SAMLLoginWindow::fail, [this, loginWindow](const QString &code, const QString msg) {
        this->onSAMLLoginFail(code, msg);
        loginWindow->deleteLater();
    });
    connect(loginWindow, &SAMLLoginWindow::rejected, [this, loginWindow]() {
        this->onLoginWindowRejected();
        loginWindow->deleteLater();
    });

    loginWindow->login(preloginResponse.samlMethod(), preloginResponse.samlRequest(), preloginUrl);
}

void PortalAuthenticator::onSAMLLoginSuccess(const QMap<QString, QString> samlResult)
{
    if (samlResult.contains("preloginCookie")) {
        LOGI << "SAML login succeeded, got the prelogin-cookie";
    } else {
        LOGI << "SAML login succeeded, got the portal-userauthcookie";
    }

    fetchConfig(samlResult.value("username"), "", samlResult.value("preloginCookie"), samlResult.value("userAuthCookie"));
}

void PortalAuthenticator::onSAMLLoginFail(const QString &code, const QString &msg)
{
    if (code == "ERR002" && attempts < MAX_ATTEMPTS) {
        LOGI << "Failed to authenticate, trying to re-authenticate...";
        authenticate();
    } else {
        emitFail(msg);
    }
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

    LOGI << "Fetching the portal config from " << configUrl;

    auto *reply = createRequest(configUrl, loginParams.toUtf8());
    connect(reply, &QNetworkReply::finished, this, &PortalAuthenticator::onFetchConfigFinished);
}

void PortalAuthenticator::onFetchConfigFinished()
{
    QNetworkReply *reply = qobject_cast<QNetworkReply*>(sender());

    if (reply->error()) {
        LOGE << QString("Failed to fetch the portal config from %1, %2").arg(configUrl).arg(reply->errorString());

        // Login failed, enable the fields of the normal login window
        if (standardLoginWindow) {
            standardLoginWindow->setProcessing(false);
            openMessageBox("Portal login failed.", "Please check your credentials and try again.");
        } else if (isAutoLogin) {
            isAutoLogin = false;
            normalAuth();
        } else {
            emit portalConfigFailed("Failed to fetch the portal config.");
        }
        return;
    }

    LOGI << "Fetch the portal config succeeded.";
    PortalConfigResponse response = PortalConfigResponse::parse(reply->readAll());

    // Add the username & password to the response object
    response.setUsername(username);
    response.setPassword(password);

    // Close the login window
    if (standardLoginWindow) {
        LOGI << "Closing the StandardLoginWindow...";

        standardLoginWindow->close();
    }

    emit success(response, preloginResponse.region());
}

void PortalAuthenticator::emitFail(const QString& msg)
{
    emit fail(msg);
}
