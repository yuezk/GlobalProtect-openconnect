#include "portalconfigresponse.h"

#include <QXmlStreamReader>
#include <plog/Log.h>

QString PortalConfigResponse::xmlUserAuthCookie = "portal-userauthcookie";
QString PortalConfigResponse::xmlPrelogonUserAuthCookie = "portal-prelogonuserauthcookie";
QString PortalConfigResponse::xmlGateways = "gateways";

PortalConfigResponse::PortalConfigResponse()
    : _gateways(new QList<GPGateway>)
{
}

PortalConfigResponse::~PortalConfigResponse()
{
    delete _gateways;
}

PortalConfigResponse PortalConfigResponse::parse(const QByteArray& xml)
{
    QXmlStreamReader xmlReader(xml);
    PortalConfigResponse response;
    response.setRawResponse(xml);

    while (!xmlReader.atEnd()) {
        xmlReader.readNextStartElement();

        QString name = xmlReader.name().toString();

        if (name == xmlUserAuthCookie) {
            response.setUserAuthCookie(xmlReader.readElementText());
        } else if (name == xmlPrelogonUserAuthCookie) {
            response.setPrelogonUserAuthCookie(xmlReader.readElementText());
        } else if (name == xmlGateways) {
            parseGateways(xmlReader, response.allGateways());
        }
    }

    return response;
}

const QByteArray& PortalConfigResponse::rawResponse() const
{
    return _rawResponse;
}

QString PortalConfigResponse::username() const
{
    return _username;
}

QString PortalConfigResponse::password() const
{
    return _password;
}

void PortalConfigResponse::parseGateways(QXmlStreamReader &xmlReader, QList<GPGateway> *gateways)
{
    while (xmlReader.name() != xmlGateways || !xmlReader.isEndElement()) {
        xmlReader.readNext();
        // Parse the gateways -> external -> list -> entry
        if (xmlReader.name() == "entry" && xmlReader.isStartElement()) {
            GPGateway gateway;
            QString address = xmlReader.attributes().value("name").toString();
            gateway.setAddress(address);
            gateway.setPriorityRules(parsePriorityRules(xmlReader));
            gateway.setName(parseGatewayName(xmlReader));
            gateways->append(gateway);
        }
    }
}

QMap<QString, int> PortalConfigResponse::parsePriorityRules(QXmlStreamReader &xmlReader)
{
    QMap<QString, int> priorityRules;

    while (xmlReader.name() != "priority-rule" || !xmlReader.isEndElement()) {
        xmlReader.readNext();

        if (xmlReader.name() == "entry" && xmlReader.isStartElement()) {
            QString ruleName = xmlReader.attributes().value("name").toString();
            // Read the priority tag
            xmlReader.readNextStartElement();
            int ruleValue = xmlReader.readElementText().toUInt();
            priorityRules.insert(ruleName, ruleValue);
        }
    }
    return priorityRules;
}

QString PortalConfigResponse::parseGatewayName(QXmlStreamReader &xmlReader)
{
   while (xmlReader.name() != "description" || !xmlReader.isEndElement()) {
       xmlReader.readNext();
       if (xmlReader.name() == "description" && xmlReader.tokenType() == xmlReader.StartElement) {
           return xmlReader.readElementText();
       }
   }

   PLOGE << "Error: <description> tag not found";
   return "";
}

QString PortalConfigResponse::userAuthCookie() const
{
    return _userAuthCookie;
}

QString PortalConfigResponse::prelogonUserAuthCookie() const
{
    return _prelogonAuthCookie;
}

QList<GPGateway>* PortalConfigResponse::allGateways()
{
    return _gateways;
}

void PortalConfigResponse::setRawResponse(const QByteArray &response)
{
    _rawResponse = response;
}

void PortalConfigResponse::setUsername(const QString& username)
{
    _username = username;
}

void PortalConfigResponse::setPassword(const QString& password)
{
    _password = password;
}

void PortalConfigResponse::setUserAuthCookie(const QString &cookie)
{
    _userAuthCookie = cookie;
}

void PortalConfigResponse::setPrelogonUserAuthCookie(const QString &cookie)
{
    _prelogonAuthCookie = cookie;
}
