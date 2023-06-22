#ifndef ENHANCEDWEBPAGE_H
#define ENHANCEDWEBPAGE_H

#include <QtWebEngineWidgets/qwebenginepage.h>

class EnhancedWebPage : public QWebEnginePage
{
protected:
    bool certificateError(const QWebEngineCertificateError &certificateError) override;
};

#endif // !ECHANCEDWEBPAG
