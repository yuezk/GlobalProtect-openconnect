#include <QtWidgets/QVBoxLayout>
#include <QtWebEngineWidgets/QWebEngineProfile>
#include <QtWebEngineWidgets/QWebEngineView>
#include <QWebEngineCookieStore>
#include <plog/Log.h>

#include "samlloginwindow.h"

SAMLLoginWindow::SAMLLoginWindow(QWidget *parent)
    : QDialog(parent)
    , webView(new EnhancedWebView(this))
{
    setWindowTitle("GlobalProtect Login");
    setModal(true);
    resize(700, 550);

    QVBoxLayout *verticalLayout = new QVBoxLayout(this);
    webView->setUrl(QUrl("about:blank"));
    webView->setAttribute(Qt::WA_DeleteOnClose);
    verticalLayout->addWidget(webView);

    webView->initialize();
    connect(webView, &EnhancedWebView::responseReceived, this, &SAMLLoginWindow::onResponseReceived);
    connect(webView, &EnhancedWebView::loadFinished, this, &SAMLLoginWindow::onLoadFinished);

    // Show the login window automatically when exceeds the MAX_WAIT_TIME
    QTimer::singleShot(MAX_WAIT_TIME, this, [this]() {
        if (failed) {
            return;
        }
        LOGI << "MAX_WAIT_TIME exceeded, display the login window.";
        this->show();
    });
}

void SAMLLoginWindow::closeEvent(QCloseEvent *event)
{
    event->accept();
    reject();
}

void SAMLLoginWindow::login(const QString samlMethod, const QString samlRequest, const QString preloginUrl)
{
    webView->page()->profile()->cookieStore()->deleteSessionCookies();

    if (samlMethod == "POST") {
        webView->setHtml(samlRequest, preloginUrl);
    } else if (samlMethod == "REDIRECT") {
        LOGI << "Redirect to " << samlRequest;
        webView->load(samlRequest);
    } else {
        LOGE << "Unknown saml-auth-method expected POST or REDIRECT, got " << samlMethod;
        failed = true;
        emit fail("ERR001", "Unknown saml-auth-method, got " + samlMethod);
    }
}

void SAMLLoginWindow::onResponseReceived(QJsonObject params)
{
    const auto type = params.value("type").toString();
    // Skip non-document response
    if (type != "Document") {
        return;
    }

    auto response = params.value("response").toObject();
    auto headers = response.value("headers").toObject();

    LOGI << "Trying to receive authentication cookie from " << response.value("url").toString();

    const auto username = headers.value("saml-username").toString();
    const auto preloginCookie = headers.value("prelogin-cookie").toString();
    const auto userAuthCookie = headers.value("portal-userauthcookie").toString();

    this->checkSamlResult(username, preloginCookie, userAuthCookie);
}

void SAMLLoginWindow::checkSamlResult(QString username, QString preloginCookie, QString userAuthCookie)
{
    LOGI << "Checking the authentication result...";

    if (!username.isEmpty()) {
        samlResult.insert("username", username);
    }

    if (!preloginCookie.isEmpty()) {
        samlResult.insert("preloginCookie", preloginCookie);
    }

    if (!userAuthCookie.isEmpty()) {
        samlResult.insert("userAuthCookie", userAuthCookie);
    }

    // Check the SAML result
    if (samlResult.contains("username")
            && (samlResult.contains("preloginCookie") || samlResult.contains("userAuthCookie"))) {
        LOGI << "Got the SAML authentication information successfully. "
             << "username: " << samlResult.value("username")
             << ", preloginCookie: " << samlResult.value("preloginCookie")
             << ", userAuthCookie: " << samlResult.value("userAuthCookie");

        emit success(samlResult);
        accept();
    }
}

void SAMLLoginWindow::onLoadFinished()
{
     LOGI << "Load finished " << webView->page()->url().toString();
     webView->page()->toHtml([this] (const QString &html) { this->handleHtml(html); });
}

void SAMLLoginWindow::handleHtml(const QString &html)
{
    // try to check the html body and extract from there
    const auto samlAuthStatus = parseTag("saml-auth-status", html);

    if (samlAuthStatus == "1") {
        const auto preloginCookie = parseTag("prelogin-cookie", html);
        const auto username = parseTag("saml-username", html);
        const auto userAuthCookie = parseTag("portal-userauthcookie", html);

        checkSamlResult(username, preloginCookie, userAuthCookie);
    } else if (samlAuthStatus == "-1") {
        LOGI << "SAML authentication failed...";
        failed = true;
        emit fail("ERR002", "Authentication failed, please try again.");
    } else {
        show();
    }
}

QString SAMLLoginWindow::parseTag(const QString &tag, const QString &html) {
    const QRegularExpression expression(QString("<%1>(.*)</%1>").arg(tag));
    return expression.match(html).captured(1);
}
