#ifndef GPCLIENT_H
#define GPCLIENT_H

#include "gpservice_interface.h"
#include "portalconfigresponse.h"

#include <QMainWindow>
#include <QSystemTrayIcon>
#include <QMenu>

QT_BEGIN_NAMESPACE
namespace Ui { class GPClient; }
QT_END_NAMESPACE

class GPClient : public QMainWindow
{
    Q_OBJECT

public:
    GPClient(QWidget *parent = nullptr);
    ~GPClient();

    void activate();

private slots:
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
    void onVPNLogAvailable(QString log);

private:
    enum class VpnStatus
    {
        disconnected,
        pending,
        connected
    };

    Ui::GPClient *ui;
    com::yuezk::qt::GPService *vpn;

    QSystemTrayIcon *systemTrayIcon;
    QMenu *contextMenu;
    QAction *openAction;
    QAction *connectAction;

    QMenu *gatewaySwitchMenu;
    QAction *clearAction;
    QAction *quitAction;

    bool isQuickConnect { false };
    bool isSwitchingGateway { false };
    PortalConfigResponse portalConfig;

    void initSystemTrayIcon();
    void initVpnStatus();
    void populateGatewayMenu();
    void updateConnectionStatus(const VpnStatus &status);

    void doConnect();
    void portalLogin();
    void tryGatewayLogin();
    void gatewayLogin();

    QString portal() const;
    bool connected() const;

    QList<GPGateway> allGateways() const;
    void setAllGateways(QList<GPGateway> gateways);

    GPGateway currentGateway() const;
    void setCurrentGateway(const GPGateway gateway);

    void clearSettings();
    void quit();
};
#endif // GPCLIENT_H
