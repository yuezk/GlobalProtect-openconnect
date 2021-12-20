#ifndef CHALLENGEDIALOG_H
#define CHALLENGEDIALOG_H

#include <QDialog>

namespace Ui {
class ChallengeDialog;
}

class ChallengeDialog : public QDialog
{
    Q_OBJECT

public:
    explicit ChallengeDialog(QWidget *parent = nullptr);
    ~ChallengeDialog();

    void setMessage(const QString &message);
    const QString getChallenge();

private slots:
    void on_challengeInput_textChanged(const QString &arg1);

private:
    Ui::ChallengeDialog *ui;
};

#endif // CHALLENGEDIALOG_H
