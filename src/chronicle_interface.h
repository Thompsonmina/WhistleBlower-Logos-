#ifndef CHRONICLE_INTERFACE_H
#define CHRONICLE_INTERFACE_H

#include <QByteArray>
#include <QMetaType>
#include <QObject>
#include <QString>
#include <QtGlobal>

#include "interface.h"
#include "logos_types.h"

class ChronicleInterface : public PluginInterface
{
public:
    virtual ~ChronicleInterface() = default;

    Q_INVOKABLE virtual QString health() = 0;

    Q_INVOKABLE virtual QString uploadFileJson(const QString& path,
                                               const QString& contentType,
                                               const QString& title) = 0;

    Q_INVOKABLE virtual QString uploadStatusJson(const QString& uploadId) = 0;

    Q_INVOKABLE virtual QString normalizeContentTypeJson(
        const QString& contentType) = 0;

    Q_INVOKABLE virtual QString hashMetadataJson(const QString& contentType,
                                                 const QString& sizeBytes,
                                                 const QString& title,
                                                 const QString& description,
                                                 const QString& tagsJson) = 0;

    Q_INVOKABLE virtual QString buildMetadataEnvelopeJson(
        const QString& envelopeInputJson) = 0;

    Q_INVOKABLE virtual QString startBroadcasterJson() = 0;

    Q_INVOKABLE virtual QString broadcastEnvelopeJson(
        const QString& envelopeJson) = 0;

    Q_INVOKABLE virtual QString broadcastStatusJson(
        const QString& broadcastId) = 0;

    Q_INVOKABLE virtual QString publishFileJson(
        const QString& requestJson) = 0;

    Q_INVOKABLE virtual QString publishStatusJson(
        const QString& publishId) = 0;

    Q_INVOKABLE virtual QString listPublishedJson() = 0;
};

#define ChronicleInterface_iid "org.logos.ChronicleInterface"
Q_DECLARE_INTERFACE(ChronicleInterface, ChronicleInterface_iid)
Q_DECLARE_METATYPE(LogosResult)

#endif // CHRONICLE_INTERFACE_H
