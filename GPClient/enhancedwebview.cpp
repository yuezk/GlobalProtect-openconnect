#include <QtCore/QProcessEnvironment>
#include <QtWebEngineWidgets/QWebEngineView>

#include "enhancedwebview.h"
#include "cdpcommandmanager.h"

EnhancedWebView::EnhancedWebView(QWidget *parent)
    : QWebEngineView(parent)
    , cdp(new CDPCommandManager)
{
    QObject::connect(cdp, &CDPCommandManager::ready, this, &EnhancedWebView::onCDPReady);
    QObject::connect(cdp, &CDPCommandManager::eventReceived, this, &EnhancedWebView::onEventReceived);
}

EnhancedWebView::~EnhancedWebView()
{
    delete cdp;
}

void EnhancedWebView::initialize()
{
    QString port = QProcessEnvironment::systemEnvironment().value(ENV_CDP_PORT);
    cdp->initialize("http://127.0.0.1:" + port + "/json");
}

void EnhancedWebView::onCDPReady()
{
    cdp->sendCommand("Network.enable");
}

void EnhancedWebView::onEventReceived(QString eventName, QJsonObject params)
{
    if (eventName == "Network.responseReceived") {
        emit responseReceived(params);
    }
}
