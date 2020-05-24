#include "samlloginwindow.h"

#include <QVBoxLayout>
#include <plog/Log.h>
#include <QWebEngineProfile>
#include <QWebEngineView>

SAMLLoginWindow::SAMLLoginWindow(QWidget *parent)
    : QDialog(parent)
    , webView(new EnhancedWebView(this))
{
    setWindowTitle("GlobalProtect SAML Login");
    setModal(true);
    resize(700, 550);

    QVBoxLayout *verticalLayout = new QVBoxLayout(this);
    webView->setUrl(QUrl("about:blank"));
    // webView->page()->profile()->setPersistentCookiesPolicy(QWebEngineProfile::NoPersistentCookies);
    verticalLayout->addWidget(webView);

    webView->initialize();
    connect(webView, &EnhancedWebView::responseReceived, this, &SAMLLoginWindow::onResponseReceived);
    connect(webView, &EnhancedWebView::loadFinished, this, &SAMLLoginWindow::onLoadFinished);
}

SAMLLoginWindow::~SAMLLoginWindow()
{
    delete webView;
}

void SAMLLoginWindow::closeEvent(QCloseEvent *event)
{
    event->accept();
    reject();
}

void SAMLLoginWindow::login(const QString samlMethod, const QString samlRequest, const QString preloingUrl)
{
    if (samlMethod == "POST") {
        webView->setHtml(samlRequest, preloingUrl);
    } else if (samlMethod == "REDIRECT") {
        webView->load(samlRequest);
    } else {
        PLOGE << "Unknown saml-auth-method expected POST or REDIRECT, got " << samlMethod;
        emit fail("Unknown saml-auth-method, got " + samlMethod);
    }
}

void SAMLLoginWindow::onResponseReceived(QJsonObject params)
{
    QString type = params.value("type").toString();
    // Skip non-document response
    if (type != "Document") {
        return;
    }

    QJsonObject response = params.value("response").toObject();
    QJsonObject headers = response.value("headers").toObject();

    const QString username = headers.value("saml-username").toString();
    const QString preloginCookie = headers.value("prelogin-cookie").toString();

    if (!username.isEmpty() && !preloginCookie.isEmpty()) {
        samlResult.insert("username", username);
        samlResult.insert("preloginCookie", preloginCookie);
    }
}

void SAMLLoginWindow::onLoadFinished()
{
     LOGI << "Load finished " << this->webView->page()->url().toString();

    // Check the SAML result
    if (!samlResult.value("username").isEmpty() && !samlResult.value("preloginCookie").isEmpty()) {
        emit success(samlResult);
        accept();
    } else {
        this->show();
    }
}
