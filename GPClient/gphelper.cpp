#include <QtCore/QXmlStreamReader>
#include <QtWidgets/QMessageBox>
#include <QtWidgets/QDesktopWidget>
#include <QtWidgets/QApplication>
#include <QtWidgets/QWidget>
#include <QtNetwork/QNetworkRequest>
#include <QtNetwork/QSslConfiguration>
#include <QtNetwork/QSslSocket>
#include <plog/Log.h>
#include <QWebEngineProfile>
#include <QWebEngineCookieStore>
#include <keychain.h>

#include "gphelper.h"

using namespace QKeychain;

QNetworkAccessManager* gpclient::helper::networkManager = new QNetworkAccessManager;

QNetworkReply* gpclient::helper::createRequest(QString url, QByteArray params)
{
    QNetworkRequest request(url);

    // Skip the ssl verifying
    QSslConfiguration conf = request.sslConfiguration();
    conf.setPeerVerifyMode(QSslSocket::VerifyNone);
    conf.setSslOption(QSsl::SslOptionDisableLegacyRenegotiation, false);
    request.setSslConfiguration(conf);

    request.setHeader(QNetworkRequest::ContentTypeHeader, "application/x-www-form-urlencoded");
    request.setHeader(QNetworkRequest::UserAgentHeader, UA);

    if (params == nullptr) {
        return networkManager->post(request, QByteArray(nullptr));
    }
    return networkManager->post(request, params);
}

GPGateway gpclient::helper::filterPreferredGateway(QList<GPGateway> gateways, const QString ruleName)
{
    LOGI << gateways.size() << " gateway(s) available, filter the gateways with rule: " << ruleName;

    GPGateway gateway = gateways.first();

    for (GPGateway g : gateways) {
        if (g.priorityOf(ruleName) > gateway.priorityOf(ruleName)) {
            LOGI << "Find a preferred gateway: " << g.name();
            gateway = g;
        }
    }

    return gateway;
}

QUrlQuery gpclient::helper::parseGatewayResponse(const QByteArray &xml)
{
    LOGI << "Start parsing the gateway response...";
    LOGI << "The gateway response is: " << xml;

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
    msgBox.setWindowTitle("Notice");
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

QStringList gpclient::helper::settings::get_all(const QString &key, const QVariant &defaultValue)
{
	QRegularExpression re(key);
	return 	_settings->allKeys().filter(re);
}

void gpclient::helper::settings::save(const QString &key, const QVariant &value)
{
    _settings->setValue(key, value);
}


void gpclient::helper::settings::clear()
{
    QStringList keys = _settings->allKeys();
    for (const auto &key : qAsConst(keys)) {
        if (!reservedKeys.contains(key)) {
            _settings->remove(key);
        }
    }

    QWebEngineProfile::defaultProfile()->cookieStore()->deleteAllCookies();
}


bool gpclient::helper::settings::secureSave(const QString &key, const QString &value) {
    WritePasswordJob job( QLatin1String("gpclient") );
    job.setAutoDelete( false );
    job.setKey( key );
    job.setTextData( value );
    QEventLoop loop;
    job.connect( &job, SIGNAL(finished(QKeychain::Job*)), &loop, SLOT(quit()) );
    job.start();
    loop.exec();
    if ( job.error() ) {
        return false;
    }

    return true;
}

bool gpclient::helper::settings::secureGet(const QString &key, QString &value) {
    ReadPasswordJob job( QLatin1String("gpclient") );
    job.setAutoDelete( false );
    job.setKey( key );
    QEventLoop loop;
    job.connect( &job, SIGNAL(finished(QKeychain::Job*)), &loop, SLOT(quit()) );
    job.start();
    loop.exec();

    const QString pw = job.textData();
    if ( job.error() ) {
        return false;
    }

    value = pw;
    return true;
}
