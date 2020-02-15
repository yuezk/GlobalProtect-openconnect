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
        ui->statusLabel->setText("Not Connected");
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

    int status = vpn->status();
    if (status != 0) {
        ui->statusLabel->setText("Connected");
        ui->connectButton->setText("Disconnect");
    }
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
    QString btnText = ui->connectButton->text();

    if (btnText == "Connect") {
        QString portal = ui->portalInput->text();
        ui->statusLabel->setText("Authenticating...");
        ui->connectButton->setDisabled(true);
        samlLogin(portal);
    } else if (btnText == "Cancel") {
        ui->statusLabel->setText("Canceling...");
        ui->connectButton->setDisabled(true);
        vpn->disconnect();
    } else {
        ui->statusLabel->setText("Disconnecting...");
        ui->connectButton->setDisabled(true);
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

    ui->statusLabel->setText("Connecting...");
    ui->connectButton->setText("Cancel");
    vpn->connect(host, user, cookieValue);
}

void GPClient::onVPNConnected()
{
    ui->statusLabel->setText("Connected");
    ui->connectButton->setText("Disconnect");
    ui->connectButton->setDisabled(false);
}

void GPClient::onVPNDisconnected()
{
    ui->statusLabel->setText("Not Connected");
    ui->connectButton->setText("Connect");
    ui->connectButton->setDisabled(false);
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
