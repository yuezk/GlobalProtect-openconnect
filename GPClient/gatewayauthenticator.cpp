#include <QtNetwork/QNetworkReply>
#include <QtCore/QRegularExpression>
#include <QtCore/QRegularExpressionMatch>
#include <plog/Log.h>

#include "gatewayauthenticator.h"
#include "gphelper.h"
#include "loginparams.h"
#include "preloginresponse.h"
#include "challengedialog.h"

using namespace gpclient::helper;

GatewayAuthenticator::GatewayAuthenticator(const QString& gateway, GatewayAuthenticatorParams params)
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

void GatewayAuthenticator::authenticate()
{
    LOGI << "Start gateway authentication...";

    LoginParams loginParams { params.clientos() };
    loginParams.setUser(params.username());
    loginParams.setPassword(params.password());
    loginParams.setUserAuthCookie(params.userAuthCookie());
    loginParams.setInputStr(params.inputStr());

    login(loginParams);
}

void GatewayAuthenticator::login(const LoginParams &loginParams)
{
    LOGI << QString("Trying to login the gateway at %1, with %2").arg(loginUrl).arg(QString(loginParams.toUtf8()));

    auto *reply = createRequest(loginUrl, loginParams.toUtf8());
    connect(reply, &QNetworkReply::finished, this, &GatewayAuthenticator::onLoginFinished);
}

void GatewayAuthenticator::onLoginFinished()
{
    QNetworkReply *reply = qobject_cast<QNetworkReply*>(sender());
    QByteArray response = reply->readAll();

    if (reply->error() || response.contains("Authentication failure")) {
        LOGE << QString("Failed to login the gateway at %1, %2").arg(loginUrl, reply->errorString());

        if (standardLoginWindow) {
            standardLoginWindow->setProcessing(false);
            openMessageBox("Gateway login failed.", "Please check your credentials and try again.");
        } else {
            doAuth();
        }
        return;
    }

    // 2FA
    if (response.contains("Challenge")) {
        LOGI << "The server need input the challenge...";
        showChallenge(response);
        return;
    }

    if (standardLoginWindow) {
        standardLoginWindow->close();
    }

    const auto params = gpclient::helper::parseGatewayResponse(response);
    emit success(params.toString());
}

void GatewayAuthenticator::doAuth()
{
    LOGI << "Perform the gateway prelogin at " << preloginUrl;

    auto *reply = createRequest(preloginUrl);
    connect(reply, &QNetworkReply::finished, this, &GatewayAuthenticator::onPreloginFinished);
}

void GatewayAuthenticator::onPreloginFinished()
{
    auto *reply = qobject_cast<QNetworkReply*>(sender());

    if (reply->error()) {
        LOGE << QString("Failed to prelogin the gateway at %1, %2").arg(preloginUrl, reply->errorString());

        emit fail("Error occurred on the gateway prelogin interface.");
        return;
    }

    LOGI << "Gateway prelogin succeeded.";

    auto response = PreloginResponse::parse(reply->readAll());

    if (response.hasSamlAuthFields()) {
        samlAuth(response.samlMethod(), response.samlRequest(), reply->url().toString());
    } else if (response.hasNormalAuthFields()) {
        normalAuth(response.labelUsername(), response.labelPassword(), response.authMessage());
    } else {
        LOGE << QString("Unknown prelogin response for %1, got %2").arg(preloginUrl, QString::fromUtf8(response.rawResponse()));
        emit fail("Unknown response for gateway prelogin interface.");
    }

    delete reply;
}

void GatewayAuthenticator::normalAuth(QString labelUsername, QString labelPassword, QString authMessage)
{
    LOGI << QString("Trying to perform the normal login with %1 / %2 credentials").arg(labelUsername, labelPassword);

    standardLoginWindow = new StandardLoginWindow {gateway, labelUsername, labelPassword, authMessage};

    // Do login
    connect(standardLoginWindow, &StandardLoginWindow::performLogin, this, &GatewayAuthenticator::onPerformStandardLogin);
    connect(standardLoginWindow, &StandardLoginWindow::rejected, this, &GatewayAuthenticator::onLoginWindowRejected);
    connect(standardLoginWindow, &StandardLoginWindow::finished, this, &GatewayAuthenticator::onLoginWindowFinished);

    standardLoginWindow->show();
}

void GatewayAuthenticator::onPerformStandardLogin(const QString &username, const QString &password)
{
    LOGI << "Start to perform normal login...";

    standardLoginWindow->setProcessing(true);
    params.setUsername(username);
    params.setPassword(password);

    authenticate();
}

void GatewayAuthenticator::onLoginWindowRejected()
{
    emit fail();
}

void GatewayAuthenticator::onLoginWindowFinished()
{
    delete standardLoginWindow;
    standardLoginWindow = nullptr;
}

void GatewayAuthenticator::samlAuth(QString samlMethod, QString samlRequest, QString preloginUrl)
{
    LOGI << "Trying to perform SAML login with saml-method " << samlMethod;

    auto *loginWindow = new SAMLLoginWindow(gateway);

    connect(loginWindow, &SAMLLoginWindow::success, [this, loginWindow](const QMap<QString, QString> &samlResult) {
        this->onSAMLLoginSuccess(samlResult);
        loginWindow->deleteLater();
    });
    connect(loginWindow, &SAMLLoginWindow::fail, [this, loginWindow](const QString &code, const QString &error) {
        this->onSAMLLoginFail(code, error);
        loginWindow->deleteLater();
    });
    connect(loginWindow, &SAMLLoginWindow::rejected, [this, loginWindow]() {
        this->onLoginWindowRejected();
        loginWindow->deleteLater();
    });

    loginWindow->login(samlMethod, samlRequest, preloginUrl);
}

void GatewayAuthenticator::onSAMLLoginSuccess(const QMap<QString, QString> &samlResult)
{
    if (samlResult.contains("preloginCookie")) {
        LOGI << "SAML login succeeded, got the prelogin-cookie " << samlResult.value("preloginCookie");
    } else {
        LOGI << "SAML login succeeded, got the portal-userauthcookie " << samlResult.value("userAuthCookie");
    }

    LoginParams loginParams { params.clientos() };
    loginParams.setUser(samlResult.value("username"));
    loginParams.setPreloginCookie(samlResult.value("preloginCookie"));
    loginParams.setUserAuthCookie(samlResult.value("userAuthCookie"));

    login(loginParams);
}

void GatewayAuthenticator::onSAMLLoginFail(const QString &code, const QString &msg)
{
    emit fail(msg);
}

void GatewayAuthenticator::showChallenge(const QString &responseText)
{
    QRegularExpression re("\"(.*?)\";");
    QRegularExpressionMatchIterator i = re.globalMatch(responseText);

    i.next(); // Skip the status value
    QString message = i.next().captured(1);
    QString inputStr = i.next().captured(1);
    // update the inputSrc field
    params.setInputStr(inputStr);

    challengeDialog = new ChallengeDialog;
    challengeDialog->setMessage(message);

    connect(challengeDialog, &ChallengeDialog::accepted, this, [this] {
        params.setPassword(challengeDialog->getChallenge());
        LOGI << "Challenge submitted, try to re-authenticate...";
        authenticate();
    });

    connect(challengeDialog, &ChallengeDialog::rejected, this, [this] {
        if (standardLoginWindow) {
            standardLoginWindow->close();
        }
        emit fail();
    });

    connect(challengeDialog, &ChallengeDialog::finished, this, [this] {
        delete challengeDialog;
        challengeDialog = nullptr;
    });

    challengeDialog->show();
}
