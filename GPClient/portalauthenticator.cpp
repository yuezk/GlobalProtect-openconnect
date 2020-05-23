#include "portalauthenticator.h"
#include "gphelper.h"
#include "normalloginwindow.h"
#include "samlloginwindow.h"
#include "loginparams.h"
#include "preloginresponse.h"
#include "portalconfigresponse.h"
#include "gpgateway.h"

#include <plog/Log.h>
#include <QNetworkReply>

using namespace gpclient::helper;

PortalAuthenticator::PortalAuthenticator(const QString& portal) : QObject()
  , portal(portal)
  , preloginUrl("https://" + portal + "/global-protect/prelogin.esp")
  , configUrl("https://" + portal + "/global-protect/getconfig.esp")
{
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
        PLOGE << QString("Error occurred while accessing %1, %2").arg(preloginUrl).arg(reply->errorString());
        emit preloginFailed("Error occurred on the portal prelogin interface.");
        delete reply;
        return;
    }

    PLOGI << "Portal prelogin succeeded.";

    preloginResponse = PreloginResponse::parse(reply->readAll());
    if (preloginResponse.hasSamlAuthFields()) {
        // Do SAML authentication
        samlAuth();
    } else if (preloginResponse.hasNormalAuthFields()) {
        // Do normal username/password authentication
        tryAutoLogin();
    } else {
        PLOGE << QString("Unknown prelogin response for %1 got %2").arg(preloginUrl).arg(QString::fromUtf8(preloginResponse.rawResponse()));
        emitFail("Unknown response for portal prelogin interface.");
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

    normalLoginWindow->exec();
    delete normalLoginWindow;
    normalLoginWindow = nullptr;
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

void PortalAuthenticator::samlAuth()
{
    PLOGI << "Trying to perform SAML login with saml-method " << preloginResponse.samlMethod();

    SAMLLoginWindow *loginWindow = samlLogin(preloginResponse.samlMethod(), preloginResponse.samlRequest(), preloginUrl);

    if (!loginWindow) {
        openMessageBox("SAML Login failed for portal");
        return;
    }

    connect(loginWindow, &SAMLLoginWindow::success, this, &PortalAuthenticator::onSAMLLoginSuccess);
    connect(loginWindow, &SAMLLoginWindow::rejected, this, &PortalAuthenticator::onLoginWindowRejected);
}

void PortalAuthenticator::onSAMLLoginSuccess(const QMap<QString, QString> &samlResult)
{
    PLOGI << "SAML login succeeded, got the prelogin cookie " << samlResult.value("preloginCookie");

    fetchConfig(samlResult.value("username"), "", samlResult.value("preloginCookie"));
}

void PortalAuthenticator::fetchConfig(QString username, QString password, QString preloginCookie)
{
    LoginParams params;
    params.setServer(portal);
    params.setUser(username);
    params.setPassword(password);
    params.setPreloginCookie(preloginCookie);

    // Save the username and password for future use.
    this->username = username;
    this->password = password;

    PLOGI << "Fetching the portal config from " << configUrl << " for user: " << username;

    QNetworkReply *reply = createRequest(configUrl, params.toUtf8());

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
            emitFail("Failed to fetch the portal config.");
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
        // Save the credentials for reuse
        settings::save("username", username);
        settings::save("password", password);
        normalLoginWindow->close();
    }

    emit success(response, filterPreferredGateway(response.allGateways(), preloginResponse.region()));
}

void PortalAuthenticator::emitFail(const QString& msg)
{
    emit fail(msg);
}
