#ifndef STANDARDLOGINWINDOW_H
#define STANDARDLOGINWINDOW_H

#include <QtWidgets/QDialog>

namespace Ui {
    class StandardLoginWindow;
}

class StandardLoginWindow : public QDialog {
Q_OBJECT

public:
    explicit StandardLoginWindow(const QString &portalAddress, const QString &labelUsername,
                                 const QString &labelPassword, const QString &authMessage);

    void setProcessing(bool isProcessing);

private slots:

    void on_loginButton_clicked();

signals:

    void performLogin(QString username, QString password);

private:
    Ui::StandardLoginWindow *ui;

    void closeEvent(QCloseEvent *event);
};

#endif // STANDARDLOGINWINDOW_H
