//#include "gpservice.h"
#include "singleapplication.h"
#include "sigwatch.h"
#include "iostream"

//#include <QtDBus>
#include <QProcessEnvironment>


int main(int argc, char *argv[])
{
    SingleApplication app(argc, argv);

//    if (!QDBusConnection::systemBus().isConnected()) {
//        qWarning("Cannot connect to the D-Bus session bus.\n"
//                 "Please check your system settings and try again.\n");
//        return 1;
//    }

//    GPService service;

    QString env = "ENV: "  + QProcessEnvironment::systemEnvironment().toStringList().join("\n");
    std::cout << env.toStdString();

    UnixSignalWatcher sigwatch;
    sigwatch.watchForSignal(SIGINT);
    sigwatch.watchForSignal(SIGTERM);
    sigwatch.watchForSignal(SIGQUIT);
    sigwatch.watchForSignal(SIGHUP);
//    QObject::connect(&sigwatch, &UnixSignalWatcher::unixSignal, &service, &GPService::quit);

    return app.exec();
}
