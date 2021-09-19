#ifndef PORTALAUTHWINDOW_H
#define PORTALAUTHWINDOW_H

#include <QtWidgets/QDialog>

namespace Ui {
class NormalLoginWindow;
}

class NormalLoginWindow : public QDialog
{
    Q_OBJECT

public:
    explicit NormalLoginWindow(QWidget *parent = nullptr);
    ~NormalLoginWindow();

    void setAuthMessage(QString);
    void setUsernameLabel(QString);
    void setPasswordLabel(QString);
    void setPortalAddress(QString);

    void setProcessing(bool isProcessing);

private slots:
    void on_loginButton_clicked();

signals:
    void performLogin(QString username, QString password);

private:
    Ui::NormalLoginWindow *ui;

    void closeEvent(QCloseEvent *event);
};

#endif // PORTALAUTHWINDOW_H
