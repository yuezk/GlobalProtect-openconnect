#include <QtDBus>
#include "gpservice.h"
#include "singleapplication.h"

int main(int argc, char *argv[])
{
    SingleApplication app(argc, argv);

    if (!QDBusConnection::systemBus().isConnected()) {
        qWarning("Cannot connect to the D-Bus session bus.\n"
                 "Please check your system settings and try again.\n");
        return 1;
    }

    new GPService;

    return app.exec();
}
