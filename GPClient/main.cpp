#include "singleapplication.h"
#include "gpclient.h"

int main(int argc, char *argv[])
{
    QString port = QString::fromLocal8Bit(qgetenv(ENV_CDP_PORT));
    if (port == "") {
        qputenv(ENV_CDP_PORT, "12315");
    }
    SingleApplication app(argc, argv);
    GPClient w;
    w.show();

    QObject::connect(&app, &SingleApplication::instanceStarted, &w, &GPClient::raise);

    return app.exec();
}
