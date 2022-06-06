#ifndef SAMLLOGINWINDOW_H
#define SAMLLOGINWINDOW_H

#include <QtCore/QMap>
#include <QtGui/QCloseEvent>
#include <QtWidgets/QDialog>

#include "enhancedwebview.h"

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
    void getHTML(QString sHTML);

protected slots:
    void handleHTML(QString sHTML);

private slots:
    void onResponseReceived(QJsonObject params);
    void onLoadFinished();
    void checkSamlResult(QString username, QString preloginCookie, QString userAuthCookie);

private:
    EnhancedWebView *webView;
    QMap<QString, QString> samlResult;

    void closeEvent(QCloseEvent *event);
};

#endif // SAMLLOGINWINDOW_H
