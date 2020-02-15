#ifndef SAMLLOGINWINDOW_H
#define SAMLLOGINWINDOW_H

#include "enhancedwebview.h"

#include <QDialog>
#include <QJsonObject>
#include <QCloseEvent>

class SAMLLoginWindow : public QDialog
{
    Q_OBJECT

public:
    explicit SAMLLoginWindow(QWidget *parent = nullptr);
    ~SAMLLoginWindow();

    void login(QString url);

signals:
    void success(QJsonObject samlResult);

private slots:
    void onResponseReceived(QJsonObject params);

private:
    EnhancedWebView *webView;
    QJsonObject samlResult;

    void closeEvent(QCloseEvent *event);
};

#endif // SAMLLOGINWINDOW_H
