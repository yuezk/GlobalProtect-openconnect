#ifndef ENHANCEDWEBVIEW_H
#define ENHANCEDWEBVIEW_H

#include "cdpcommandmanager.h"
#include <QtWebEngineWidgets/QWebEngineView>

#define ENV_CDP_PORT "QTWEBENGINE_REMOTE_DEBUGGING"

class EnhancedWebView : public QWebEngineView
{
    Q_OBJECT
public:
    explicit EnhancedWebView(QWidget *parent = nullptr);
    ~EnhancedWebView();

    void initialize();

signals:
    void responseReceived(QJsonObject params);

private slots:
    void onCDPReady();
    void onEventReceived(QString eventName, QJsonObject params);

private:
    CDPCommandManager *cdp;
};

#endif // ENHANCEDWEBVIEW_H
