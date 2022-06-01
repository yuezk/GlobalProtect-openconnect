#include <QtGui/QIcon>
#include <plog/Log.h>

#include "gpclient.h"
#include "gphelper.h"
#include "ui_gpclient.h"
#include "portalauthenticator.h"
#include "gatewayauthenticator.h"
#include "settingsdialog.h"
#include "gatewayauthenticatorparams.h"

using namespace gpclient::helper;

GPClient::GPClient(QWidget *parent, IVpn *vpn)
    : QMainWindow(parent)
    , ui(new Ui::GPClient)
    , vpn(vpn)
    , settingsDialog(new SettingsDialog(this))
{
    ui->setupUi(this);

    setWindowTitle("GlobalProtect");
    setFixedSize(width(), height());
    gpclient::helper::moveCenter(this);

    setupSettings();

    // Restore portal from the previous settings
    this->portal(settings::get("portal", "").toString());

    // DBus service setup
    QObject *ov = dynamic_cast<QObject*>(vpn);
    connect(ov, SIGNAL(connected()), this, SLOT(onVPNConnected()));
    connect(ov, SIGNAL(disconnected()), this, SLOT(onVPNDisconnected()));
    connect(ov, SIGNAL(error(QString)), this, SLOT(onVPNError(QString)));
    connect(ov, SIGNAL(logAvailable(QString)), this, SLOT(onVPNLogAvailable(QString)));

    // Initialize the context menu of system tray.
    initSystemTrayIcon();
    initVpnStatus();
}

GPClient::~GPClient()
{
    delete ui;
    delete vpn;
    delete settingsDialog;
    delete settingsButton;
}

void GPClient::setupSettings()
{
    settingsButton = new QPushButton(this);
    settingsButton->setIcon(QIcon(":/images/settings_icon.png"));
    settingsButton->setFixedSize(QSize(28, 28));

    QRect rect = this->geometry();
    settingsButton->setGeometry(
                rect.width() - settingsButton->width() - 15,
                15,
                settingsButton->geometry().width(),
                settingsButton->geometry().height()
                );

    connect(settingsButton, &QPushButton::clicked, this, &GPClient::onSettingsButtonClicked);
    connect(settingsDialog, &QDialog::accepted, this, &GPClient::onSettingsAccepted);
}

void GPClient::onSettingsButtonClicked()
{
    settingsDialog->setClientos(settings::get("clientos", "Linux").toString());
    settingsDialog->show();
}

void GPClient::onSettingsAccepted()
{
    settings::save("clientos", settingsDialog->clientos());
}

void GPClient::on_connectButton_clicked()
{
    doConnect();
}

void GPClient::on_portalInput_returnPressed()
{
    doConnect();
}

void GPClient::on_portalInput_editingFinished()
{
    populateGatewayMenu();
}

void GPClient::initSystemTrayIcon()
{
    systemTrayIcon = new QSystemTrayIcon(this);
    contextMenu = new QMenu("GlobalProtect", this);

    gatewaySwitchMenu = new QMenu("Switch Gateway", this);
    gatewaySwitchMenu->setIcon(QIcon::fromTheme("network-workgroup"));
    populateGatewayMenu();

    systemTrayIcon->setIcon(QIcon(":/images/not_connected.png"));
    systemTrayIcon->setToolTip("GlobalProtect");
    systemTrayIcon->setContextMenu(contextMenu);

    connect(systemTrayIcon, &QSystemTrayIcon::activated, this, &GPClient::onSystemTrayActivated);
    connect(gatewaySwitchMenu, &QMenu::triggered, this, &GPClient::onGatewayChanged);

    openAction = contextMenu->addAction(QIcon::fromTheme("window-new"), "Open", this, &GPClient::activate);
    connectAction = contextMenu->addAction(QIcon::fromTheme("preferences-system-network"), "Connect", this, &GPClient::doConnect);
    contextMenu->addMenu(gatewaySwitchMenu);
    contextMenu->addSeparator();
    clearAction = contextMenu->addAction(QIcon::fromTheme("edit-clear"), "Reset Settings", this, &GPClient::clearSettings);
    quitAction = contextMenu->addAction(QIcon::fromTheme("application-exit"), "Quit", this, &GPClient::quit);

    systemTrayIcon->show();
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

void GPClient::populateGatewayMenu()
{
    PLOGI << "Populating the Switch Gateway menu...";

    const QList<GPGateway> gateways = allGateways();
    gatewaySwitchMenu->clear();

    if (gateways.isEmpty()) {
        gatewaySwitchMenu->addAction("<None>")->setData(-1);
        return;
    }

    const QString currentGatewayName = currentGateway().name();
    for (int i = 0; i < gateways.length(); i++) {
        const GPGateway g = gateways.at(i);
        QString iconImage = ":/images/radio_unselected.png";
        if (g.name() == currentGatewayName) {
            iconImage = ":/images/radio_selected.png";
        }
        gatewaySwitchMenu->addAction(QIcon(iconImage), QString("%1 (%2)").arg(g.name(), g.address()))->setData(i);
    }
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
            gatewaySwitchMenu->setEnabled(true);
            clearAction->setEnabled(true);
            break;
        case VpnStatus::pending:
            ui->statusImage->setStyleSheet("image: url(:/images/pending.png); padding: 15;");
            ui->connectButton->setDisabled(true);
            ui->portalInput->setReadOnly(true);

            systemTrayIcon->setIcon(QIcon{ ":/images/pending.png" });
            connectAction->setEnabled(false);
            gatewaySwitchMenu->setEnabled(false);
            clearAction->setEnabled(false);
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
            gatewaySwitchMenu->setEnabled(true);
            clearAction->setEnabled(false);
            break;
        default:
            break;
    }
}

void GPClient::onSystemTrayActivated(QSystemTrayIcon::ActivationReason reason)
{
    switch (reason) {
        case QSystemTrayIcon::Trigger:
        case QSystemTrayIcon::DoubleClick:
            this->activate();
            break;
        default:
            break;
    }
}

void GPClient::onGatewayChanged(QAction *action)
{
    const int index = action->data().toInt();

    if (index == -1) {
        return;
    }

    const GPGateway g = allGateways().at(index);

    // If the selected gateway is the same as the current gateway
    if (g.name() == currentGateway().name()) {
        return;
    }

    setCurrentGateway(g);

    if (connected()) {
        ui->statusLabel->setText("Switching Gateway...");
        ui->connectButton->setEnabled(false);

        vpn->disconnect();
        isSwitchingGateway = true;
    }
}

void GPClient::doConnect()
{
    PLOGI << "Start connecting...";

    const QString btnText = ui->connectButton->text();
    const QString portal = this->portal();

    // Display the main window if portal is empty
    if (portal.isEmpty()) {
        activate();
        return;
    }

    if (btnText.endsWith("Connect")) {
        settings::save("portal", portal);

        // Login to the previously saved gateway
        if (!currentGateway().name().isEmpty()) {
            PLOGI << "Start gateway login using the previously saved gateway...";
            isQuickConnect = true;
            gatewayLogin();
        } else {
            // Perform the portal login
            PLOGI << "Start portal login...";
            portalLogin();
        }
    } else {
        PLOGI << "Start disconnecting the VPN...";

        ui->statusLabel->setText("Disconnecting...");
        updateConnectionStatus(VpnStatus::pending);
        vpn->disconnect();
    }
}

// Login to the portal interface to get the portal config and preferred gateway
void GPClient::portalLogin()
{
    PortalAuthenticator *portalAuth = new PortalAuthenticator(portal(), settings::get("clientos", "Linux").toString());

    connect(portalAuth, &PortalAuthenticator::success, [this, portalAuth](const PortalConfigResponse response, const QString region) {
        this->onPortalSuccess(response, region);
        portalAuth->deleteLater();
    });
    // Prelogin failed on the portal interface, try to treat the portal as a gateway interface
    connect(portalAuth, &PortalAuthenticator::preloginFailed, [this, portalAuth](const QString msg) {
        this->onPortalPreloginFail(msg);
        portalAuth->deleteLater();
    });
    connect(portalAuth, &PortalAuthenticator::portalConfigFailed, [this, portalAuth](const QString msg) {
        this->onPortalConfigFail(msg);
        portalAuth->deleteLater();
    });
    // Portal login failed
    connect(portalAuth, &PortalAuthenticator::fail, [this, portalAuth](const QString &msg) {
        this->onPortalFail(msg);
        portalAuth->deleteLater();
    });

    ui->statusLabel->setText("Authenticating...");
    updateConnectionStatus(VpnStatus::pending);
    portalAuth->authenticate();
}

void GPClient::onPortalSuccess(const PortalConfigResponse portalConfig, const QString region)
{
    PLOGI << "Portal authentication succeeded.";

    // No gateway found in protal configuration
    if (portalConfig.allGateways().size() == 0) {
        PLOGI << "No gateway found in portal configuration, treat the portal address as a gateway.";
        tryGatewayLogin();
        return;
    }

    GPGateway gateway = filterPreferredGateway(portalConfig.allGateways(), region);
    setAllGateways(portalConfig.allGateways());
    setCurrentGateway(gateway);
    this->portalConfig = portalConfig;

    gatewayLogin();
}

void GPClient::onPortalPreloginFail(const QString msg)
{
    PLOGI << "Portal prelogin failed: " << msg;
    tryGatewayLogin();
}

void GPClient::onPortalConfigFail(const QString msg)
{
    PLOGI << "Failed to get the portal configuration, " << msg << " Treat the portal address as gateway.";
    tryGatewayLogin();
}

void GPClient::onPortalFail(const QString &msg)
{
    if (!msg.isEmpty()) {
        openMessageBox("Portal authentication failed.", msg);
    }

    updateConnectionStatus(VpnStatus::disconnected);
}

void GPClient::tryGatewayLogin()
{
    PLOGI << "Try to preform login on the the gateway interface...";

    // Treat the portal input as the gateway address
    GPGateway g;
    g.setName(portal());
    g.setAddress(portal());

    QList<GPGateway> gateways;
    gateways.append(g);

    setAllGateways(gateways);
    setCurrentGateway(g);

    gatewayLogin();
}

// Login to the gateway
void GPClient::gatewayLogin()
{
    PLOGI << "Performing gateway login...";

    GatewayAuthenticatorParams params = GatewayAuthenticatorParams::fromPortalConfigResponse(portalConfig);
    params.setClientos(settings::get("clientos", "Linux").toString());

    GatewayAuthenticator *gatewayAuth = new GatewayAuthenticator(currentGateway().address(), params);

    connect(gatewayAuth, &GatewayAuthenticator::success, [this, gatewayAuth](const QString &authToken) {
        this->onGatewaySuccess(authToken);
        gatewayAuth->deleteLater();
    });
    connect(gatewayAuth, &GatewayAuthenticator::fail, [this, gatewayAuth](const QString &msg) {
        this->onGatewayFail(msg);
        gatewayAuth->deleteLater();
    });

    ui->statusLabel->setText("Authenticating...");
    updateConnectionStatus(VpnStatus::pending);
    gatewayAuth->authenticate();
}

void GPClient::onGatewaySuccess(const QString &authCookie)
{
    PLOGI << "Gateway login succeeded, got the cookie " << authCookie;

    isQuickConnect = false;
    QList<QString> gatewayAddresses;
    for (GPGateway &gw : allGateways()) {
      gatewayAddresses.push_back(gw.address());
    }
    vpn->connect(currentGateway().address(), gatewayAddresses, portalConfig.username(), authCookie);
    ui->statusLabel->setText("Connecting...");
    updateConnectionStatus(VpnStatus::pending);
}

void GPClient::onGatewayFail(const QString &msg)
{
    // If the quick connect on gateway failed, perform the portal login
    if (isQuickConnect && !msg.isEmpty()) {
        isQuickConnect = false;
        portalLogin();
        return;
    }

    if (!msg.isEmpty()) {
        openMessageBox("Gateway authentication failed.", msg);
    }

    updateConnectionStatus(VpnStatus::disconnected);
}

void GPClient::activate()
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

void GPClient::portal(QString p)
{
    ui->portalInput->setText(p);
}

bool GPClient::connected() const
{
    const QString statusText = ui->statusLabel->text();
    return statusText.contains("Connected") && !statusText.contains("Not");
}

QList<GPGateway> GPClient::allGateways() const
{
    const QString gatewaysJson = settings::get(portal() + "_gateways").toString();
    return GPGateway::fromJson(gatewaysJson);
}

void GPClient::setAllGateways(QList<GPGateway> gateways)
{
    PLOGI << "Updating all the gateways...";

    settings::save(portal() + "_gateways", GPGateway::serialize(gateways));
    populateGatewayMenu();
}

GPGateway GPClient::currentGateway() const
{
    const QString selectedGateway = settings::get(portal() + "_selectedGateway").toString();

    for (auto g : allGateways()) {
        if (g.name() == selectedGateway) {
            return g;
        }
    }
    return GPGateway{};
}

void GPClient::setCurrentGateway(const GPGateway gateway)
{
    PLOGI << "Updating the current gateway to " << gateway.name();

    settings::save(portal() + "_selectedGateway", gateway.name());
    populateGatewayMenu();
}

void GPClient::clearSettings()
{
    settings::clear();
    populateGatewayMenu();
    ui->portalInput->clear();
}

void GPClient::quit()
{
    vpn->disconnect();
    QApplication::quit();
}

void GPClient::onVPNConnected()
{
    updateConnectionStatus(VpnStatus::connected);
}

void GPClient::onVPNDisconnected()
{
    updateConnectionStatus(VpnStatus::disconnected);

    if (isSwitchingGateway) {
        gatewayLogin();
        isSwitchingGateway = false;
    }
}

void GPClient::onVPNError(QString errorMessage)
{
    updateConnectionStatus(VpnStatus::disconnected);
    openMessageBox("Failed to connect", errorMessage);
}

void GPClient::onVPNLogAvailable(QString log)
{
    PLOGI << log;
}
