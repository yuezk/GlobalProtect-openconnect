#ifndef SAMLLOGINWINDOW_H
#define SAMLLOGINWINDOW_H

#include "enhancedwebview.h"

#include <QDialog>
#include <QMap>
#include <QCloseEvent>

class SAMLLoginWindow : public QDialog
{
    Q_OBJECT

public:
    explicit SAMLLoginWindow(QWidget *parent = nullptr);
    ~SAMLLoginWindow();

    void login(const QString samlMethod, const QString samlRequest, const QString preloingUrl);

signals:
    void success(QMap<QString, QString> samlResult);
    void fail(const QString msg);

private slots:
    void onResponseReceived(QJsonObject params);
    void onLoadFinished();

private:
    EnhancedWebView *webView;
    QMap<QString, QString> samlResult;

    void closeEvent(QCloseEvent *event);
};

#endif // SAMLLOGINWINDOW_H
