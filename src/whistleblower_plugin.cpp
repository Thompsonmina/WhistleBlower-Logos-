#include "whistleblower_plugin.h"

#include <QDebug>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QStringList>
#include <QTimer>

#include "logos_api.h"
#include "logos_api_client.h"
#include "token_manager.h"

namespace {
constexpr int POLL_INTERVAL_MS = 1000;

QString compactJson(const QJsonObject& obj) {
    return QString::fromUtf8(QJsonDocument(obj).toJson(QJsonDocument::Compact));
}

QJsonObject parseObject(const QString& json) {
    return QJsonDocument::fromJson(json.toUtf8()).object();
}
}  // namespace

WhistleblowerPlugin::WhistleblowerPlugin(QObject* parent)
    : WhistleblowerSimpleSource(parent)
{
    setStatus(QStringLiteral("idle"));
    setBusy(false);
    setDeliveryReady(false);
}

WhistleblowerPlugin::~WhistleblowerPlugin() {
    if (m_pollTimer != nullptr) {
        m_pollTimer->stop();
    }
    delete m_chronicleClient;
}

void WhistleblowerPlugin::initLogos(LogosAPI* api) {
    m_logosAPI = api;
    setBackend(this);

    m_pollTimer = new QTimer(this);
    m_pollTimer->setInterval(POLL_INTERVAL_MS);
    connect(m_pollTimer, &QTimer::timeout,
            this, &WhistleblowerPlugin::pollPublishStatus);

    ensureChronicleClient();

    // Pre-warm Chronicle's delivery node synchronously from the UI host main
    // thread. Required because Chronicle's lazy delivery init fails when first
    // triggered from an internal async timer callback.
    QTimer::singleShot(0, this, [this]() { startBroadcaster(); });

    qDebug() << "WhistleblowerPlugin: initialized";
}

void WhistleblowerPlugin::ensureChronicleClient() {
    if (m_chronicleClient != nullptr || m_logosAPI == nullptr) {
        return;
    }
    m_chronicleClient = new LogosAPIClient(
        QStringLiteral("chronicle"),
        QStringLiteral("whistleblower"),
        m_logosAPI->getTokenManager(),
        this);
}

QString WhistleblowerPlugin::callChronicle(const QString& method,
                                           const QVariantList& args) {
    ensureChronicleClient();
    if (m_chronicleClient == nullptr) {
        return compactJson({{QStringLiteral("ok"), false},
                            {QStringLiteral("code"), QStringLiteral("CLIENT_UNAVAILABLE")},
                            {QStringLiteral("error"), QStringLiteral("chronicle client not ready")}});
    }

    QVariant response;
    if (args.isEmpty()) {
        response = m_chronicleClient->invokeRemoteMethod(
            QStringLiteral("chronicle"), method);
    } else if (args.size() == 1) {
        response = m_chronicleClient->invokeRemoteMethod(
            QStringLiteral("chronicle"), method, args[0]);
    } else if (args.size() == 2) {
        response = m_chronicleClient->invokeRemoteMethod(
            QStringLiteral("chronicle"), method, args[0], args[1]);
    } else if (args.size() == 3) {
        response = m_chronicleClient->invokeRemoteMethod(
            QStringLiteral("chronicle"), method, args[0], args[1], args[2]);
    } else if (args.size() == 5) {
        response = m_chronicleClient->invokeRemoteMethod(
            QStringLiteral("chronicle"), method,
            args[0], args[1], args[2], args[3], args[4]);
    } else {
        qWarning() << "WhistleblowerPlugin: unsupported arg count for"
                   << method << args.size();
        return {};
    }
    return response.toString();
}

void WhistleblowerPlugin::startBroadcaster() {
    const QString resp = callChronicle(QStringLiteral("startBroadcasterJson"));
    const QJsonObject obj = parseObject(resp);
    const bool ok = obj.value(QStringLiteral("ok")).toBool();
    setDeliveryReady(ok);
    if (!ok) {
        const QString err = obj.value(QStringLiteral("error")).toString();
        qWarning() << "WhistleblowerPlugin: startBroadcaster failed:" << err << "raw:" << resp;
    } else {
        qDebug() << "WhistleblowerPlugin: chronicle broadcaster ready";
    }
}

void WhistleblowerPlugin::resetPublishState() {
    setCurrentPublishId(QString());
    setCid(QString());
    setMetadataHash(QString());
    setLastError(QString());
}

void WhistleblowerPlugin::publish(QString path,
                                  QString contentType,
                                  QString title,
                                  QString description,
                                  QString tagsCsv) {
    if (busy()) {
        setLastError(QStringLiteral("a publish is already in progress"));
        return;
    }

    QJsonArray tagsArr;
    for (const QString& raw : tagsCsv.split(',', Qt::SkipEmptyParts)) {
        const QString tag = raw.trimmed();
        if (!tag.isEmpty()) {
            tagsArr.append(tag);
        }
    }

    QJsonObject req;
    req.insert(QStringLiteral("path"), path);
    req.insert(QStringLiteral("content_type"),
               contentType.trimmed().isEmpty()
                   ? QStringLiteral("application/octet-stream")
                   : contentType);
    req.insert(QStringLiteral("title"), title);
    req.insert(QStringLiteral("description"), description);
    req.insert(QStringLiteral("tags"), tagsArr);
    req.insert(QStringLiteral("broadcast"), true);

    resetPublishState();
    setBusy(true);
    setStatus(QStringLiteral("queued"));

    const QString resp = callChronicle(
        QStringLiteral("publishFileJson"),
        QVariantList{compactJson(req)});
    handlePublishResponse(resp);
}

void WhistleblowerPlugin::handlePublishResponse(const QString& responseJson) {
    const QJsonObject obj = parseObject(responseJson);

    if (!obj.value(QStringLiteral("queued")).toBool()) {
        setBusy(false);
        setStatus(QStringLiteral("error"));
        const QString code  = obj.value(QStringLiteral("code")).toString();
        const QString error = obj.value(QStringLiteral("error")).toString();
        setLastError(code.isEmpty() ? error
                                    : QStringLiteral("%1: %2").arg(code, error));
        return;
    }

    setCurrentPublishId(obj.value(QStringLiteral("publish_id")).toString());
    if (m_pollTimer != nullptr && !m_pollTimer->isActive()) {
        m_pollTimer->start();
    }
}

void WhistleblowerPlugin::pollPublishStatus() {
    const QString publishId = currentPublishId();
    if (publishId.isEmpty()) {
        m_pollTimer->stop();
        return;
    }

    const QString resp = callChronicle(
        QStringLiteral("publishStatusJson"),
        QVariantList{publishId});
    const QJsonObject obj = parseObject(resp);

    const QString newStatus = obj.value(QStringLiteral("status")).toString();
    if (!newStatus.isEmpty()) {
        setStatus(newStatus);
    }

    const QString newCid = obj.value(QStringLiteral("cid")).toString();
    if (!newCid.isEmpty() && newCid != cid()) {
        setCid(newCid);
    }
    const QString newHash = obj.value(QStringLiteral("metadata_hash")).toString();
    if (!newHash.isEmpty() && newHash != metadataHash()) {
        setMetadataHash(newHash);
    }

    const bool terminal = (newStatus == QStringLiteral("broadcast_sent") ||
                           newStatus == QStringLiteral("error"));
    if (terminal) {
        m_pollTimer->stop();
        setBusy(false);
        if (newStatus == QStringLiteral("error")) {
            const QString code  = obj.value(QStringLiteral("code")).toString();
            const QString error = obj.value(QStringLiteral("error")).toString();
            setLastError(code.isEmpty() ? error
                                        : QStringLiteral("%1: %2").arg(code, error));
        }
    }
}

QString WhistleblowerPlugin::listPublishedJson() {
    return callChronicle(QStringLiteral("listPublishedJson"));
}
