#include <QtCore/QFileInfo>
#include <QtCore/QDateTime>
#include <QtCore/QVariant>
#include <QtCore/QRegularExpression>
#include <QtCore/QRegularExpressionMatch>
#include <QtDBus/QtDBus>

#include "INIReader.h"
#include "gpservice.h"
#include "gpserviceadaptor.h"

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
    for (auto& binaryPath : binaryPaths) {
        if (QFileInfo::exists(binaryPath)) {
            return binaryPath;
        }
    }
    return nullptr;
}

QString GPService::extraOpenconnectArgs(const QString &gateway)
{
    INIReader reader("/etc/gpservice/gp.conf");

    if (reader.ParseError() < 0) {
        return "";
    }

    std::string defaultArgs = reader.Get("*", "openconnect-args", "");
    std::string extraArgs = reader.Get(gateway.toStdString(), "openconnect-args", defaultArgs);

    return QString::fromStdString(extraArgs);
}

/* Port from https://github.com/qt/qtbase/blob/11d1dcc6e263c5059f34b44d531c9ccdf7c0b1d6/src/corelib/io/qprocess.cpp#L2115 */
QStringList GPService::splitCommand(const QString &command)
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

void GPService::connect(QString server, QString username, QString passwd)
{
    if (vpnStatus != GPService::VpnNotConnected) {
        log("VPN status is: " + QVariant::fromValue(vpnStatus).toString());
        return;
    }

    QString bin = findBinary();
    if (bin == nullptr) {
        log("Could not find openconnect binary, make sure openconnect is installed, exiting.");
        emit error("The OpenConect CLI was not found, make sure it has been installed!");
        return;
    }

    if (!isValidVersion(bin)) {
        return;
    }

    const QString extraArgs = extraOpenconnectArgs(server);
    log(QString("Got extra OpenConnect args for server: %1, %2").arg(server, extraArgs.isEmpty() ? "<empty>" : extraArgs));

    QStringList args;
    args << QCoreApplication::arguments().mid(1)
         << "--protocol=gp"
         << splitCommand(extraArgs)
         << "-u" << username
         << "--cookie-on-stdin"
         << server;

    log("Start process with arugments: " + args.join(", "));

    openconnect->start(bin, args);
    openconnect->write((passwd + "\n").toUtf8());
}

bool GPService::isValidVersion(QString &bin) {
    QProcess p;
    p.start(bin, QStringList("--version"));
    p.waitForFinished();
    QString output = p.readAllStandardError() + p.readAllStandardOutput();

    QRegularExpression re("v(\\d+).*?(\\s|\\n)");
    QRegularExpressionMatch match = re.match(output);

    if (match.hasMatch()) {
        log("Output of `openconnect --version`: " + output);

        QString fullVersion = match.captured(0);
        QString majorVersion = match.captured(1);

        if (majorVersion.toInt() < 8) {
            emit error("The OpenConnect version must greater than v8.0.0, got " + fullVersion);
            return false;
        }
    } else {
        log("Failed to parse the OpenConnect version from " + output);
    }

    return true;
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
    if (output.indexOf("Connected as") >= 0 ||
        output.indexOf("Configured as") >= 0 ||
        output.indexOf("Configurado como") >= 0) {
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
