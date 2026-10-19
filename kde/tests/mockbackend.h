#pragma once

#include <QObject>
#include <QVariantMap>
#include <QVariantList>
#include "../battery.h"

class MockBackend : public QObject
{
    Q_OBJECT
    Q_PROPERTY(QVariantMap state MEMBER state NOTIFY stateChanged)
    Q_PROPERTY(bool available MEMBER available CONSTANT)
    Q_PROPERTY(QVariantList devices MEMBER devices NOTIFY devicesChanged)
    Q_PROPERTY(QVariantList candidates MEMBER candidates CONSTANT)
    Q_PROPERTY(QString daemonError MEMBER empty CONSTANT)
    Q_PROPERTY(QString deviceOperation MEMBER empty CONSTANT)
    Q_PROPERTY(QVariantMap eqDetail MEMBER eqDetail NOTIFY eqDetailChanged)
    Q_PROPERTY(bool eqBusy MEMBER no CONSTANT)
    Q_PROPERTY(QVariantList presets MEMBER presets CONSTANT)
    Q_PROPERTY(bool scanning MEMBER no CONSTANT)
    Q_PROPERTY(bool startOnLogin MEMBER no CONSTANT)
public:
    QVariantMap state{{"Connected", true}, {"Address", "aa:bb:cc:dd:ee:ff"}, {"ModelName", "AirPods Pro 2"}, {"Model", "A2698"}, {"MicStatus", "idle"},
        {"Features", QStringList{"anc", "adaptive", "ca", "one_bud_anc"}},
        {"BatteryLeft", 82}, {"BatteryRight", 76}, {"BatteryCase", 43}, {"PreferredDevice", "AA:BB:CC:DD:EE:FF"},
        {"ChargingCase", true}, {"EarLeft", true}, {"EarRight", true},
        {"AncMode", "noise"}, {"EqPreset", "bass"}, {"EqStatus", "active"},
        {"AutoReconnect", true}, {"PauseOnRemoval", true}, {"Version", "0.3.0"}};
    bool available = true, no = false;
    QString empty;
    QVariantList devices{QVariantMap{{"address", "AA:BB:CC:DD:EE:FF"}, {"name", "Test AirPods"}}};
    QVariantList candidates;
    QVariantMap eqDetail{{"id", "bass"}, {"name", "Bass Boost"}, {"description", "Bass preset"}, {"preamp", -3},
        {"bands", QVariantList{QVariantMap{{"type", "peaking"}, {"freq", 100}, {"q", 1}, {"gain", 6}}}}};
    QVariantMap saved;
    bool activated = false;
    bool failRename = false;
    QVariantList presets{QVariantMap{{"name", "AirPods Pro (Crinacle)"}, {"id", "airpods-pro-crinacle"}},
        QVariantMap{{"name", "Bass Boost"}, {"id", "bass"}}, QVariantMap{{"name", "Flat"}, {"id", "flat"}},
        QVariantMap{{"name", "Vocal Clarity"}, {"id", "vocal"}},
        QVariantMap{{"name", "User"}, {"id", "user"}, {"user_editable", true}}};
    QString lastMethod;
    QVariant lastValue;
    Q_INVOKABLE void control(const QString &method, const QVariant &value = {})
    { lastMethod = method; lastValue = value; }
    Q_INVOKABLE void refresh() {}
    Q_INVOKABLE void renameDevice(const QString &address, const QString &name) {
        lastMethod = "RenameDevice";
        lastValue = QVariantMap{{"address", address}, {"name", name}};
        if (failRename) { emit error("Test rename failure"); return; }
        for (auto &entry : devices) {
            auto device = entry.toMap();
            if (device.value("address").toString() == address) { device["name"] = name; entry = device; }
        }
        emit devicesChanged();
        emit deviceRenamed();
    }
    Q_INVOKABLE QColor batteryColor(int level) const { return ::batteryColor(level); }
    Q_INVOKABLE void scan() { lastMethod = "QuickPairScan"; }
    Q_INVOKABLE void setting(const QString &name, const QVariant &value) { lastMethod = name; lastValue = value; }
    Q_INVOKABLE void saveEqPreset(const QVariantMap &p, bool activate) { saved = p; activated = activate; }
    Q_INVOKABLE void deleteEqPreset(const QString &id) { lastMethod = "DeleteEqPreset"; lastValue = id; }
    Q_INVOKABLE void setStartOnLogin(bool value) { lastMethod = "StartOnLogin"; lastValue = value; }
    Q_INVOKABLE void copyText(const QString &) {}
signals:
    void devicesChanged();
    void deviceRenamed();
    void eqDetailChanged();
    void eqSaved(const QString &id);
    void eqDeleted(const QString &id);
    void stateChanged();
    void error(const QString &message);
};
