#include "gpservice.h"
#include "gpservice_adaptor.h"

#include <QFileInfo>
#include <QtDBus>
#include <QDateTime>
#include <QVariant>

GPService::GPService(QObject *parent)
    : QObject(parent)
    , openconnect(new QProcess)
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

GPService::~GPService()
{
    delete openconnect;
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

/* Port from https://github.com/qt/qtbase/blob/11d1dcc6e263c5059f34b44d531c9ccdf7c0b1d6/src/corelib/io/qprocess.cpp#L2115 */
QStringList GPService::splitCommand(QStringView command)
{
    QStringList args;
    QString tmp;
    int quoteCount = 0;
    bool inQuote = false;

    // handle quoting. tokens can be surrounded by double quotes
    // "hello world". three consecutive double quotes represent
    // the quote character itself.
    for (int i = 0; i < command.size(); ++i) {
        if (command.at(i) == QLatin1Char('"')) {
            ++quoteCount;
            if (quoteCount == 3) {
                // third consecutive quote
                quoteCount = 0;
                tmp += command.at(i);
            }
            continue;
        }
        if (quoteCount) {
            if (quoteCount == 1)
                inQuote = !inQuote;
            quoteCount = 0;
        }
        if (!inQuote && command.at(i).isSpace()) {
            if (!tmp.isEmpty()) {
                args += tmp;
                tmp.clear();
            }
        } else {
            tmp += command.at(i);
        }
    }
    if (!tmp.isEmpty())
        args += tmp;

    return args;
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

void GPService::connect(QString server, QString username, QString passwd, QString extraArgs)
{
    log("VPN status is: " + QVariant::fromValue(vpnStatus).toString());

    if (vpnStatus != GPService::VpnNotConnected) {
        log("VPN status is: " + QVariant::fromValue(vpnStatus).toString());
        return;
    }

    log("Before findBinary");

//    QString bin = findBinary();

//    log("After findBinary");

//    if (bin == nullptr) {
//        log("Could not find openconnect binary, make sure openconnect is installed, exiting.");
//        emit error("The OpenConect CLI was not found, make sure it has been installed!");
//        return;
//    }

    QStringList args;
    args << QCoreApplication::arguments().mid(1)
     << "--protocol=gp"
     << splitCommand(extraArgs)
     << "-u" << username
     << "-C" << passwd
     << server;

    log("Start process with arugments: " + args.join(" "));

    openconnect->start("openconnect", args);
}

void GPService::disconnect()
{
    if (openconnect->state() != QProcess::NotRunning) {
        vpnStatus = GPService::VpnDisconnecting;
        openconnect->terminate();
    }
}

int GPService::status()
{
    return vpnStatus;
}

void GPService::onProcessStarted()
{
    log("Openconnect started successfully, PID=" + QString::number(openconnect->processId()));
    vpnStatus = GPService::VpnConnecting;
}

void GPService::onProcessError(QProcess::ProcessError error)
{
    log("Error occurred: " + QVariant::fromValue(error).toString());
    vpnStatus = GPService::VpnNotConnected;
    emit disconnected();
}

void GPService::onProcessStdout()
{
    QString output = openconnect->readAllStandardOutput();

    log(output);
    if (output.indexOf("Connected as") >= 0) {
        vpnStatus = GPService::VpnConnected;
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
    vpnStatus = GPService::VpnNotConnected;
    emit disconnected();

    if (aboutToQuit) {
        exit(0);
    };
}

void GPService::log(QString msg)
{
    emit logAvailable(msg);
}
