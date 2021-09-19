#ifndef SETTINGSDIALOG_H
#define SETTINGSDIALOG_H

#include <QtWidgets/QDialog>

namespace Ui {
class SettingsDialog;
}

class SettingsDialog : public QDialog
{
    Q_OBJECT

public:
    explicit SettingsDialog(QWidget *parent = nullptr);
    ~SettingsDialog();

    void setExtraArgs(QString extraArgs);
    QString extraArgs();

    void setClientos(QString clientos);
    QString clientos();

private:
    Ui::SettingsDialog *ui;
};

#endif // SETTINGSDIALOG_H
