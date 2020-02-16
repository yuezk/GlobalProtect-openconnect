#ifndef GPCLIENT_H
#define GPCLIENT_H

#include "gpservice_interface.h"
#include "samlloginwindow.h"
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
    SAMLLoginWindow *loginWindow;
    QNetworkAccessManager *networkManager;
    QNetworkReply *reply;
    com::yuezk::qt::GPService *vpn;
    QSettings *settings;

    void moveCenter();
    void samlLogin(const QString portal);
};
#endif // GPCLIENT_H
