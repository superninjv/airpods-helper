#pragma once

#include <QObject>
#include <QVariantMap>
#include <QDBusMessage>
#include <QColor>

class Client : public QObject
{
    Q_OBJECT
    Q_PROPERTY(QVariantMap state READ state NOTIFY stateChanged)
    Q_PROPERTY(bool available READ available NOTIFY stateChanged)
    Q_PROPERTY(QVariantList devices READ devices NOTIFY devicesChanged)
    Q_PROPERTY(QVariantList candidates READ candidates NOTIFY devicesChanged)
    Q_PROPERTY(QVariantList presets READ presets NOTIFY presetsChanged)
    Q_PROPERTY(bool scanning READ scanning NOTIFY devicesChanged)
    Q_PROPERTY(bool startOnLogin READ startOnLogin NOTIFY loginChanged)
    Q_PROPERTY(QString daemonError READ daemonError NOTIFY stateChanged)
    Q_PROPERTY(QString deviceOperation READ deviceOperation NOTIFY devicesChanged)
    Q_PROPERTY(QVariantMap eqDetail READ eqDetail NOTIFY eqDetailChanged)
    Q_PROPERTY(bool eqBusy READ eqBusy NOTIFY eqBusyChanged)
public:
    explicit Client(QObject *parent = nullptr);
    QVariantMap state() const { return m_state; }
    bool available() const { return m_available; }
    QVariantList devices() const { return m_devices; }
    QVariantList candidates() const { return m_candidates; }
    QVariantList presets() const { return m_presets; }
    bool scanning() const { return m_scanning; }
    bool startOnLogin() const;
    QString daemonError() const { return m_daemonError; }
    QString deviceOperation() const { return m_deviceOperation; }
    QVariantMap eqDetail() const { return m_eqDetail; }
    bool eqBusy() const { return m_eqBusy; }
    Q_INVOKABLE void refresh();
    Q_INVOKABLE QColor batteryColor(int level) const;
    Q_INVOKABLE void control(const QString &method, const QVariant &value = {});
    Q_INVOKABLE void setting(const QString &name, const QVariant &value);
    Q_INVOKABLE void scan();
    Q_INVOKABLE void renameDevice(const QString &address, const QString &name);
    Q_INVOKABLE void setStartOnLogin(bool enabled);
    Q_INVOKABLE void loadEqPreset(const QString &id);
    Q_INVOKABLE void saveEqPreset(const QVariantMap &preset, bool activate);
    Q_INVOKABLE void deleteEqPreset(const QString &id);
    Q_INVOKABLE void copyText(const QString &text);
signals:
    void deviceRenamed();
    void stateChanged();
    void devicesChanged();
    void presetsChanged();
    void loginChanged();
    void eqDetailChanged();
    void eqBusyChanged();
    void eqSaved(const QString &id);
    void eqDeleted(const QString &id);
    void error(const QString &message);
private slots:
    void refreshDevices();
    void propertiesChanged(const QString &interface, const QVariantMap &changed,
                           const QStringList &invalidated);
private:
    void request(const QString &method, const QVariantList &args = {});
    void listDevices(const QString &method, const QVariantList &args = {});
    void loadPresets();
    QVariantMap m_state;
    QVariantList m_devices, m_candidates, m_presets;
    QVariantMap m_eqDetail;
    QString m_daemonError, m_deviceOperation;
    bool m_eqBusy = false;
    bool m_available = false, m_scanning = false;
    int m_generation = 0;
    int m_eqRequest = 0;
};
