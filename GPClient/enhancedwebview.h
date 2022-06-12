#ifndef ENHANCEDWEBVIEW_H
#define ENHANCEDWEBVIEW_H

#include <QtWebEngineWidgets/QWebEngineView>

#include "cdpcommandmanager.h"

#define ENV_CDP_PORT "QTWEBENGINE_REMOTE_DEBUGGING"

class EnhancedWebView : public QWebEngineView
{
    Q_OBJECT
public:
    explicit EnhancedWebView(QWidget *parent = nullptr);

    void initialize();

signals:
    void responseReceived(QJsonObject params);

private slots:
    void onCDPReady();
    void onEventReceived(QString eventName, QJsonObject params);

private:
    CDPCommandManager *cdp { nullptr };
};

#endif // ENHANCEDWEBVIEW_H
