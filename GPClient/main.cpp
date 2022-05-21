#include <QtCore/QObject>
#include <QtCore/QString>
#include <QtCore/QDir>
#include <QtCore/QStandardPaths>
#include <plog/Log.h>
#include <plog/Init.h>
#include <plog/Appenders/ColorConsoleAppender.h>
#include <plog/Formatters/TxtFormatter.h>

#include "singleapplication.h"
#include "gpclient.h"
#include "vpn_dbus.h"
#include "vpn_json.h"
#include "enhancedwebview.h"
#include "sigwatch.h"
#include "version.h"

#define QT_AUTO_SCREEN_SCALE_FACTOR "QT_AUTO_SCREEN_SCALE_FACTOR"

int main(int argc, char *argv[])
{
    plog::ColorConsoleAppender<plog::TxtFormatter> consoleAppender(plog::streamStdErr);
    plog::init(plog::debug, &consoleAppender);

    PLOGI << "GlobalProtect started, version: " << VERSION;

    QString port = QString::fromLocal8Bit(qgetenv(ENV_CDP_PORT));
    QString hidpiSupport = QString::fromLocal8Bit(qgetenv(QT_AUTO_SCREEN_SCALE_FACTOR));

    if (port.isEmpty()) {
        qputenv(ENV_CDP_PORT, "12315");
    }

    if (hidpiSupport.isEmpty()) {
        qputenv(QT_AUTO_SCREEN_SCALE_FACTOR, "1");
    }

    SingleApplication app(argc, argv);
    app.setQuitOnLastWindowClosed(false);

    QCommandLineParser parser;
    parser.addHelpOption();
    parser.addVersionOption();
    parser.addPositionalArgument("server", "The URL of the VPN server. Optional.");
    parser.addPositionalArgument("gateway", "The URL of the specific VPN gateway. Optional.");
    parser.addOptions({
      {"json", "Write the result of the handshake with the GlobalConnect server to stdout as JSON and terminate. Useful for scripting."},
      {"now", "Do not show the dialog with the connect button; connect immediately instead."},
    });
    parser.process(app);

    const QStringList positional = parser.positionalArguments();

    IVpn *vpn = parser.isSet("json") // yes it leaks, but this is cleared on exit anyway
      ? static_cast<IVpn*>(new VpnJson(nullptr)) // Print to stdout and exit
      : static_cast<IVpn*>(new VpnDbus(nullptr)); // Contact GPService daemon via dbus
    GPClient w(nullptr, vpn);
    w.show();

    if (positional.size() > 0) {
      w.portal(positional.at(0));
    }
    if (positional.size() > 1) {
      GPGateway gw;
      gw.setName(positional.at(1));
      gw.setAddress(positional.at(1));
      w.setCurrentGateway(gw);
    }

    QObject::connect(&app, &SingleApplication::instanceStarted, &w, &GPClient::activate);

    UnixSignalWatcher sigwatch;
    sigwatch.watchForSignal(SIGINT);
    sigwatch.watchForSignal(SIGTERM);
    sigwatch.watchForSignal(SIGQUIT);
    sigwatch.watchForSignal(SIGHUP);
    QObject::connect(&sigwatch, &UnixSignalWatcher::unixSignal, &w, &GPClient::quit);

    if (parser.isSet("now")) {
      w.doConnect();
    }
    if (parser.isSet("json")) {
      QObject::connect(static_cast<VpnJson*>(vpn), &VpnJson::connected, &w, &GPClient::quit);
    }

    return app.exec();
}
