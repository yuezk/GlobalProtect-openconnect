#ifndef GPCLIENT_H
#define GPCLIENT_H

#include <QtWidgets/QMainWindow>
#include <QtWidgets/QSystemTrayIcon>
#include <QtWidgets/QMenu>
#include <QtWidgets/QPushButton>

#include "portalconfigresponse.h"
#include "settingsdialog.h"
#include "vpn.h"

QT_BEGIN_NAMESPACE
namespace Ui { class GPClient; }
QT_END_NAMESPACE

class GPClient : public QMainWindow
{
    Q_OBJECT

public:
    GPClient(QWidget *parent, IVpn *vpn);
    ~GPClient();

    void activate();
    void quit();

    QString portal() const;
    void portal(QString);

    GPGateway currentGateway() const;
    void setCurrentGateway(const GPGateway gateway);

    void doConnect();

private slots:
    void onSettingsButtonClicked();
    void onSettingsAccepted();

    void on_connectButton_clicked();
    void on_portalInput_returnPressed();
    void on_portalInput_editingFinished();

    void onSystemTrayActivated(QSystemTrayIcon::ActivationReason reason);
    void onGatewayChanged(QAction *action);

    void onPortalSuccess(const PortalConfigResponse portalConfig, const QString region);
    void onPortalPreloginFail(const QString msg);
    void onPortalConfigFail(const QString msg);
    void onPortalFail(const QString &msg);

    void onGatewaySuccess(const QString &authCookie);
    void onGatewayFail(const QString &msg);

    void onVPNConnected();
    void onVPNDisconnected();
    void onVPNError(QString errorMessage);
    void onVPNLogAvailable(QString log);

private:
    enum class VpnStatus
    {
        disconnected,
        pending,
        connected
    };

    Ui::GPClient *ui;
    IVpn *vpn;

    QSystemTrayIcon *systemTrayIcon;
    QMenu *contextMenu;
    QAction *openAction;
    QAction *connectAction;

    QMenu *gatewaySwitchMenu;
    QAction *clearAction;
    QAction *quitAction;

    SettingsDialog *settingsDialog;
    QPushButton *settingsButton;

    bool isQuickConnect { false };
    bool isSwitchingGateway { false };
    PortalConfigResponse portalConfig;

    void setupSettings();

    void initSystemTrayIcon();
    void initVpnStatus();
    void populateGatewayMenu();
    void updateConnectionStatus(const VpnStatus &status);

    void portalLogin();
    void tryGatewayLogin();
    void gatewayLogin();

    bool connected() const;

    QList<GPGateway> allGateways() const;
    void setAllGateways(QList<GPGateway> gateways);

    void clearSettings();
};
#endif // GPCLIENT_H
