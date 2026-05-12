#include "chronicle_anchor_client.h"

#include <QDebug>
#include <QJsonDocument>
#include <QJsonObject>
#include <QProcessEnvironment>

namespace {
constexpr const char* kLibName     = "libchronicle_registry_ffi.so";
constexpr const char* kEnvOverride = "CHRONICLE_REGISTRY_FFI_PATH";

QString errJson(const QString& msg) {
    QJsonObject obj;
    obj.insert(QStringLiteral("ok"), false);
    obj.insert(QStringLiteral("error"), msg);
    return QString::fromUtf8(QJsonDocument(obj).toJson(QJsonDocument::Compact));
}
} // namespace

ChronicleAnchorClient::ChronicleAnchorClient(QObject* parent) : QObject(parent) {}

ChronicleAnchorClient::~ChronicleAnchorClient() {
    if (m_lib.isLoaded()) m_lib.unload();
}

bool ChronicleAnchorClient::ensureLoaded(QString* error) {
    if (m_loaded) return true;

    const QString override =
        QProcessEnvironment::systemEnvironment().value(QLatin1String(kEnvOverride));
    const QString fileName = override.isEmpty() ? QString::fromLatin1(kLibName) : override;

    m_lib.setFileName(fileName);
    if (!m_lib.load()) {
        m_lastError = QStringLiteral("could not load %1: %2")
                          .arg(fileName, m_lib.errorString());
        if (error != nullptr) *error = m_lastError;
        return false;
    }

    m_initRegistry = reinterpret_cast<FnCall>(m_lib.resolve("chronicle_registry_init_registry"));
    m_indexBatch   = reinterpret_cast<FnCall>(m_lib.resolve("chronicle_registry_index_batch"));
    m_getRegistry  = reinterpret_cast<FnCall>(m_lib.resolve("chronicle_registry_get_registry"));
    m_freeString   = reinterpret_cast<FnFree>(m_lib.resolve("chronicle_registry_free_string"));
    m_version      = reinterpret_cast<FnVer> (m_lib.resolve("chronicle_registry_version"));

    if (m_initRegistry == nullptr || m_indexBatch == nullptr ||
        m_getRegistry == nullptr || m_freeString == nullptr ||
        m_version == nullptr) {
        m_lastError = QStringLiteral("missing symbols in %1 (rebuild the FFI crate?)")
                          .arg(m_lib.fileName());
        if (error != nullptr) *error = m_lastError;
        m_lib.unload();
        return false;
    }

    m_loaded = true;
    qDebug() << "ChronicleAnchorClient: loaded" << m_lib.fileName();
    return true;
}

QString ChronicleAnchorClient::version() {
    if (!ensureLoaded()) return errJson(m_lastError);
    char* result = m_version();
    if (result == nullptr) return errJson(QStringLiteral("FFI returned null"));
    const QString s = QString::fromUtf8(result);
    m_freeString(result);
    return s;
}

QString ChronicleAnchorClient::initRegistry(const QString& argsJson) {
    // ensureLoaded must run BEFORE we evaluate m_initRegistry — function-call
    // argument evaluation reads the member at call time, and on the first
    // call the symbols haven't been resolved yet.
    if (!ensureLoaded()) return errJson(m_lastError);
    return callJson(m_initRegistry, argsJson, "init_registry");
}

QString ChronicleAnchorClient::indexBatch(const QString& argsJson) {
    if (!ensureLoaded()) return errJson(m_lastError);
    return callJson(m_indexBatch, argsJson, "index_batch");
}

QString ChronicleAnchorClient::getRegistry(const QString& argsJson) {
    if (!ensureLoaded()) return errJson(m_lastError);
    return callJson(m_getRegistry, argsJson, "get_registry");
}

QString ChronicleAnchorClient::callJson(FnCall fn, const QString& argsJson,
                                        const char* fnName) {
    if (fn == nullptr) {
        return errJson(QStringLiteral("ffi: %1 not resolved")
                           .arg(QString::fromLatin1(fnName)));
    }
    const QByteArray args = argsJson.toUtf8();
    char* result = fn(args.constData());
    if (result == nullptr) {
        return errJson(QStringLiteral("ffi: %1 returned null")
                           .arg(QString::fromLatin1(fnName)));
    }
    const QString s = QString::fromUtf8(result);
    m_freeString(result);
    return s;
}
