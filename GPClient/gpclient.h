#ifndef GPCLIENT_H
#define GPCLIENT_H

#include "gpservice_interface.h"
#include <QMainWindow>
#include <QNetworkAccessManager>
#include <QNetworkReply>

QT_BEGIN_NAMESPACE
namespace Ui { class GPClient; }
QT_END_NAMESPACE

class GPClient : public QMainWindow
{
    Q_OBJECT

public:
    GPClient(QWidget *parent = nullptr);
    ~GPClient();

signals:
    void connectFailed();

private slots:
    void on_connectButton_clicked();
    void preloginResultFinished();

    void onLoginSuccess(QJsonObject loginResult);

    void onVPNConnected();
    void onVPNDisconnected();
    void onVPNLogAvailable(QString log);

private:
    Ui::GPClient *ui;
    QNetworkAccessManager *networkManager;
    QNetworkReply *reply;
    com::yuezk::qt::GPService *vpn;
    QSettings *settings;

    void initVpnStatus();
    void moveCenter();
    void updateConnectionStatus(QString status);
    void doAuth(const QString portal);
    void samlLogin(const QString loginUrl);
};
#endif // GPCLIENT_H
