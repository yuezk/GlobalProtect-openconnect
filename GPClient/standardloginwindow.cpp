#include <QtGui/QCloseEvent>

#include "standardloginwindow.h"
#include "ui_standardloginwindow.h"

StandardLoginWindow::StandardLoginWindow(const QString &portalAddress, const QString &labelUsername,
                                         const QString &labelPassword, const QString &authMessage) :
        QDialog(nullptr),
        ui(new Ui::StandardLoginWindow) {
    ui->setupUi(this);
    ui->portalAddress->setText(portalAddress);
    ui->username->setPlaceholderText(labelUsername);
    ui->password->setPlaceholderText(labelPassword);
    ui->authMessage->setText(authMessage);

    setWindowTitle("GlobalProtect Login");
    setFixedSize(width(), height());
    setModal(true);
}

void StandardLoginWindow::setProcessing(bool isProcessing) {
    ui->username->setReadOnly(isProcessing);
    ui->password->setReadOnly(isProcessing);
    ui->loginButton->setDisabled(isProcessing);
}

void StandardLoginWindow::on_loginButton_clicked() {
    const QString username = ui->username->text().trimmed();
    const QString password = ui->password->text().trimmed();

    if (username.isEmpty() || password.isEmpty()) {
        return;
    }

    emit performLogin(username, password);
}

void StandardLoginWindow::closeEvent(QCloseEvent *event) {
    event->accept();
    reject();
}
