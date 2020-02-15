#ifndef CDPCOMMAND_H
#define CDPCOMMAND_H

#include <QObject>

class CDPCommand : public QObject
{
    Q_OBJECT
public:
    explicit CDPCommand(QObject *parent = nullptr);
    CDPCommand(int id, QString cmd, QVariantMap& params);

    QByteArray toJson();

signals:
    void finished();

private:
    int id;
    QString cmd;
    QVariantMap *params;
};

#endif // CDPCOMMAND_H
