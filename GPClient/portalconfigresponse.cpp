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
    LOGI << "Start parsing the portal configuration...";

    QXmlStreamReader xmlReader(xml);
    PortalConfigResponse response;
    response.setRawResponse(xml);

    while (!xmlReader.atEnd()) {
        xmlReader.readNextStartElement();

        QString name = xmlReader.name().toString();

        if (name == xmlUserAuthCookie) {
            LOGI << "Start reading " << name;
            response.setUserAuthCookie(xmlReader.readElementText());
        } else if (name == xmlPrelogonUserAuthCookie) {
            LOGI << "Start reading " << name;
            response.setPrelogonUserAuthCookie(xmlReader.readElementText());
        } else if (name == xmlGateways) {
            response.setAllGateways(parseGateways(xmlReader));
        }
    }

    LOGI << "Finished parsing portal configuration.";

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
    LOGI << "Start parsing the gateways from portal configuration...";

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
            parseGateway(xmlReader, g);
            gateways.append(g);
        }
    }

    LOGI << "Finished parsing the gateways.";

    return gateways;
}

void PortalConfigResponse::parseGateway(QXmlStreamReader &reader, GPGateway &gateway) {
    LOGI << "Start parsing gateway...";

    auto finished = false;
    while (!finished) {
        if (reader.name() == "entry" && reader.isStartElement()) {
            auto address = reader.attributes().value("name").toString();
            gateway.setAddress(address);
        } else if (reader.name() == "description" && reader.isStartElement()) { // gateway name
            gateway.setName(reader.readElementText());
        } else if (reader.name() == "priority-rule" && reader.isStartElement()) { // priority rules
            parsePriorityRule(reader, gateway);
        }

        auto result = reader.readNext();
        finished = result == QXmlStreamReader::Invalid || (reader.name() == "entry" && reader.isEndElement());
    }
}

void PortalConfigResponse::parsePriorityRule(QXmlStreamReader &reader, GPGateway &gateway) {
    LOGI << "Start parsing priority rule...";

    QMap<QString, int> priorityRules;
    auto finished = false;

    while (!finished) {
        // Parse the priority-rule -> entry
        if (reader.name() == "entry" && reader.isStartElement()) {
            auto ruleName = reader.attributes().value("name").toString();
            // move to the priority value
            while (reader.readNextStartElement()) {
                if (reader.name() == "priority") {
                    auto priority = reader.readElementText().toInt();
                    priorityRules.insert(ruleName, priority);
                    break;
                }
            }
        }
        auto result = reader.readNext();
        finished = result == QXmlStreamReader::Invalid || (reader.name() == "priority-rule" && reader.isEndElement());
    }

    gateway.setPriorityRules(priorityRules);
}

QString PortalConfigResponse::userAuthCookie() const
{
    return m_userAuthCookie;
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
