#ifndef GLOBALPROTECTSERVICE_H
#define GLOBALPROTECTSERVICE_H

#include <QObject>
#include <QProcess>

#define NM_OPENCONNECT_USER "nm-openconnect"

static const QString binaryPaths[] {
    "/usr/bin/openconnect",
    "/usr/sbin/openconnect",
    "/usr/local/bin/openconnect",
    "/usr/local/sbin/openconnect",
    "/opt/bin/openconnect",
    "/opt/sbin/openconnect"
};

class GPService : public QObject
{
    Q_OBJECT
    Q_CLASSINFO("D-Bus Interface", "com.yuezk.qt.GPService")
public:
    explicit GPService(QObject *parent = nullptr);

signals:
    void connected();
    void disconnected();
    void logAvailable(QString log);

public slots:
    void connect(QString server, QString username, QString passwd);
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

    void log(QString msg);
    static QString findBinary();
    static char *createPersistentTundev();
    static void destroyPersistentTundev(char *tun_name);
};

#endif // GLOBALPROTECTSERVICE_H
