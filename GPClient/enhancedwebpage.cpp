#include "enhancedwebpage.h"
#include <QWebEngineCertificateError>
#include <plog/Log.h>

bool EnhancedWebPage::certificateError(const QWebEngineCertificateError &certificateError) {
    LOGI << "An error occurred during certificate verification for " << certificateError.url().toString() << "; " << certificateError.errorDescription();
    return certificateError.isOverridable();
};
