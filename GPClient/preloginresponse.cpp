#include <QtCore/QXmlStreamReader>
#include <QtCore/QMap>
#include <plog/Log.h>

#include "preloginresponse.h"

QString PreloginResponse::xmlAuthMessage = "authentication-message";
QString PreloginResponse::xmlLabelUsername = "username-label";
QString PreloginResponse::xmlLabelPassword = "password-label";
QString PreloginResponse::xmlSamlMethod = "saml-auth-method";
QString PreloginResponse::xmlSamlRequest = "saml-request";
QString PreloginResponse::xmlRegion = "region";

PreloginResponse::PreloginResponse()
{
    add(xmlAuthMessage, "");
    add(xmlLabelUsername, "");
    add(xmlLabelPassword, "");
    add(xmlSamlMethod, "");
    add(xmlSamlRequest, "");
    add(xmlRegion, "");
}

PreloginResponse PreloginResponse::parse(const QByteArray& xml)
{
    PLOGI << "Start parsing the prelogin response...";

    QXmlStreamReader xmlReader(xml);
    PreloginResponse response;
    response.setRawResponse(xml);

    while (!xmlReader.atEnd()) {
        xmlReader.readNextStartElement();
        QString name = xmlReader.name().toString();
        if (response.has(name)) {
            response.add(name, xmlReader.readElementText());
        }
    }
    return response;
}

const QByteArray& PreloginResponse::rawResponse() const
{
    return _rawResponse;
}

QString PreloginResponse::authMessage() const
{
    return resultMap.value(xmlAuthMessage);
}

QString PreloginResponse::labelUsername() const
{
    return resultMap.value(xmlLabelUsername);
}

QString PreloginResponse::labelPassword() const
{
    return resultMap.value(xmlLabelPassword);
}

QString PreloginResponse::samlMethod() const
{
    return resultMap.value(xmlSamlMethod);
}

QString PreloginResponse::samlRequest() const
{
    return QByteArray::fromBase64(resultMap.value(xmlSamlRequest).toUtf8());
}

QString PreloginResponse::region() const
{
    return resultMap.value(xmlRegion);
}

bool PreloginResponse::hasSamlAuthFields() const
{
    return !samlMethod().isEmpty() && !samlRequest().isEmpty();
}

bool PreloginResponse::hasNormalAuthFields() const
{
    return !labelUsername().isEmpty() && !labelPassword().isEmpty();
}

void PreloginResponse::setRawResponse(const QByteArray response)
{
    _rawResponse = response;
}

bool PreloginResponse::has(const QString name) const
{
    return resultMap.contains(name);
}

void PreloginResponse::add(const QString name, const QString value)
{
    resultMap.insert(name, value);
}
