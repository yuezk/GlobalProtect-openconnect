#include "gpservice.h"
#include "gpservice_adaptor.h"

#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <unistd.h>
#include <fcntl.h>
#include <signal.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <sys/ioctl.h>
#include <linux/if_tun.h>
#include <net/if.h>
#include <pwd.h>
#include <grp.h>

#include <QFileInfo>
#include <QDebug>
#include <QtDBus>
#include <QIODevice>
#include <QProcess>
#include <QDateTime>
#include <QVariant>

struct {
    uid_t tun_owner;
    gid_t tun_group;
} tun_user;

class SandboxProcess : public QProcess
{
protected:
    void setupChildProcess() override;
};

void SandboxProcess::setupChildProcess()
{
    /*if (initgroups (NM_OPENCONNECT_USER, tun_user.tun_group) ||
            setgid (tun_user.tun_group) ||
            setuid (tun_user.tun_owner)) {
        qDebug() << "Failed to drop privileges when spawning openconnect";
    }*/
}

GPService::GPService(QObject *parent)
    : QObject(parent)
    , openconnect(new SandboxProcess)
{
    // Register the DBus service
    new GPServiceAdaptor(this);
    QDBusConnection dbus = QDBusConnection::systemBus();
    dbus.registerObject("/", this);
    dbus.registerService("com.yuezk.qt.GPService");

    // Setup the openconnect process
    QObject::connect(openconnect, &QProcess::started, this, &GPService::onProcessStarted);
    QObject::connect(openconnect, &QProcess::errorOccurred, this, &GPService::onProcessError);
    QObject::connect(openconnect, &QProcess::readyReadStandardOutput, this, &GPService::onProcessStdout);
    QObject::connect(openconnect, &QProcess::readyReadStandardError, this, &GPService::onProcessStderr);
    QObject::connect(openconnect, QOverload<int, QProcess::ExitStatus>::of(&QProcess::finished), this, &GPService::onProcessFinished);
}

void GPService::quit()
{
    if (openconnect->state() == QProcess::NotRunning) {
        exit(0);
    } else {
        aboutToQuit = true;
        openconnect->terminate();
    }
}

void GPService::connect(QString server, QString username, QString passwd)
{
    if (status() != QProcess::NotRunning) {
        log("Openconnect has already started on PID " + QString::number(openconnect->processId()) + ", nothing changed.");
        return;
    }

    QString bin = findBinary();
    if (bin == nullptr) {
        log("Could not found openconnect binary, make sure openconnect is installed, exiting.");
        return;
    }

    char *tunName = "tun0"; // createPersistentTundev();
    // Failed to create device
    if (tunName == nullptr) {
        log("Could not create tun, exiting.");
        return;
    }

    // openconnect --protocol=gp -i vpn0 -s 'sudo -E /etc/vpnc/vpnc-script' -u "zyue@microstrategy.com" --passwd-on-stdin "https://vpn.microstrategy.com/gateway:prelogin-cookie"
    QStringList args;
    args << "--protocol=gp"
         << "--no-dtls"
        // << "-i" << tunName
        // << "-s" << "sudo -E /etc/vpnc/vpnc-script"
        // << "-U" << NM_OPENCONNECT_USER
         << "-u" << username
         << "--passwd-on-stdin"
         << server;

    openconnect->start(bin, args);
    openconnect->write(passwd.toUtf8());
    openconnect->closeWriteChannel();
}

void GPService::disconnect()
{
    if (openconnect->state() != QProcess::NotRunning) {
        openconnect->terminate();
    }
}

int GPService::status()
{
    return openconnect->state();
}

void GPService::onProcessStarted()
{
    log("Openconnect started successfully, PID=" + QString::number(openconnect->processId()));
}

void GPService::onProcessError(QProcess::ProcessError error)
{
    log("Error occurred: " + QVariant::fromValue(error).toString());
    emit disconnected();
}

void GPService::onProcessStdout()
{
    QString output = openconnect->readAllStandardOutput();

    log(output);
    if (output.startsWith("Connected as")) {
        emit connected();
    }
}

void GPService::onProcessStderr()
{
    log(openconnect->readAllStandardError());
}

void GPService::onProcessFinished(int exitCode, QProcess::ExitStatus exitStatus)
{
    log("Openconnect process exited with code " + QString::number(exitCode) + " and exit status " + QVariant::fromValue(exitStatus).toString());
    emit disconnected();

    if (aboutToQuit) {
        exit(0);
    };
}

void GPService::log(QString msg)
{
    // 2020-02-12 15:33:45.120: log messsage
    QString record = QDateTime::currentDateTime().toString("yyyy-MM-dd hh:mm:ss.zzz") + ": " + msg;
    qDebug() << record;
    emit logAvailable(record);
}

QString GPService::findBinary()
{
    for (int i = 0; i < binaryPaths->length(); i++) {
        if (QFileInfo::exists(binaryPaths[i])) {
            return binaryPaths[i];
        }
    }

    return nullptr;
}

char *GPService::createPersistentTundev()
{
    struct passwd *pw;
    struct ifreq ifr;
    int fd;
    int i;

    pw = getpwnam(NM_OPENCONNECT_USER);
    if (!pw)
        return nullptr;

    tun_user.tun_owner = pw->pw_uid;
    tun_user.tun_group = pw->pw_gid;

    fd = open("/dev/net/tun", O_RDWR);
    if (fd < 0) {
        qDebug("Failed to open /dev/net/tun");
        return nullptr;
    }

    memset(&ifr, 0, sizeof(ifr));
    ifr.ifr_flags = IFF_TUN | IFF_NO_PI;

    for (i = 0; i < 256; i++) {
        sprintf(ifr.ifr_name, "gpvpn%d", i);

        int retcode = ioctl(fd, TUNSETIFF, (void *)&ifr);

        if (!retcode) {
            break;
        }
    }

    if (i == 256) {
        qDebug("Failed to create tun");
        return nullptr;
    }

    if (ioctl(fd, TUNSETOWNER, tun_user.tun_owner) < 0) {
        qDebug("TUNSETOWNER");
        return nullptr;
    }

    if (ioctl(fd, TUNSETPERSIST, 1)) {
        qDebug("TUNSETPERSIST");
        return nullptr;
    }
    close(fd);
    qDebug("Created tundev %s\n", ifr.ifr_name);
    return strdup(ifr.ifr_name);
}

void GPService::destroyPersistentTundev(char *tun_name)
{
    struct ifreq ifr;
    int fd;

    fd = open("/dev/net/tun", O_RDWR);
    if (fd < 0) {
        qDebug() << "Failed to open /dev/net/tun";
        return;
    }

    memset(&ifr, 0, sizeof(ifr));
    ifr.ifr_flags = IFF_TUN | IFF_NO_PI;
    strcpy(ifr.ifr_name, tun_name);

    if (ioctl(fd, TUNSETIFF, (void *)&ifr) < 0) {
        qDebug() << "TUNSETIFF";
        return;
    }

    if (ioctl(fd, TUNSETPERSIST, 0)) {
        qDebug() << "TUNSETPERSIST";
        return;
    }

    qDebug() << "Destroyed  tundev %s\n" << tun_name;
    close(fd);
}
