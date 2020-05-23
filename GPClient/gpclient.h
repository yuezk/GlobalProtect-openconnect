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
    void activiate();

private slots:
    void on_connectButton_clicked();
    void on_portalInput_returnPressed();

    void onPortalSuccess(const PortalConfigResponse &portalConfig, const GPGateway &gateway);
    void onPortalPreloginFail();
    void onPortalFail(const QString &msg);
    void onGatewaySuccess(const QString &authCookie);
    void onGatewayFail(const QString &msg);

    void onVPNConnected();
    void onVPNDisconnected();
    void onVPNLogAvailable(QString log);

    void onSystemTrayActivated(QSystemTrayIcon::ActivationReason reason);

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
    QAction *quitAction;

    GPGateway gateway;
    PortalConfigResponse portalConfig;

    QString portal() const;

    void initVpnStatus();
    void doConnect();
    void updateConnectionStatus(const VpnStatus &status);

    void portalLogin(const QString& portal);
    void gatewayLogin() const;

    void quit();
};
#endif // GPCLIENT_H
