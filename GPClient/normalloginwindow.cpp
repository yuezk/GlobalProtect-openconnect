#include <QtGui/QCloseEvent>

#include "normalloginwindow.h"
#include "ui_normalloginwindow.h"
#include "gpheader.h"

using namespace gpclient::helper;

NormalLoginWindow::NormalLoginWindow(QWidget *parent) :
    QDialog(parent),
    ui(new Ui::NormalLoginWindow)
{
    ui->setupUi(this);
    setWindowTitle("GlobalProtect Login");
    setFixedSize(width(), height());
    setModal(true);

    ui->username->setText(settings::get("username", "").toString());
    ui->password->setText(settings::get("password", "").toString());
}

NormalLoginWindow::~NormalLoginWindow()
{
    delete ui;
}

void NormalLoginWindow::setAuthMessage(QString message)
{
    ui->authMessage->setText(message);
}

void NormalLoginWindow::setUsernameLabel(QString label)
{
    ui->username->setPlaceholderText(label);
}

void NormalLoginWindow::setPasswordLabel(QString label)
{
    ui->password->setPlaceholderText(label);
}

void NormalLoginWindow::setPortalAddress(QString portal)
{
    ui->portalAddress->setText(portal);
}

void NormalLoginWindow::setProcessing(bool isProcessing)
{
    ui->username->setReadOnly(isProcessing);
    ui->password->setReadOnly(isProcessing);
    ui->loginButton->setDisabled(isProcessing);
}

void NormalLoginWindow::on_loginButton_clicked()
{
    const QString username = ui->username->text().trimmed();
    const QString password = ui->password->text().trimmed();

    //save username and password (not secure tho)
    settings::save("username", username);
    settings::save("password", password);

    if (username.isEmpty() || password.isEmpty()) {
        return;
    }

    emit performLogin(username, password);
}

void NormalLoginWindow::closeEvent(QCloseEvent *event)
{
    event->accept();
    reject();
}
