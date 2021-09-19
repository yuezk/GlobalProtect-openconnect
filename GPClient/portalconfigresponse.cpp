#include <QtCore/QXmlStreamReader>
#include <plog/Log.h>

#include "portalconfigresponse.h"

QString PortalConfigResponse::xmlUserAuthCookie = "portal-userauthcookie";
QString PortalConfigResponse::xmlPrelogonUserAuthCookie = "portal-prelogonuserauthcookie";
QString PortalConfigResponse::xmlGateways = "gateways";

PortalConfigResponse::PortalConfigResponse()
{
}

PortalConfigResponse::~PortalConfigResponse()
{
}

PortalConfigResponse PortalConfigResponse::parse(const QByteArray xml)
{
    PLOGI << "Start parsing the portal configuration...";

    QXmlStreamReader xmlReader(xml);
    PortalConfigResponse response;
    response.setRawResponse(xml);

    while (!xmlReader.atEnd()) {
        xmlReader.readNextStartElement();

        QString name = xmlReader.name().toString();

        if (name == xmlUserAuthCookie) {
            PLOGI << "Start reading " << name;
            response.setUserAuthCookie(xmlReader.readElementText());
        } else if (name == xmlPrelogonUserAuthCookie) {
            PLOGI << "Start reading " << name;
            response.setPrelogonUserAuthCookie(xmlReader.readElementText());
        } else if (name == xmlGateways) {
            response.setAllGateways(parseGateways(xmlReader));
        }
    }

    PLOGI << "Finished parsing portal configuration.";

    return response;
}

const QByteArray PortalConfigResponse::rawResponse() const
{
    return m_rawResponse;
}

const QString &PortalConfigResponse::username() const
{
    return m_username;
}

QString PortalConfigResponse::password() const
{
    return m_password;
}

QList<GPGateway> PortalConfigResponse::parseGateways(QXmlStreamReader &xmlReader)
{
    PLOGI << "Start parsing the gateways from portal configuration...";

    QList<GPGateway> gateways;

    while (xmlReader.name() != "external"){
        xmlReader.readNext();
    }

    while (xmlReader.name() != "list"){
        xmlReader.readNext();
    }

    while (xmlReader.name() != xmlGateways || !xmlReader.isEndElement()) {
        xmlReader.readNext();
        // Parse the gateways -> external -> list -> entry
        if (xmlReader.name() == "entry" && xmlReader.isStartElement()) {
            GPGateway g;
            QString address = xmlReader.attributes().value("name").toString();
            g.setAddress(address);
            g.setPriorityRules(parsePriorityRules(xmlReader));
            g.setName(parseGatewayName(xmlReader));
            gateways.append(g);
        }
    }

    PLOGI << "Finished parsing the gateways.";

    return gateways;
}

QMap<QString, int> PortalConfigResponse::parsePriorityRules(QXmlStreamReader &xmlReader)
{
    PLOGI << "Start parsing the priority rules...";

    QMap<QString, int> priorityRules;

    while ((xmlReader.name() != "priority-rule" || !xmlReader.isEndElement()) && !xmlReader.hasError()) {
        xmlReader.readNext();

        if (xmlReader.name() == "entry" && xmlReader.isStartElement()) {
            QString ruleName = xmlReader.attributes().value("name").toString();
            // Read the priority tag
            while (xmlReader.name() != "priority"){
                xmlReader.readNext();
            }
            int ruleValue = xmlReader.readElementText().toUInt();
            priorityRules.insert(ruleName, ruleValue);
        }
    }

    PLOGI << "Finished parsing the priority rules.";

    return priorityRules;
}

QString PortalConfigResponse::parseGatewayName(QXmlStreamReader &xmlReader)
{
    PLOGI << "Start parsing the gateway name...";

    while (xmlReader.name() != "description" || !xmlReader.isEndElement()) {
        xmlReader.readNext();
        if (xmlReader.name() == "description" && xmlReader.tokenType() == xmlReader.StartElement) {
            PLOGI << "Finished parsing the gateway name";
            return xmlReader.readElementText();
        }
    }

    PLOGE << "Error: <description> tag not found";
    return "";
}

QString PortalConfigResponse::userAuthCookie() const
{
    return m_userAuthCookie;
}

QString PortalConfigResponse::prelogonUserAuthCookie() const
{
    return m_prelogonAuthCookie;
}

QList<GPGateway> PortalConfigResponse::allGateways() const
{
    return m_gateways;
}

void PortalConfigResponse::setAllGateways(QList<GPGateway> gateways)
{
    m_gateways = gateways;
}

void PortalConfigResponse::setRawResponse(const QByteArray response)
{
    m_rawResponse = response;
}

void PortalConfigResponse::setUsername(const QString username)
{
    m_username = username;
}

void PortalConfigResponse::setPassword(const QString password)
{
    m_password = password;
}

void PortalConfigResponse::setUserAuthCookie(const QString cookie)
{
    m_userAuthCookie = cookie;
}

void PortalConfigResponse::setPrelogonUserAuthCookie(const QString cookie)
{
    m_prelogonAuthCookie = cookie;
}
