#include "singleapplication.h"
#include "gpclient.h"
#include "enhancedwebview.h"

#include <QStandardPaths>
#include <QProcessEnvironment>
#include <plog/Log.h>
#include <plog/Appenders/ColorConsoleAppender.h>

static const QString version = "v1.3.3";

int main(int argc, char *argv[])
{
    const QDir path = QStandardPaths::writableLocation(QStandardPaths::GenericCacheLocation) + "/GlobalProtect-openconnect";
    const QString logFile = path.path() + "/gpclient.log";
    if (!path.exists()) {
        path.mkpath(".");
    }

    static plog::ColorConsoleAppender<plog::TxtFormatter> consoleAppender;
    plog::init(plog::debug, logFile.toUtf8()).addAppender(&consoleAppender);

    PLOGI << "GlobalProtect started, version: " << version;
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

    return app.exec();
}
