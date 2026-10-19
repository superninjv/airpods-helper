#include "client.h"
#include "battery.h"
#include "trayicon.h"

#include <Plasma/Applet>
#include <KPluginFactory>
#include <QQmlEngine>
#include <QGuiApplication>
#include <QQuickItem>

class AirPodsApplet : public Plasma::Applet
{
    Q_OBJECT
    Q_PROPERTY(QObject *backend READ backend CONSTANT)
    Q_PROPERTY(QIcon batteryIcon READ trayIcon NOTIFY batteryIconChanged)
public:
    AirPodsApplet(QObject *parent, const KPluginMetaData &data, const QVariantList &args)
        : Plasma::Applet(parent, data, args), m_client(this)
    {
        connect(&m_client, &Client::stateChanged, this, &AirPodsApplet::batteryIconChanged);
        connect(qGuiApp, &QGuiApplication::paletteChanged, this, &AirPodsApplet::batteryIconChanged);
    }
    QObject *backend() { return &m_client; }
    QIcon trayIcon() const { return ::batteryIcon(batteryLevel(m_client.state()), qGuiApp->palette()); }
    Q_INVOKABLE void prepareEngine(QQuickItem *item)
    {
        auto *engine = qmlEngine(item);
        if (engine && !engine->imageProvider("airpods-glyph")) engine->addImageProvider("airpods-glyph", new PanelIcons);
    }
private:
    Client m_client;
signals:
    void batteryIconChanged();
};

K_PLUGIN_CLASS_WITH_JSON(AirPodsApplet, "metadata.json")
#include "applet.moc"
