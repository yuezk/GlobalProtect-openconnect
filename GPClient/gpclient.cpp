#include "gpclient.h"
#include "ui_gpclient.h"
#include "samlloginwindow.h"

#include <QDesktopWidget>
#include <QGraphicsScene>
#include <QGraphicsView>
#include <QGraphicsPixmapItem>
#include <QImage>
#include <QStyle>

GPClient::GPClient(QWidget *parent)
    : QMainWindow(parent)
    , ui(new Ui::GPClient)
{
    ui->setupUi(this);
    setFixedSize(width(), height());
    moveCenter();

    // Restore portal from the previous settings
    settings = new QSettings("com.yuezk.qt", "GPClient");
    ui->portalInput->setText(settings->value("portal", "").toString());

    QObject::connect(this, &GPClient::connectFailed, [this]() {
        updateConnectionStatus("not_connected");
    });

    // QNetworkAccessManager setup
    networkManager = new QNetworkAccessManager(this);

    // DBus service setup
    vpn = new com::yuezk::qt::GPService("com.yuezk.qt.GPService", "/", QDBusConnection::systemBus(), this);
    QObject::connect(vpn, &com::yuezk::qt::GPService::connected, this, &GPClient::onVPNConnected);
    QObject::connect(vpn, &com::yuezk::qt::GPService::disconnected, this, &GPClient::onVPNDisconnected);
    QObject::connect(vpn, &com::yuezk::qt::GPService::logAvailable, this, &GPClient::onVPNLogAvailable);

    initVpnStatus();
}

GPClient::~GPClient()
{
    delete ui;
    delete networkManager;
    delete reply;
    delete vpn;
    delete settings;
}

void GPClient::on_connectButton_clicked()
{
    QString btnText = ui->connectButton->text();

    if (btnText == "Connect") {
        QString portal = ui->portalInput->text();
        settings->setValue("portal", portal);
        ui->statusLabel->setText("Authenticating...");
        updateConnectionStatus("pending");
        doAuth(portal);
    } else if (btnText == "Cancel") {
        ui->statusLabel->setText("Canceling...");
        updateConnectionStatus("pending");

        if (reply->isRunning()) {
            reply->abort();
        }
        vpn->disconnect();
    } else {
        ui->statusLabel->setText("Disconnecting...");
        updateConnectionStatus("pending");
        vpn->disconnect();
    }
}

void GPClient::preloginResultFinished()
{
    if (reply->error()) {
        qWarning() << "Prelogin request error";
        emit connectFailed();
        return;
    }

    QByteArray bytes = reply->readAll();
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
        qWarning("This does not appear to be a SAML prelogin response (<saml-auth-method> or <saml-request> tags missing)");
        emit connectFailed();
        return;
    }

    if (samlMethod == "POST") {
        // TODO
        qDebug("TODO: SAML method is POST");
        emit connectFailed();
    } else if (samlMethod == "REDIRECT") {
        samlLogin(samlRequest);
    }
}

void GPClient::onLoginSuccess(QJsonObject loginResult)
{
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
    vpn->connect(host, user, cookieValue);
    ui->statusLabel->setText("Connecting...");
    updateConnectionStatus("pending");
}

void GPClient::updateConnectionStatus(QString status)
{
    if (status == "not_connected") {
        ui->statusLabel->setText("Not Connected");
        ui->statusImage->setStyleSheet("image: url(:/images/not_connected.png); padding: 15;");
        ui->connectButton->setText("Connect");
        ui->connectButton->setDisabled(false);
    } else if (status == "pending") {
        ui->statusImage->setStyleSheet("image: url(:/images/pending.png); padding: 15;");
        ui->connectButton->setText("Cancel");
        ui->connectButton->setDisabled(false);
    } else if (status == "connected") {
        ui->statusLabel->setText("Connected");
        ui->statusImage->setStyleSheet("image: url(:/images/connected.png); padding: 15;");
        ui->connectButton->setText("Disconnect");
        ui->connectButton->setDisabled(false);
    }
}

void GPClient::onVPNConnected()
{
    updateConnectionStatus("connected");
}

void GPClient::onVPNDisconnected()
{
    updateConnectionStatus("not_connected");
}

void GPClient::onVPNLogAvailable(QString log)
{
    qInfo() << log;
}

void GPClient::initVpnStatus() {
    int status = vpn->status();
    if (status == 1) {
        ui->statusLabel->setText("Connecting...");
        updateConnectionStatus("pending");
    } else if (status == 2) {
        updateConnectionStatus("connected");
    } else if (status == 3) {
        ui->statusLabel->setText("Disconnecting...");
        updateConnectionStatus("pending");
    }
}

void GPClient::moveCenter()
{
    QDesktopWidget *desktop = QApplication::desktop();

    int screenWidth, width;
    int screenHeight, height;
    int x, y;
    QSize windowSize;

    screenWidth = desktop->width();
    screenHeight = desktop->height();

    windowSize = size();
    width = windowSize.width();
    height = windowSize.height();

    x = (screenWidth - width) / 2;
    y = (screenHeight - height) / 2;
    y -= 50;
    move(x, y);
}

void GPClient::doAuth(const QString portal)
{
    const QString preloginUrl = "https://" + portal + "/ssl-vpn/prelogin.esp";
    reply = networkManager->post(QNetworkRequest(preloginUrl), (QByteArray) nullptr);
    connect(reply, &QNetworkReply::finished, this, &GPClient::preloginResultFinished);
}

void GPClient::samlLogin(const QString loginUrl)
{
    SAMLLoginWindow *loginWindow = new SAMLLoginWindow(this);

    QObject::connect(loginWindow, &SAMLLoginWindow::success, this, &GPClient::onLoginSuccess);
    QObject::connect(loginWindow, &SAMLLoginWindow::rejected, this, &GPClient::connectFailed);

    loginWindow->login(loginUrl);
    loginWindow->exec();
    delete loginWindow;
}
