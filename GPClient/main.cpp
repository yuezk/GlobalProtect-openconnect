#include "gpclient.h"

#include <QApplication>

int main(int argc, char *argv[])
{
    QApplication a(argc, argv);
    GPClient w;
    w.show();
    return a.exec();
}
