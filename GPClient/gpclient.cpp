#include "gpclient.h"
#include "ui_gpclient.h"
#include "samlloginwindow.h"

GPClient::GPClient(QWidget *parent)
    : QMainWindow(parent)
    , ui(new Ui::GPClient)
{
    ui->setupUi(this);

    QObject::connect(this, &GPClient::connectFailed, [this]() {
        ui->connectButton->setDisabled(false);
        ui->connectButton->setText("Connect");
    });

    // QNetworkAccessManager setup
    networkManager = new QNetworkAccessManager(this);

    // Login window setup
    loginWindow = new SAMLLoginWindow(this);
    QObject::connect(loginWindow, &SAMLLoginWindow::success, this, &GPClient::onLoginSuccess);
    QObject::connect(loginWindow, &SAMLLoginWindow::rejected, this, &GPClient::connectFailed);

    // DBus service setup
    vpn = new com::yuezk::qt::GPService("com.yuezk.qt.GPService", "/", QDBusConnection::systemBus(), this);
    QObject::connect(vpn, &com::yuezk::qt::GPService::connected, this, &GPClient::onVPNConnected);
    QObject::connect(vpn, &com::yuezk::qt::GPService::disconnected, this, &GPClient::onVPNDisconnected);
    QObject::connect(vpn, &com::yuezk::qt::GPService::logAvailable, this, &GPClient::onVPNLogAvailable);
}

GPClient::~GPClient()
{
    delete ui;
    delete networkManager;
    delete reply;
    delete loginWindow;
    delete vpn;
}

void GPClient::on_connectButton_clicked()
{
    if (ui->connectButton->text() == "Connect") {
        QString portal = ui->portalInput->text();

        ui->connectButton->setDisabled(true);
        ui->connectButton->setText("Connecting...");
        samlLogin(portal);
    } else {
        ui->connectButton->setDisabled(true);
        ui->connectButton->setText("Disconnecting...");

        vpn->disconnect();
    }
}

void GPClient::preloginResultFinished()
{
    if (reply->error()) {
        qDebug() << "request error";
        emit connectFailed();
        return;
    }

    QByteArray bytes = reply->readAll();
    qDebug("response is: %s", bytes.toStdString().c_str());

    const QString tagMethod = "saml-auth-method";
    const QString tagRequest = "saml-request";
    QString samlMethod;
    QString samlRequest;

    QXmlStreamReader xml(bytes);
    while (!xml.atEnd()) {
        xml.readNext();
        if (xml.tokenType() == xml.StartElement) {
            if (xml.name() == tagMethod) {
                samlMethod = xml.readElementText();
            } else if (xml.name() == tagRequest) {
                samlRequest = QByteArray::fromBase64(QByteArray::fromStdString(xml.readElementText().toStdString()));
            }
        }
    }

    if (samlMethod == nullptr || samlRequest == nullptr) {
        qCritical("This does not appear to be a SAML prelogin response (<saml-auth-method> or <saml-request> tags missing)");
        emit connectFailed();
        return;
    }

    if (samlMethod == "POST") {
        // TODO
        qInfo("TODO: SAML method is POST");
        emit connectFailed();
    } else if (samlMethod == "REDIRECT") {
        qInfo() << "Request URL is: %s" << samlRequest;

        loginWindow->login(samlRequest);
        loginWindow->exec();
    }
    delete reply;
}

void GPClient::onLoginSuccess(QJsonObject loginResult)
{
    qDebug() << "Login success:" << loginResult;

    QString fullpath = "/ssl-vpn/login.esp";
    QString shortpath = "gateway";
    QString user = loginResult.value("saml-username").toString();
    QString cookieName;
    QString cookieValue;
    QString cookies[]{"prelogin-cookie", "portal-userauthcookie"};

    for (int i = 0; i < cookies->length(); i++) {
        cookieValue = loginResult.value(cookies[i]).toString();
        if (cookieValue != nullptr) {
            cookieName = cookies[i];
            break;
        }
    }

    QString host = QString("https://%1/%2:%3").arg(loginResult.value("server").toString(), shortpath, cookieName);
    qDebug() << "Server:" << host << ", User:" << user << "Cookie:" << cookieValue;
    qDebug() << "openconnect --protocol=gp -u" << user << "--passwd-on-stdin" << host;

    vpn->connect(host, user, cookieValue);
}

void GPClient::onVPNConnected()
{
    qDebug() << "VPN connected";
    ui->connectButton->setDisabled(false);
    ui->connectButton->setText("Disconnect");
}

void GPClient::onVPNDisconnected()
{
    qDebug() << "VPN disconnected";
    ui->connectButton->setDisabled(false);
    ui->connectButton->setText("Connect");
}

void GPClient::onVPNLogAvailable(QString log)
{
    qDebug() << log;
}

void GPClient::samlLogin(const QString portal)
{
    const QString preloginUrl = "https://" + portal + "/ssl-vpn/prelogin.esp";
    qDebug("%s", preloginUrl.toStdString().c_str());

    reply = networkManager->post(QNetworkRequest(preloginUrl), (QByteArray) nullptr);
    connect(reply, &QNetworkReply::finished, this, &GPClient::preloginResultFinished);
}
