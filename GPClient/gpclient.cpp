#include "gpclient.h"
#include "gphelper.h"
#include "ui_gpclient.h"
#include "portalauthenticator.h"
#include "gatewayauthenticator.h"

#include <plog/Log.h>
#include <QIcon>

using namespace gpclient::helper;

GPClient::GPClient(QWidget *parent)
    : QMainWindow(parent)
    , ui(new Ui::GPClient)
    , systemTrayIcon(new QSystemTrayIcon(parent))
    , contextMenu(new QMenu("GlobalProtect", parent))
{
    ui->setupUi(this);
    setWindowTitle("GlobalProtect");
    setFixedSize(width(), height());
    gpclient::helper::moveCenter(this);

    // Restore portal from the previous settings
    ui->portalInput->setText(settings::get("portal", "").toString());

    // DBus service setup
    vpn = new com::yuezk::qt::GPService("com.yuezk.qt.GPService", "/", QDBusConnection::systemBus(), this);
    connect(vpn, &com::yuezk::qt::GPService::connected, this, &GPClient::onVPNConnected);
    connect(vpn, &com::yuezk::qt::GPService::disconnected, this, &GPClient::onVPNDisconnected);
    connect(vpn, &com::yuezk::qt::GPService::logAvailable, this, &GPClient::onVPNLogAvailable);

    connect(systemTrayIcon, &QSystemTrayIcon::activated, this, &GPClient::onSystemTrayActivated);

    // Initiallize the context menu of system tray.
    openAction = contextMenu->addAction(QIcon::fromTheme("system-run"), "Open", this, &GPClient::activiate);
    connectAction = contextMenu->addAction(QIcon::fromTheme("preferences-system-network"), "Connect", this, &GPClient::doConnect);
    contextMenu->addSeparator();
    quitAction = contextMenu->addAction(QIcon::fromTheme("application-exit"), "Quit", this, &GPClient::quit);
    systemTrayIcon->setContextMenu(contextMenu);
    systemTrayIcon->setToolTip("GlobalProtect");

    initVpnStatus();
    systemTrayIcon->show();
}

GPClient::~GPClient()
{
    delete ui;
    delete vpn;
    delete systemTrayIcon;
    delete openAction;
    delete connectAction;
    delete quitAction;
    delete contextMenu;
}

void GPClient::on_connectButton_clicked()
{
    doConnect();
}

void GPClient::on_portalInput_returnPressed()
{
    doConnect();
}

void GPClient::updateConnectionStatus(const GPClient::VpnStatus &status)
{
    switch (status) {
        case VpnStatus::disconnected:
            ui->statusLabel->setText("Not Connected");
            ui->statusImage->setStyleSheet("image: url(:/images/not_connected.png); padding: 15;");
            ui->connectButton->setText("Connect");
            ui->connectButton->setDisabled(false);
            ui->portalInput->setReadOnly(false);

            systemTrayIcon->setIcon(QIcon{ ":/images/not_connected.png" });
            connectAction->setEnabled(true);
            connectAction->setText("Connect");
            break;
        case VpnStatus::pending:
            ui->statusImage->setStyleSheet("image: url(:/images/pending.png); padding: 15;");
            ui->connectButton->setDisabled(true);
            ui->portalInput->setReadOnly(true);

            systemTrayIcon->setIcon(QIcon{ ":/images/pending.png" });
            connectAction->setEnabled(false);
            break;
        case VpnStatus::connected:
            ui->statusLabel->setText("Connected");
            ui->statusImage->setStyleSheet("image: url(:/images/connected.png); padding: 15;");
            ui->connectButton->setText("Disconnect");
            ui->connectButton->setDisabled(false);
            ui->portalInput->setReadOnly(true);

            systemTrayIcon->setIcon(QIcon{ ":/images/connected.png" });
            connectAction->setEnabled(true);
            connectAction->setText("Disconnect");
            break;
        default:
            break;
    }
}

void GPClient::onVPNConnected()
{
    updateConnectionStatus(VpnStatus::connected);
}

void GPClient::onVPNDisconnected()
{
    updateConnectionStatus(VpnStatus::disconnected);
}

void GPClient::onVPNLogAvailable(QString log)
{
    PLOGI << log;
}

void GPClient::onSystemTrayActivated(QSystemTrayIcon::ActivationReason reason)
{
    switch (reason) {
        case QSystemTrayIcon::Trigger:
        case QSystemTrayIcon::DoubleClick:
            this->activiate();
            break;
        default:
            break;
    }
}

void GPClient::activiate()
{
    activateWindow();
    showNormal();
}

QString GPClient::portal() const
{
    const QString input = ui->portalInput->text().trimmed();

    if (input.startsWith("http")) {
        return QUrl(input).authority();
    }
    return input;
}

void GPClient::initVpnStatus() {
    int status = vpn->status();

    if (status == 1) {
        ui->statusLabel->setText("Connecting...");
        updateConnectionStatus(VpnStatus::pending);
    } else if (status == 2) {
        updateConnectionStatus(VpnStatus::connected);
    } else if (status == 3) {
        ui->statusLabel->setText("Disconnecting...");
        updateConnectionStatus(VpnStatus::pending);
    } else {
        updateConnectionStatus(VpnStatus::disconnected);
    }
}

void GPClient::doConnect()
{
    const QString btnText = ui->connectButton->text();
    const QString portal = this->portal();

    if (portal.isEmpty()) {
        activiate();
        return;
    }

    if (btnText.endsWith("Connect")) {
        settings::save("portal", portal);
        ui->statusLabel->setText("Authenticating...");
        updateConnectionStatus(VpnStatus::pending);

        // Perform the portal login
        portalLogin(portal);
    } else {
        ui->statusLabel->setText("Disconnecting...");
        updateConnectionStatus(VpnStatus::pending);

        vpn->disconnect();
    }
}

// Login to the portal interface to get the portal config and preferred gateway
void GPClient::portalLogin(const QString& portal)
{
    PortalAuthenticator *portalAuth = new PortalAuthenticator(portal);

    connect(portalAuth, &PortalAuthenticator::success, this, &GPClient::onPortalSuccess);
    // Prelogin failed on the portal interface, try to treat the portal as a gateway interface
    connect(portalAuth, &PortalAuthenticator::preloginFailed, this, &GPClient::onPortalPreloginFail);
    // Portal login failed
    connect(portalAuth, &PortalAuthenticator::fail, this, &GPClient::onPortalFail);

    portalAuth->authenticate();
}

void GPClient::onPortalSuccess(const PortalConfigResponse &portalConfig, const GPGateway &gateway)
{
    this->portalConfig = portalConfig;
    this->gateway = gateway;

    gatewayLogin();
}

void GPClient::onPortalPreloginFail()
{
    PLOGI << "Portal prelogin failed, try to preform login on the the gateway interface...";

    // Set the gateway address to portal input
    gateway.setAddress(portal());
    gatewayLogin();
}

void GPClient::onPortalFail(const QString &msg)
{
    if (!msg.isEmpty()) {
        openMessageBox("Portal authentication failed.", msg);
    }

    updateConnectionStatus(VpnStatus::disconnected);
}

// Login to the gateway
void GPClient::gatewayLogin() const
{
    GatewayAuthenticator *gatewayAuth = new GatewayAuthenticator(gateway.address(), portalConfig);

    connect(gatewayAuth, &GatewayAuthenticator::success, this, &GPClient::onGatewaySuccess);
    connect(gatewayAuth, &GatewayAuthenticator::fail, this, &GPClient::onGatewayFail);

    gatewayAuth->authenticate();
}

void GPClient::quit()
{
    vpn->disconnect();
    QApplication::quit();
}

void GPClient::onGatewaySuccess(const QString &authCookie)
{
    PLOGI << "Gateway login succeeded, got the cookie " << authCookie;

    vpn->connect(gateway.address(), portalConfig.username(), authCookie);
    ui->statusLabel->setText("Connecting...");
    updateConnectionStatus(VpnStatus::pending);
}

void GPClient::onGatewayFail(const QString &msg)
{
    if (!msg.isEmpty()) {
        openMessageBox("Portal authentication failed.", msg);
    }

    updateConnectionStatus(VpnStatus::disconnected);
}
