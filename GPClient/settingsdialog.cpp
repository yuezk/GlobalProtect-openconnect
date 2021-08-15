#include "settingsdialog.h"
#include "ui_settingsdialog.h"

SettingsDialog::SettingsDialog(QWidget *parent) :
    QDialog(parent),
    ui(new Ui::SettingsDialog)
{
    ui->setupUi(this);

    ui->extraArgsInput->setPlaceholderText("e.g. --name=value");
}

SettingsDialog::~SettingsDialog()
{
    delete ui;
}

void SettingsDialog::setExtraArgs(QString args)
{
    ui->extraArgsInput->setPlainText(args);
}

QString SettingsDialog::extraArgs()
{
    return ui->extraArgsInput->toPlainText().trimmed();
}
