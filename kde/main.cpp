#include "battery.h"
#include "client.h"
#include "panel.h"
#include "icons.h"
#include "trayicon.h"
#include "theme.h"
#include <KConfigWatcher>

#include <KDBusService>
#include <KStatusNotifierItem>
#include <QActionGroup>
#include <QApplication>
#include <QMenu>
#include <QPainter>
#include <QQmlApplicationEngine>
#include <QQmlContext>
#include <QQmlError>
#include <QQuickWindow>
#include <QSignalBlocker>
#include <QUrl>
#include <cstdio>


int main(int argc, char **argv)
{
    QApplication app(argc, argv);
    app.setApplicationName("airpods-helper-kde");
    app.setApplicationDisplayName("AirPods Helper");
    app.setApplicationVersion(QStringLiteral(APP_VERSION));
    app.setOrganizationDomain("costa.org");
    app.setDesktopFileName("org.costa.AirPodsHelper");
    app.setQuitOnLastWindowClosed(false);
    QQuickStyle::setStyle("org.kde.plasma.components");
    applyDesktopTheme();
    const auto themeWatcher = KConfigWatcher::create(KSharedConfig::openConfig("kdeglobals"));
    QObject::connect(themeWatcher.data(), &KConfigWatcher::configChanged, &app, [] { applyDesktopTheme(); });
    KDBusService unique(KDBusService::Unique);

    Client client;
    QQmlApplicationEngine engine;
    engine.setOutputWarningsToStandardError(false);
    QObject::connect(&engine, &QQmlEngine::warnings, &app, [](const QList<QQmlError> &errors) {
        for (const auto &error : errors) std::fprintf(stderr, "%s\n", qPrintable(error.toString()));
    });
    engine.setProperty("_kirigamiTheme", QStringLiteral("Plasma"));
    engine.addImageProvider("airpods-glyph", new PanelIcons);
    engine.rootContext()->setContextProperty("backend", &client);
    engine.load(QUrl(QStringLiteral("qrc:/qt/qml/AirPods/qml/Main.qml")));
    if (engine.rootObjects().isEmpty()) {
        std::fputs("Could not load the AirPods panel QML.\n", stderr);
        return 1;
    }
    auto *window = qobject_cast<QQuickWindow *>(engine.rootObjects().first());
    if (!window) {
        std::fprintf(stderr, "The AirPods panel root is not a window: %s\n", engine.rootObjects().first()->metaObject()->className());
        return 1;
    }
    TrayPanel panel(window);
    auto show = [&panel] { panel.show(); };
    QObject::connect(&unique, &KDBusService::activateRequested, &app, show);

    KStatusNotifierItem tray("airpods-helper", &app);
    tray.setTitle("AirPods Helper");
    tray.setCategory(KStatusNotifierItem::Hardware);
    tray.setStatus(KStatusNotifierItem::Active);
    tray.setStandardActionsEnabled(false);
    QObject::connect(&tray, &KStatusNotifierItem::activateRequested, &app, [&panel](bool, const QPoint &position) {
        panel.toggle(position);
    });
    auto *menu = new QMenu;
    auto *headline = menu->addAction("AirPods Helper");
    headline->setEnabled(false);
    menu->addSeparator();
    auto *modes = new QActionGroup(menu);
    const QList<QPair<QString, QString>> labels = {
        {"off", "Off"}, {"noise", "Noise Cancel"},
        {"transparency", "Transparency"}, {"adaptive", "Adaptive"}};
    for (const auto &[id, label] : labels) {
        auto *action = menu->addAction(label);
        action->setData(id);
        action->setCheckable(true);
        modes->addAction(action);
        QObject::connect(action, &QAction::triggered, &client, [&client, id] { client.control("SetAncMode", id); });
    }
    menu->addSeparator();
    menu->addAction("Open AirPods Helper", &app, show);
    menu->addAction("Quit", &app, &QApplication::quit);
    tray.setContextMenu(menu);
    auto updateTray = [&] {
        const auto state = client.state();
        const int level = batteryLevel(state);
        const bool connected = state.value("Connected").toBool();
        const auto name = state.value("ModelName", "AirPods").toString();
        QStringList battery;
        const bool headphones = state.value("Features").toStringList().contains("headphones");
        for (const QString &part : {QStringLiteral("Left"), QStringLiteral("Right"), QStringLiteral("Case")}) {
            if (headphones && part != "Left") continue;
            const int charge = state.value("Battery" + part, -1).toInt();
            if (charge < 0 || !connected) continue;
            battery << QString("%1: %2%%3").arg(headphones ? "Battery" : part).arg(charge)
                .arg(state.value("Charging" + part).toBool() ? " (charging)" : "");
        }
        const auto detail = !client.available() ? QString("Daemon not running")
            : !connected ? QString("Disconnected") : battery.isEmpty() ? QString("Connected") : battery.join(" · ");
        tray.setIconByPixmap(batteryIcon(level, app.palette()));
        tray.setToolTip("audio-headphones", name, detail.toHtmlEscaped());
        headline->setText(detail);
        const auto features = state.value("Features").toStringList();
        for (auto *action : modes->actions()) {
            QSignalBlocker blocker(action);
            action->setVisible(connected && features.contains("anc")
                && (action->data() != "adaptive" || features.contains("adaptive")));
            action->setChecked(action->data() == state.value("AncMode"));
        }
    };
    QObject::connect(&client, &Client::stateChanged, &app, updateTray);
    QObject::connect(&app, &QGuiApplication::paletteChanged, &app, updateTray);
    updateTray();
    if (!app.arguments().contains("--minimized")) show();
    return app.exec();
}
