#include "samlloginwindow.h"

#include <QVBoxLayout>

SAMLLoginWindow::SAMLLoginWindow(QWidget *parent)
    : QDialog(parent)
{
    setWindowTitle("SAML Login");
    resize(610, 406);
    QVBoxLayout *verticalLayout = new QVBoxLayout(this);
    webView = new EnhancedWebView(this);
    webView->setUrl(QUrl("about:blank"));
    verticalLayout->addWidget(webView);

    webView->initialize();
    QObject::connect(webView, &EnhancedWebView::responseReceived, this, &SAMLLoginWindow::onResponseReceived);
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

void SAMLLoginWindow::login(QString url)
{
    webView->load(QUrl(url));
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

    foreach (const QString& key, headers.keys()) {
        if (key.startsWith("saml-") || key == "prelogin-cookie" || key == "portal-userauthcookie") {
            samlResult.insert(key, headers.value(key));
        }
    }

    // Check the SAML result
    if (samlResult.contains("saml-username")
            && (samlResult.contains("prelogin-cookie") || samlResult.contains("portal-userauthcookie"))) {
        samlResult.insert("server", QUrl(response.value("url").toString()).authority());
        emit success(samlResult);
        accept();
    }
}
