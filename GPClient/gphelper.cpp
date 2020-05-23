#include "gphelper.h"
#include <QNetworkRequest>
#include <QXmlStreamReader>
#include <QMessageBox>
#include <QDesktopWidget>
#include <QApplication>
#include <QWidget>
#include <plog/Log.h>

QNetworkAccessManager* gpclient::helper::networkManager = new QNetworkAccessManager;

QNetworkReply* gpclient::helper::createRequest(QString url, QByteArray params)
{
    QNetworkRequest request(url);
    request.setHeader(QNetworkRequest::ContentTypeHeader, "application/x-www-form-urlencoded");
    request.setHeader(QNetworkRequest::UserAgentHeader, UA);

    if (params == nullptr) {
        return networkManager->post(request, QByteArray(nullptr));
    }
    return networkManager->post(request, params);
}

SAMLLoginWindow* gpclient::helper::samlLogin(QString samlMethod, QString samlRequest, QString preloginUrl)
{
    SAMLLoginWindow *loginWindow = new SAMLLoginWindow;

    if (samlMethod == "POST") {
        loginWindow->login(preloginUrl, samlRequest);
    } else if (samlMethod == "REDIRECT") {
        loginWindow->login(samlRequest);
    } else {
        PLOGE << "Unknown saml-auth-method expected POST or REDIRECT, got " << samlMethod;
        return nullptr;
    }
    return loginWindow;
}

GPGateway &gpclient::helper::filterPreferredGateway(QList<GPGateway> &gateways, QString ruleName)
{
    GPGateway& gateway = gateways.first();

    for (GPGateway& g : gateways) {
        if (g.priorityOf(ruleName) > gateway.priorityOf(ruleName)) {
            gateway = g;
        }
    }

    return gateway;
}

QUrlQuery gpclient::helper::parseGatewayResponse(const QByteArray &xml)
{
    QXmlStreamReader xmlReader{xml};
    QList<QString> args;

    while (!xmlReader.atEnd()) {
        xmlReader.readNextStartElement();
        if (xmlReader.name() == "argument") {
            args.append(QUrl::toPercentEncoding(xmlReader.readElementText()));
        }
    }

    QUrlQuery params{};
    params.addQueryItem("authcookie", args.at(1));
    params.addQueryItem("portal", args.at(3));
    params.addQueryItem("user", args.at(4));
    params.addQueryItem("domain", args.at(7));
    params.addQueryItem("preferred-ip", args.at(15));
    params.addQueryItem("computer", QUrl::toPercentEncoding(QSysInfo::machineHostName()));

    return params;
}

void gpclient::helper::openMessageBox(const QString &message, const QString& informativeText)
{
    QMessageBox msgBox;
    msgBox.setWindowTitle("GlobalProtect");
    msgBox.setText(message);
    msgBox.setFixedWidth(500);
    msgBox.setStyleSheet("QLabel{min-width: 250px}");
    msgBox.setInformativeText(informativeText);
    msgBox.exec();
}

void gpclient::helper::moveCenter(QWidget *widget)
{
    QDesktopWidget *desktop = QApplication::desktop();

    int screenWidth, width;
    int screenHeight, height;
    int x, y;
    QSize windowSize;

    screenWidth = desktop->width();
    screenHeight = desktop->height();

    windowSize = widget->size();
    width = windowSize.width();
    height = windowSize.height();

    x = (screenWidth - width) / 2;
    y = (screenHeight - height) / 2;
    y -= 50;
    widget->move(x, y);
}

QSettings *gpclient::helper::settings::_settings = new QSettings("com.yuezk.qt", "GPClient");

QVariant gpclient::helper::settings::get(const QString &key, const QVariant &defaultValue)
{
    return _settings->value(key, defaultValue);
}

void gpclient::helper::settings::save(const QString &key, const QVariant &value)
{
    _settings->setValue(key, value);
}
