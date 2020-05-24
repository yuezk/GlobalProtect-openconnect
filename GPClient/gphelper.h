#ifndef GPHELPER_H
#define GPHELPER_H

#include "samlloginwindow.h"
#include "gpgateway.h"

#include <QObject>
#include <QNetworkAccessManager>
#include <QNetworkRequest>
#include <QNetworkReply>
#include <QUrlQuery>
#include <QSettings>


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

            QVariant get(const QString &key, const QVariant &defaultValue = QVariant());
            void save(const QString &key, const QVariant &value);
            void clear();
        }
    }
}

#endif // GPHELPER_H
