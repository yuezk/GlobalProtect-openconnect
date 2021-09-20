#include <QtCore/QObject>
#include <QtCore/QString>
#include <QtCore/QDir>
#include <QtCore/QStandardPaths>
#include <plog/Log.h>
#include <plog/Appenders/ColorConsoleAppender.h>

#include "singleapplication.h"
#include "gpclient.h"
#include "enhancedwebview.h"
#include "sigwatch.h"
#include "version.h"

int main(int argc, char *argv[])
{
    const QDir path = QStandardPaths::writableLocation(QStandardPaths::GenericCacheLocation) + "/GlobalProtect-openconnect";
    const QString logFile = path.path() + "/gpclient.log";
    if (!path.exists()) {
        path.mkpath(".");
    }

    static plog::ColorConsoleAppender<plog::TxtFormatter> consoleAppender;
    plog::init(plog::debug, logFile.toUtf8()).addAppender(&consoleAppender);

    PLOGI << "GlobalProtect started, version: " << VERSION;
    PLOGI << "PATH: " << qgetenv("PATH");

    QString port = QString::fromLocal8Bit(qgetenv(ENV_CDP_PORT));

    if (port == "") {
        qputenv(ENV_CDP_PORT, "12315");
    }

    PLOGI << "ENV: " << QProcessEnvironment::systemEnvironment().toStringList().join("\n");

    SingleApplication app(argc, argv);
    app.setQuitOnLastWindowClosed(false);

    GPClient w;
    w.show();

    QObject::connect(&app, &SingleApplication::instanceStarted, &w, &GPClient::activate);

    UnixSignalWatcher sigwatch;
    sigwatch.watchForSignal(SIGINT);
    sigwatch.watchForSignal(SIGTERM);
    sigwatch.watchForSignal(SIGQUIT);
    sigwatch.watchForSignal(SIGHUP);
    QObject::connect(&sigwatch, &UnixSignalWatcher::unixSignal, &w, &GPClient::quit);

    return app.exec();
}
