#ifndef GPHELPER_H
#define GPHELPER_H

#include <QtCore/QObject>
#include <QtCore/QUrlQuery>
#include <QtCore/QSettings>
#include <QtNetwork/QNetworkAccessManager>
#include <QtNetwork/QNetworkRequest>
#include <QtNetwork/QNetworkReply>

#include "samlloginwindow.h"
#include "gpgateway.h"


const QString UA = "PAN GlobalProtect";

namespace gpclient {
    namespace helper {
        extern QNetworkAccessManager *networkManager;

        QNetworkReply* createRequest(QString url, QByteArray params = nullptr);

        GPGateway filterPreferredGateway(QList<GPGateway> gateways, const QString ruleName);

        QUrlQuery parseGatewayResponse(const QByteArray& xml);

        void openMessageBox(const QString& message, const QString& informativeText = "");

        void moveCenter(QWidget *widget);

        namespace settings {

            extern QSettings *_settings;
            static const QStringList reservedKeys {"extraArgs", "clientos", "samlUserAgent"};

            QVariant get(const QString &key, const QVariant &defaultValue = QVariant());
            QStringList get_all(const QString &key, const QVariant &defaultValue = QVariant());
            void save(const QString &key, const QVariant &value);
            void clear();

            bool secureSave(const QString &key, const QString &value);
            bool secureGet(const QString &key, QString &value);
        }
    }
}

#endif // GPHELPER_H
