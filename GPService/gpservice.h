#ifndef GLOBALPROTECTSERVICE_H
#define GLOBALPROTECTSERVICE_H

#include <QObject>
#include <QProcess>

static const QString binaryPaths[] {
    "/usr/local/bin/openconnect",
    "/usr/local/sbin/openconnect",
    "/usr/bin/openconnect",
    "/usr/sbin/openconnect",
    "/opt/bin/openconnect",
    "/opt/sbin/openconnect"
};

class GPService : public QObject
{
    Q_OBJECT
    Q_CLASSINFO("D-Bus Interface", "com.yuezk.qt.GPService")
public:
    explicit GPService(QObject *parent = nullptr);
    ~GPService();

    enum VpnStatus {
        VpnNotConnected,
        VpnConnecting,
        VpnConnected,
        VpnDisconnecting,
    };

signals:
    void connected();
    void disconnected();
    void error(QString errorMessage);
    void logAvailable(QString log);

public slots:
    void connect(QString server, QString username, QString passwd, QString extraArgs);
    void disconnect();
    int status();
    void quit();

private slots:
    void onProcessStarted();
    void onProcessError(QProcess::ProcessError error);
    void onProcessStdout();
    void onProcessStderr();
    void onProcessFinished(int exitCode, QProcess::ExitStatus exitStatus);

private:
    QProcess *openconnect;
    bool aboutToQuit = false;
    int vpnStatus = GPService::VpnNotConnected;

    void log(QString msg);
    static QString findBinary();
    static QStringList splitCommand(QString command);
};

#endif // GLOBALPROTECTSERVICE_H
