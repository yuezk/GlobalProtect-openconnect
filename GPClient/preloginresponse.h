#ifndef PRELOGINRESPONSE_H
#define PRELOGINRESPONSE_H

#include <QtCore/QString>
#include <QtCore/QMap>

class PreloginResponse
{
public:
    PreloginResponse();

    static PreloginResponse parse(const QByteArray& xml);

    const QByteArray& rawResponse() const;
    QString authMessage() const;
    QString labelUsername() const;
    QString labelPassword() const;
    QString samlMethod() const;
    QString samlRequest() const;
    QString region() const;

    bool hasSamlAuthFields() const;
    bool hasNormalAuthFields() const;

private:
    static QString xmlAuthMessage;
    static QString xmlLabelUsername;
    static QString xmlLabelPassword;
    static QString xmlSamlMethod;
    static QString xmlSamlRequest;
    static QString xmlRegion;

    QMap<QString, QString> resultMap;
    QByteArray _rawResponse;

    void setRawResponse(const QByteArray response);
    void add(const QString name, const QString value);
    bool has(const QString name) const;
};

#endif // PRELOGINRESPONSE_H
