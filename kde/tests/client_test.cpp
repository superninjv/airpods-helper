#include "client.h"
#include "eqband.h"
#include <QDBusContext>
#include <QDBusConnection>
#include <QDBusMetaType>
#include <QtTest>
#include <limits>

struct Device { QString address, name; };
struct Candidate { QString address, name, model; qint16 rssi; bool pairing; };
Q_DECLARE_METATYPE(Candidate)
struct Preset { QString id, name, description; bool editable; };
Q_DECLARE_METATYPE(Device)
Q_DECLARE_METATYPE(Preset)
QDBusArgument &operator<<(QDBusArgument &a, const Device &d)
{ a.beginStructure(); a << d.address << d.name; a.endStructure(); return a; }
const QDBusArgument &operator>>(const QDBusArgument &a, Device &d)
{ a.beginStructure(); a >> d.address >> d.name; a.endStructure(); return a; }
QDBusArgument &operator<<(QDBusArgument &a, const Preset &p)
{ a.beginStructure(); a << p.id << p.name << p.description << p.editable; a.endStructure(); return a; }
const QDBusArgument &operator>>(const QDBusArgument &a, Preset &p)
{ a.beginStructure(); a >> p.id >> p.name >> p.description >> p.editable; a.endStructure(); return a; }

QDBusArgument &operator<<(QDBusArgument &a, const Candidate &d)
{ a.beginStructure(); a << d.address << d.name << d.model << d.rssi << d.pairing; a.endStructure(); return a; }
const QDBusArgument &operator>>(const QDBusArgument &a, Candidate &d)
{ a.beginStructure(); a >> d.address >> d.name >> d.model >> d.rssi >> d.pairing; a.endStructure(); return a; }

class FakeDaemon : public QObject, protected QDBusContext
{
    Q_OBJECT
    Q_CLASSINFO("D-Bus Interface", "org.costa.AirPods")
    Q_PROPERTY(bool Connected MEMBER connected)
    Q_PROPERTY(int BatteryLeft MEMBER batteryLeft)
    Q_PROPERTY(bool AutoReconnect MEMBER autoReconnect)
    Q_PROPERTY(QString PreferredDevice MEMBER preferred)
public:
    bool connected = true, autoReconnect = true;
    int batteryLeft = 75;
    int noiseLevel = -1;
    QString ancMode, preferred, savedId, selectedId, deletedId;
    EqBands savedBands;
    QStringList deviceCalls;
    bool failSave = false;
    QString deviceName = "Test AirPods";
signals:
    void DevicesChanged();
public slots:
    void SetAncMode(const QString &value) { ancMode = value; }
    void SetAdaptiveNoiseLevel(uchar value) { noiseLevel = value; }
    QList<Device> ListPaired() const { return {{"AA:BB:CC:DD:EE:FF", deviceName}}; }
    void RenameDevice(const QString &address, const QString &name) {
        deviceCalls << "RenameDevice:" + address;
        deviceName = name;
        emit DevicesChanged();
    }
    QList<Preset> GetEqPresets() const { return {{"bass", "Bass Boost", "Bass preset", false}, {"user", "User preset", "Editable", true}}; }
    QList<Candidate> QuickPairScan(uint) const { return {{"11:22:33:44:55:66", "Nearby", "A2698", -42, false}}; }
    void Pair(const QString &address) { deviceCalls << "Pair:" + address; }
    void ConnectTo(const QString &address) { deviceCalls << "ConnectTo:" + address; }
    QString GetEqPreset(const QString &, QString &description, double &preamp, EqBands &bands) const {
        description = "Bass preset"; preamp = -3;
        bands = {{"peaking", 100, 1, 6}};
        return "Bass Boost";
    }
    void SaveEqPreset(const QString &id, const QString &, const QString &, double, const EqBands &bands) {
        if (failSave) { sendErrorReply(QDBusError::Failed, "Disk is full"); return; }
        savedId = id; savedBands = bands;
    }
    void SetEqPreset(const QString &id) { selectedId = id; }
    void DeleteEqPreset(const QString &id) { deletedId = id; }
};

class ClientTest : public QObject
{
    Q_OBJECT
private slots:
    void followsDaemon()
    {
        auto bus = QDBusConnection::sessionBus();
        QVERIFY(bus.isConnected());
        qDBusRegisterMetaType<Candidate>();
        qDBusRegisterMetaType<QList<Candidate>>();
        qDBusRegisterMetaType<EqBand>();
        qDBusRegisterMetaType<EqBands>();
        qDBusRegisterMetaType<Device>();
        qDBusRegisterMetaType<QList<Device>>();
        qDBusRegisterMetaType<Preset>();
        qDBusRegisterMetaType<QList<Preset>>();
        FakeDaemon daemon;
        QVERIFY(bus.registerObject("/org/costa/AirPods", &daemon,
            QDBusConnection::ExportAllProperties | QDBusConnection::ExportAllSlots | QDBusConnection::ExportAllSignals));
        QVERIFY(bus.registerService("org.costa.AirPods"));
        Client client;
        QTRY_VERIFY(client.available());
        QCOMPARE(client.state().value("BatteryLeft").toInt(), 75);
        QTRY_COMPARE(client.devices().size(), 1);
        QCOMPARE(client.devices().first().toMap().value("name").toString(), "Test AirPods");
        QTRY_COMPARE(client.presets().size(), 2);
        QCOMPARE(client.presets().first().toMap().value("id").toString(), "bass");
        auto changed = QDBusMessage::createSignal("/org/costa/AirPods",
            "org.freedesktop.DBus.Properties", "PropertiesChanged");
        changed.setArguments({"org.costa.AirPods", QVariantMap{{"BatteryLeft", 24}}, QStringList{}});
        QVERIFY(bus.send(changed));
        QTRY_COMPARE(client.state().value("BatteryLeft").toInt(), 24);

        client.control("SetAncMode", "transparency");
        QTRY_COMPARE(daemon.ancMode, "transparency");
        client.control("SetAdaptiveNoiseLevel", 42);
        QTRY_COMPARE(daemon.noiseLevel, 42);
        client.setting("AutoReconnect", false);
        QTRY_VERIFY(!daemon.autoReconnect);
        QSignalSpy errors(&client, &Client::error);
        client.control("SetAdaptiveNoiseLevel", 101);
        QCOMPARE(errors.size(), 1);
        QCOMPARE(daemon.noiseLevel, 42);

        client.setting("PreferredDevice", " aa:bb:cc:dd:ee:ff ");
        QTRY_COMPARE(daemon.preferred, "AA:BB:CC:DD:EE:FF");
        client.setting("PreferredDevice", "");
        QTRY_VERIFY(daemon.preferred.isEmpty());
        client.setting("PreferredDevice", "invalid");
        client.setting("AutoReconnect", "true");
        QCOMPARE(errors.size(), 3);
        QVERIFY(!daemon.autoReconnect);
        client.scan();
        QTRY_VERIFY(!client.scanning());
        QCOMPARE(client.devices().first().toMap().value("name").toString(), "Test AirPods");
        QCOMPARE(client.candidates().size(), 1);
        QCOMPARE(client.candidates().first().toMap().value("rssi").toInt(), -42);
        QVERIFY(!client.candidates().first().toMap().value("in_pair_mode").toBool());
        client.control("Pair", "11:22:33:44:55:66");
        QTRY_COMPARE(daemon.deviceCalls.size(), 2);
        QCOMPARE(daemon.deviceCalls, QStringList({"Pair:11:22:33:44:55:66", "ConnectTo:11:22:33:44:55:66"}));
        QTRY_VERIFY(client.deviceOperation().isEmpty());
        client.loadEqPreset("bass");
        QTRY_COMPARE(client.eqDetail().value("id").toString(), "bass");
        QCOMPARE(client.eqDetail().value("preamp").toDouble(), -3);
        QCOMPARE(client.eqDetail().value("bands").toList().first().toMap().value("gain").toDouble(), 6);
        auto preset = client.eqDetail();
        preset["id"] = "my-preset";
        QSignalSpy saved(&client, &Client::eqSaved);
        client.saveEqPreset(preset, true);
        QTRY_COMPARE(saved.size(), 1);
        QCOMPARE(daemon.savedId, "my-preset");
        QCOMPARE(daemon.savedBands.size(), 1);
        QCOMPARE(daemon.savedBands.first().freq, 100);
        QTRY_COMPARE(daemon.selectedId, "my-preset");
        QVERIFY(!client.eqBusy());
        daemon.failSave = true;
        client.saveEqPreset(preset, false);
        QTRY_COMPARE(errors.size(), 4);
        QVERIFY(errors.last().first().toString().contains("Disk is full"));
        QCOMPARE(saved.size(), 1);
        QVERIFY(!client.eqBusy());
        preset["preamp"] = std::numeric_limits<double>::infinity();
        client.saveEqPreset(preset, false);
        QCOMPARE(errors.size(), 5);
        preset["preamp"] = 0;
        preset.remove("bands");
        client.saveEqPreset(preset, false);
        QCOMPARE(errors.size(), 6);
        client.deleteEqPreset("bass");
        QCOMPARE(errors.size(), 7);
        QVERIFY(daemon.deletedId.isEmpty());
        QSignalSpy deleted(&client, &Client::eqDeleted);
        client.deleteEqPreset("user");
        QTRY_COMPARE(deleted.size(), 1);
        QCOMPARE(daemon.deletedId, "user");

        const auto errorCount = errors.size();
        client.renameDevice("invalid", "AirPods");
        client.renameDevice("AA:BB:CC:DD:EE:FF", "   ");
        client.renameDevice("AA:BB:CC:DD:EE:FF", "Bad\nname");
        QCOMPARE(errors.size(), errorCount + 3);
        QSignalSpy renamed(&client, &Client::deviceRenamed);
        client.renameDevice(" aa:bb:cc:dd:ee:ff ", "  My AirPods  ");
        QTRY_COMPARE(renamed.size(), 1);
        QTRY_COMPARE(client.devices().first().toMap().value("name").toString(), "My AirPods");
        QCOMPARE(daemon.deviceCalls.last(), "RenameDevice:AA:BB:CC:DD:EE:FF");

        QVERIFY(bus.unregisterService("org.costa.AirPods"));
        QTRY_VERIFY(!client.available());
        QVERIFY(client.state().isEmpty());
        daemon.batteryLeft = 90;
        QVERIFY(bus.registerService("org.costa.AirPods"));
        QTRY_VERIFY(client.available());
        QCOMPARE(client.state().value("BatteryLeft").toInt(), 90);
        bus.unregisterService("org.costa.AirPods");
        bus.unregisterObject("/org/costa/AirPods");
    }
};
QTEST_GUILESS_MAIN(ClientTest)
#include "client_test.moc"
