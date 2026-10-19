#include "client.h"
#include "battery.h"
#include "eqband.h"

#include <QDBusArgument>
#include <QDBusConnection>
#include <QDBusPendingCallWatcher>
#include <QDBusPendingReply>
#include <QDBusServiceWatcher>
#include <QDBusVariant>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QSaveFile>
#include <QStandardPaths>
#include <QCoreApplication>
#include <QGuiApplication>
#include <QClipboard>
#include <QRegularExpression>
#include <cmath>
#include <algorithm>

static const QString service = QStringLiteral("org.costa.AirPods");
static const QString path = QStringLiteral("/org/costa/AirPods");
static const QString properties = QStringLiteral("org.freedesktop.DBus.Properties");

static QDBusMessage message(const QString &interface, const QString &method,
                            const QVariantList &args)
{
    auto msg = QDBusMessage::createMethodCall(service, path, interface, method);
    msg.setArguments(args);
    return msg;
}

Client::Client(QObject *parent) : QObject(parent)
{
    qDBusRegisterMetaType<EqBand>();
    qDBusRegisterMetaType<EqBands>();
    auto bus = QDBusConnection::sessionBus();
    auto *watcher = new QDBusServiceWatcher(service, bus,
        QDBusServiceWatcher::WatchForOwnerChange, this);
    connect(watcher, &QDBusServiceWatcher::serviceOwnerChanged, this,
        [this](const QString &, const QString &, const QString &owner) {
            ++m_generation;
            m_available = false;
            m_state.clear();
            m_devices.clear();
            m_candidates.clear();
            m_presets.clear();
            m_eqDetail.clear();
            m_eqBusy = false;
            m_scanning = false;
            m_deviceOperation.clear();
            emit stateChanged();
            emit devicesChanged();
            emit presetsChanged();
            emit eqDetailChanged();
            emit eqBusyChanged();
            if (!owner.isEmpty()) refresh();
        });
    bus.connect(service, path, properties, "PropertiesChanged", this,
        SLOT(propertiesChanged(QString,QVariantMap,QStringList)));
    bus.connect(service, path, service, "DevicesChanged", this, SLOT(refreshDevices()));
    refresh();
}

QColor Client::batteryColor(int level) const
{
    return ::batteryColor(level);
}

void Client::refresh()
{
    const int generation = m_generation;
    auto *call = new QDBusPendingCallWatcher(QDBusConnection::sessionBus().asyncCall(
        message(properties, "GetAll", {service})), this);
    connect(call, &QDBusPendingCallWatcher::finished, this, [this, generation](auto *call) {
        QDBusPendingReply<QVariantMap> reply = *call;
        call->deleteLater();
        if (generation != m_generation) return;
        m_available = !reply.isError();
        m_state = m_available ? reply.value() : QVariantMap{};
        m_daemonError = reply.isError() ? reply.error().message() : QString{};
        emit stateChanged();
        if (m_available) {
            listDevices("ListPaired");
            loadPresets();
            loadEqPreset(m_state.value("EqPreset").toString());
        }
    });
}

void Client::propertiesChanged(const QString &interface, const QVariantMap &changed,
                               const QStringList &invalidated)
{
    if (interface != service) return;
    if (!invalidated.isEmpty()) { refresh(); return; }
    for (auto it = changed.begin(); it != changed.end(); ++it)
        m_state.insert(it.key(), it.value());
    emit stateChanged();
    if (changed.contains("EqPreset")) loadEqPreset(changed.value("EqPreset").toString());
}

void Client::request(const QString &method, const QVariantList &args)
{
    const bool device = QStringList{"Pair", "ConnectTo", "Disconnect", "Reconnect", "RenameDevice"}.contains(method);
    if (device && !m_deviceOperation.isEmpty()) return;
    if (device) { m_deviceOperation = method; emit devicesChanged(); }
    const int generation = m_generation;
    auto *call = new QDBusPendingCallWatcher(QDBusConnection::sessionBus().asyncCall(
        message(service, method, args), 60000), this);
    connect(call, &QDBusPendingCallWatcher::finished, this, [this, method, args, device, generation](auto *call) {
        QDBusPendingReply<> reply = *call;
        call->deleteLater();
        if (generation != m_generation) return;
        if (device) { m_deviceOperation.clear(); emit devicesChanged(); }
        if (reply.isError()) emit error(reply.error().message());
        else if (method == "RenameDevice") emit deviceRenamed();
        else if (method == "Pair") {
            listDevices("ListPaired");
            request("ConnectTo", args);
        }
        emit stateChanged();
    });
}

void Client::control(const QString &method, const QVariant &value)
{
    if (method == "Pair" || method == "ConnectTo") {
        const auto address = value.toString().trimmed().toUpper();
        static const QRegularExpression mac("^[0-9A-F]{2}(:[0-9A-F]{2}){5}$");
        if (!mac.match(address).hasMatch()) { emit error(tr("Enter a valid Bluetooth address.")); return; }
        request(method, {address});
        return;
    }
    const QStringList strings = {"SetAncMode", "SetMicMode", "SetEqPreset", "ConnectTo", "Pair"};
    const QStringList booleans = {"SetConversationalAwareness", "SetOneBudAnc", "SetVolumeSwipe"};
    if (strings.contains(method)) request(method, {value.toString()});
    else if (booleans.contains(method)) request(method, {value.toBool()});
    else if (method == "SetAdaptiveNoiseLevel" && value.toInt() >= 0 && value.toInt() <= 100)
        request(method, {QVariant::fromValue(static_cast<uchar>(value.toInt()))});
    else if (method == "Disconnect" || method == "Reconnect" || method == "DisableEq") request(method);
    else emit error(tr("Unsupported control: %1").arg(method));
}

void Client::refreshDevices() { listDevices("ListPaired"); }

void Client::renameDevice(const QString &address, const QString &name)
{
    const auto value = name.trimmed();
    static const QRegularExpression mac("^[0-9A-F]{2}(:[0-9A-F]{2}){5}$");
    if (!mac.match(address.trimmed().toUpper()).hasMatch() || value.isEmpty()
        || value.toUcs4().size() > 64 || value.contains(QRegularExpression("\\p{Cc}"))) {
        emit error(tr("Enter a valid Bluetooth address and a name of 1–64 characters without control characters."));
        return;
    }
    request("RenameDevice", {address.trimmed().toUpper(), value});
}

void Client::setting(const QString &name, const QVariant &value)
{
    const QStringList settings = {"PauseOnRemoval", "ResumeOnInsert", "AutoReconnect", "EqAutoLoad", "MicSource"};
    QVariant validated = value;
    if (name == "PreferredDevice") {
        const auto address = value.toString().trimmed().toUpper();
        static const QRegularExpression mac("^[0-9A-F]{2}(:[0-9A-F]{2}){5}$");
        if (!address.isEmpty() && !mac.match(address).hasMatch()) {
            emit error(tr("Enter a Bluetooth address like AA:BB:CC:DD:EE:FF, or leave it empty.")); return;
        }
        validated = address;
    } else if (!settings.contains(name) || value.metaType() != QMetaType::fromType<bool>()) {
        emit error(tr("Unsupported setting or value")); return;
    }
    auto *call = new QDBusPendingCallWatcher(QDBusConnection::sessionBus().asyncCall(
        message(properties, "Set", {service, name, QVariant::fromValue(QDBusVariant(validated))})), this);
    connect(call, &QDBusPendingCallWatcher::finished, this, [this](auto *call) {
        QDBusPendingReply<> reply = *call;
        if (reply.isError()) emit error(reply.error().message());
        call->deleteLater();
    });
}

void Client::listDevices(const QString &method, const QVariantList &args)
{
    const int generation = m_generation;
    const bool scan = method == "QuickPairScan";
    auto *call = new QDBusPendingCallWatcher(QDBusConnection::sessionBus().asyncCall(
        message(service, method, args), 60000), this);
    connect(call, &QDBusPendingCallWatcher::finished, this, [this, generation, scan](auto *call) {
        const auto reply = call->reply();
        call->deleteLater();
        if (generation != m_generation) return;
        if (scan) { m_scanning = false; emit devicesChanged(); }
        if (reply.type() == QDBusMessage::ErrorMessage) { emit error(reply.errorMessage()); return; }
        if (reply.arguments().isEmpty()) return;
        const auto array = qvariant_cast<QDBusArgument>(reply.arguments().first());
        QVariantList devices;
        array.beginArray();
        while (!array.atEnd()) {
            QString address, name, model;
            qint16 rssi = 0;
            bool pairing = false;
            array.beginStructure();
            array >> address >> name;
            if (scan) array >> model >> rssi >> pairing;
            array.endStructure();
            devices.append(QVariantMap{{"address", address}, {"name", name}, {"model", model},
                {"rssi", rssi}, {"in_pair_mode", pairing}});
        }
        array.endArray();
        if (scan) m_candidates = devices;
        else m_devices = devices;
        emit devicesChanged();
    });
}

void Client::scan()
{
    if (m_scanning) return;
    m_scanning = true;
    m_candidates.clear();
    emit devicesChanged();
    listDevices("QuickPairScan", {QVariant::fromValue(uint(10))});
}

void Client::loadPresets()
{
    const int generation = m_generation;
    auto *call = new QDBusPendingCallWatcher(QDBusConnection::sessionBus().asyncCall(
        message(service, "GetEqPresets", {})), this);
    connect(call, &QDBusPendingCallWatcher::finished, this, [this, generation](auto *call) {
        const auto reply = call->reply();
        call->deleteLater();
        if (generation != m_generation || reply.type() == QDBusMessage::ErrorMessage || reply.arguments().isEmpty()) return;
        const auto array = qvariant_cast<QDBusArgument>(reply.arguments().first());
        m_presets.clear();
        array.beginArray();
        while (!array.atEnd()) {
            QString id, name, description;
            bool editable;
            array.beginStructure();
            array >> id >> name >> description >> editable;
            array.endStructure();
            m_presets.append(QVariantMap{{"id", id}, {"name", name}, {"description", description}, {"user_editable", editable}});
        }
        array.endArray();
        emit presetsChanged();
    });
}

void Client::loadEqPreset(const QString &id)
{
    const int request = ++m_eqRequest;
    const int generation = m_generation;
    if (id.isEmpty()) { m_eqDetail.clear(); emit eqDetailChanged(); return; }
    auto *call = new QDBusPendingCallWatcher(QDBusConnection::sessionBus().asyncCall(
        message(service, "GetEqPreset", {id})), this);
    connect(call, &QDBusPendingCallWatcher::finished, this, [this, id, request, generation](auto *call) {
        QDBusPendingReply<QString, QString, double, EqBands> reply = *call;
        call->deleteLater();
        if (generation != m_generation || request != m_eqRequest) return;
        if (reply.isError()) { emit error(reply.error().message()); return; }
        QVariantList bands;
        for (const auto &band : reply.template argumentAt<3>())
            bands.append(QVariantMap{{"type", band.type}, {"freq", band.freq}, {"q", band.q}, {"gain", band.gain}});
        m_eqDetail = {{"id", id}, {"name", reply.template argumentAt<0>()},
            {"description", reply.template argumentAt<1>()}, {"preamp", reply.template argumentAt<2>()}, {"bands", bands}};
        emit eqDetailChanged();
    });
}

void Client::saveEqPreset(const QVariantMap &preset, bool activate)
{
    if (m_eqBusy) return;
    const auto id = preset.value("id").toString();
    const auto name = preset.value("name").toString().trimmed();
    const auto description = preset.value("description").toString().trimmed();
    const auto values = preset.value("bands").toList();
    static const QRegularExpression validId("^[a-z0-9][a-z0-9-]{0,47}$");
    bool valid = false;
    const double preamp = preset.value("preamp").toDouble(&valid);
    if (!validId.match(id).hasMatch() || name.isEmpty() || name.toUtf8().size() > 64 || description.toUtf8().size() > 256
        || !preset.value("bands").canConvert<QVariantList>() || !valid || !std::isfinite(preamp) || preamp < -24 || preamp > 12 || values.size() > 16) {
        emit error(tr("Invalid preset: check its name, ID, preamp (−24 to +12 dB), and band count (at most 16).")); return;
    }
    EqBands bands;
    const QStringList types{"peaking", "lowshelf", "highshelf", "lowpass", "highpass", "notch"};
    for (const auto &value : values) {
        const auto b = value.toMap();
        bool frequencyOk, qOk, gainOk;
        EqBand band{b.value("type").toString(), b.value("freq").toDouble(&frequencyOk),
            b.value("q").toDouble(&qOk), b.value("gain").toDouble(&gainOk)};
        if (!types.contains(band.type) || !frequencyOk || !qOk || !gainOk
            || !std::isfinite(band.freq) || !std::isfinite(band.q) || !std::isfinite(band.gain)
            || band.freq < 20 || band.freq > 20000 || band.q < 0.1 || band.q > 10 || band.gain < -24 || band.gain > 24) {
            emit error(tr("Invalid band: frequency must be 20–20000 Hz, Q 0.1–10, and gain −24 to +24 dB.")); return;
        }
        bands.append(band);
    }
    m_eqBusy = true;
    emit eqBusyChanged();
    const int generation = m_generation;
    auto *call = new QDBusPendingCallWatcher(QDBusConnection::sessionBus().asyncCall(
        message(service, "SaveEqPreset", {id, name, description, preamp, QVariant::fromValue(bands)}), 60000), this);
    connect(call, &QDBusPendingCallWatcher::finished, this, [this, id, activate, generation](auto *call) {
        QDBusPendingReply<> reply = *call;
        call->deleteLater();
        if (generation != m_generation) return;
        m_eqBusy = false;
        emit eqBusyChanged();
        if (reply.isError()) { emit error(reply.error().message()); return; }
        loadPresets();
        loadEqPreset(id);
        if (activate) request("SetEqPreset", {id});
        emit eqSaved(id);
    });
}

void Client::deleteEqPreset(const QString &id)
{
    if (m_eqBusy) return;
    const bool editable = std::any_of(m_presets.begin(), m_presets.end(), [&id](const QVariant &p) {
        return p.toMap().value("id") == id && p.toMap().value("user_editable").toBool();
    });
    if (!editable) { emit error(tr("Only user presets can be deleted.")); return; }
    m_eqBusy = true;
    emit eqBusyChanged();
    const int generation = m_generation;
    auto *call = new QDBusPendingCallWatcher(QDBusConnection::sessionBus().asyncCall(
        message(service, "DeleteEqPreset", {id}), 60000), this);
    connect(call, &QDBusPendingCallWatcher::finished, this, [this, id, generation](auto *call) {
        QDBusPendingReply<> reply = *call;
        call->deleteLater();
        if (generation != m_generation) return;
        m_eqBusy = false;
        emit eqBusyChanged();
        if (reply.isError()) { emit error(reply.error().message()); return; }
        loadPresets();
        loadEqPreset(m_state.value("EqPreset").toString());
        emit eqDeleted(id);
    });
}

void Client::copyText(const QString &text) { QGuiApplication::clipboard()->setText(text); }

static QString loginFile()
{
    return QStandardPaths::writableLocation(QStandardPaths::GenericConfigLocation)
        + "/autostart/org.costa.AirPodsHelper.desktop";
}

bool Client::startOnLogin() const { return QFile::exists(loginFile()); }

void Client::setStartOnLogin(bool enabled)
{
    if (!enabled) {
        if (QFile::exists(loginFile()) && !QFile::remove(loginFile())) emit error(tr("Could not remove login entry"));
    } else {
        const auto executable = QCoreApplication::applicationFilePath();
        QString quoted = executable;
        quoted.replace('\\', "\\\\").replace('"', "\\\"").replace('`', "\\`").replace('$', "\\$").replace('%', "%%");
        QDir().mkpath(QFileInfo(loginFile()).absolutePath());
        QSaveFile file(loginFile());
        if (!file.open(QIODevice::WriteOnly)
            || file.write(QString("[Desktop Entry]\nType=Application\nName=AirPods Helper\nExec=\"%1\" --minimized\nIcon=org.costa.AirPodsHelper\n").arg(quoted).toUtf8()) < 0
            || !file.commit()) emit error(tr("Could not write login entry"));
    }
    emit loginChanged();
}
