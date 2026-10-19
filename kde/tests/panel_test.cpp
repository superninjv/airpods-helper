#include "panel.h"
#include <QtTest>
#include <QQuickWindow>
#include <QTemporaryDir>
#include <QApplication>
#include <KConfig>
#include <KConfigGroup>
#include <QQmlEngine>
#include <QQmlComponent>
#include <QScreen>
#include <Plasma/Plasma>
#include <QMouseEvent>
#include <LayerShellQt/Window>

class PanelTest : public QObject
{
    Q_OBJECT
private slots:
    void placement()
    {
        const QRect screen(0, 0, 1920, 1080);
        const QRect available(0, 0, 1920, 1032);
        const QSize size(420, 680);
        QCOMPARE(panelEdge({1800, 1060}, screen), Qt::BottomEdge);
        QCOMPARE(panelEdge({900, 10}, screen), Qt::TopEdge);
        QCOMPARE(panelEdge({10, 500}, screen), Qt::LeftEdge);
        QCOMPARE(panelEdge({1910, 500}, screen), Qt::RightEdge);
        auto bottom = panelGeometry({1800, 1060}, screen, available, size);
        QVERIFY(available.contains(bottom));
        QCOMPARE(bottom.bottom(), available.bottom() - 8);
        auto top = panelGeometry({900, 10}, screen, {0, 48, 1920, 1032}, size);
        QCOMPARE(top.top(), 56);
        auto left = panelGeometry({10, 500}, screen, {48, 0, 1872, 1080}, size);
        QCOMPARE(left.left(), 56);
        auto right = panelGeometry({1910, 500}, screen, {0, 0, 1872, 1080}, size);
        QCOMPARE(right.right(), 1863);
        const QRect second(-1280, -200, 1280, 720);
        auto small = panelGeometry({-20, 510}, second, second, {420, 900});
        QVERIFY(second.contains(small));
        QCOMPARE(small.height(), 704);
    }
    void configuredTrayWinsAtScreenCorners()
    {
        QTemporaryDir temp;
        KConfig config(temp.filePath("panels"), KConfig::SimpleConfig);
        KConfigGroup containments(&config, "Containments");
        auto bottom = containments.group("2");
        bottom.writeEntry("plugin", "org.kde.panel");
        bottom.writeEntry("location", int(Plasma::Types::BottomEdge));
        bottom.group("Applets").group("7").writeEntry("plugin", "org.kde.plasma.systemtray");
        auto top = containments.group("28");
        top.writeEntry("plugin", "org.kde.panel");
        top.writeEntry("location", int(Plasma::Types::TopEdge));
        top.group("Applets").group("1").writeEntry("plugin", "org.kde.plasma.kickoff");
        const QRect screen(0, 0, 1920, 1080);
        const auto edges = trayPanelEdges(containments);
        QCOMPARE(edges, QList<Qt::Edge>{Qt::BottomEdge});
        // The click is nearer the right edge, but the tray belongs to the bottom panel.
        const QPoint corner(1910, 1030);
        QCOMPARE(panelEdge(corner, screen, edges), Qt::BottomEdge);
        const auto geometry = panelGeometry(corner, screen, {0, 0, 1920, 1000}, {420, 620}, edges);
        QCOMPARE(geometry.bottom(), 991);
        bottom.writeEntry("location", int(Plasma::Types::LeftEdge));
        QCOMPARE(panelEdge(corner, screen, trayPanelEdges(containments)), Qt::LeftEdge);
        bottom.group("Applets").group("7").deleteEntry("plugin");
        QVERIFY(trayPanelEdges(containments).isEmpty());
        QCOMPARE(panelEdge(corner, screen), Qt::RightEdge);
    }
    void nativeFrameFitsAvailableSpace()
    {
        QQmlEngine engine;
        engine.setProperty("_kirigamiTheme", QStringLiteral("Plasma"));
        QQmlComponent component(&engine);
        component.setData(R"(
            import QtQuick
            import org.kde.plasma.core as PlasmaCore
            PlasmaCore.Dialog {
                visible: false
                type: PlasmaCore.Dialog.AppletPopup
                floating: 4
                property int preferredPanelHeight: 612
                mainItem: Item { width: 420; height: 620 }
            }
        )", QUrl());
        QScopedPointer<QObject> object(component.create());
        QVERIFY2(object, qPrintable(component.errorString()));
        auto *window = qobject_cast<QQuickWindow *>(object.data());
        QVERIFY(window);
        TrayPanel panel(window);
        const auto *screen = qGuiApp->primaryScreen();
        const QPoint anchor(screen->geometry().right() - 20, screen->geometry().bottom() - 5);
        const auto *margins = window->property("margins").value<QObject *>();
        const int frameHeight = margins->property("top").toInt() + margins->property("bottom").toInt();
        const auto expected = panelGeometry(anchor, screen->geometry(), screen->availableGeometry(), QSize(420, 612 + frameHeight));
        panel.show(anchor);
        QTRY_COMPARE(window->size(), expected.size());
        QCOMPARE(window->property("location").toInt(), int(Plasma::Types::BottomEdge));
        QVERIFY(screen->availableGeometry().contains(window->geometry()));
        const int bottom = expected.bottom();
        window->setProperty("preferredPanelHeight", 672);
        QCoreApplication::processEvents();
        QCOMPARE(window->height(), expected.height());
        panel.toggle(anchor);
        panel.toggle(anchor);
        QCOMPARE(window->height(), expected.height());
        if (QGuiApplication::platformName().startsWith("wayland"))
            QCOMPARE(LayerShellQt::Window::get(window)->margins().bottom(), screen->geometry().bottom() - bottom);
        else QCOMPARE(window->geometry().bottom(), bottom);
    }
    void reservesFloatingDockWhenWaylandReportsFullScreen()
    {
        QTemporaryDir temp;
        KConfig config(temp.filePath("panels"), KConfig::SimpleConfig);
        KConfigGroup containments(&config, "Containments"), views(&config, "PlasmaViews");
        auto panel = containments.group("2");
        panel.writeEntry("plugin", "org.kde.panel");
        panel.group("Applets").group("7").writeEntry("plugin", "org.kde.plasma.systemtray");
        views.group("Panel 2").writeEntry("floating", true);
        views.group("Panel 2").writeEntry("floatingApplets", true);
        views.group("Panel 2").group("Defaults").writeEntry("thickness", 44);
        const QRect screen(-1920, 0, 1920, 1080);
        for (const auto location : {Plasma::Types::BottomEdge, Plasma::Types::TopEdge, Plasma::Types::LeftEdge, Plasma::Types::RightEdge}) {
            panel.writeEntry("location", int(location));
            const auto edge = trayPanelEdges(containments).first();
            const auto layout = trayPanelLayout(containments, views, edge);
            QVERIFY(layout.found && layout.floatingApplets);
            const int inset = layout.inset;
            QCOMPARE(inset, 52);
            const auto available = panelAvailableGeometry(screen, screen, edge, inset);
            const auto small = panelGeometry(screen.center(), screen, available, {420, 350}, {edge});
            const auto large = panelGeometry(screen.center(), screen, available, {500, 700}, {edge});
            QVERIFY(available.contains(small) && available.contains(large));
            if (edge == Qt::BottomEdge) QCOMPARE(small.bottom(), large.bottom());
            else if (edge == Qt::TopEdge) QCOMPARE(small.top(), large.top());
            else if (edge == Qt::LeftEdge) QCOMPARE(small.left(), large.left());
            else QCOMPARE(small.right(), large.right());
            QCOMPARE(panelAvailableGeometry(screen, available, edge, inset), available);
            views.group("Panel 2").writeEntry("floatingApplets", false);
            const auto attached = trayPanelLayout(containments, views, edge);
            QVERIFY(!attached.floatingApplets);
            QCOMPARE(attached.inset, 44);
            const auto attachedArea = panelAvailableGeometry(screen, screen, edge, attached.inset);
            const auto popup = panelGeometry(screen.center(), screen, attachedArea, {420, 350}, {edge}, 0);
            if (edge == Qt::BottomEdge) QCOMPARE(popup.bottom(), attachedArea.bottom());
            else if (edge == Qt::TopEdge) QCOMPARE(popup.top(), attachedArea.top());
            else if (edge == Qt::LeftEdge) QCOMPARE(popup.left(), attachedArea.left());
            else QCOMPARE(popup.right(), attachedArea.right());
            views.group("Panel 2").writeEntry("floatingApplets", true);
        }
    }
    void dismissesOutsideButKeepsChildPopups()
    {
        QQuickWindow window;
        window.setFlags(Qt::Tool | Qt::FramelessWindowHint);
        TrayPanel panel(&window);
        panel.show({300, 600});
        QTRY_VERIFY(window.isActive());
        QQuickWindow child;
        child.setTransientParent(&window);
        child.show();
        child.requestActivate();
        QTRY_VERIFY(child.isActive());
        QCoreApplication::processEvents();
        QVERIFY(window.isVisible());
        QQuickWindow outside;
        outside.show();
        outside.requestActivate();
        QTRY_VERIFY(!window.isVisible());
        panel.toggle({300, 600});
        QVERIFY(window.isVisible());
        window.setProperty("pinned", true);
        outside.requestActivate();
        QTRY_VERIFY(outside.isActive());
        QTest::qWait(150);
        QVERIFY(window.isVisible());
        // Explicit tray toggles still dismiss a pinned panel.
        panel.toggle({300, 600});
        QVERIFY(!window.isVisible());
    }
    void resizeFromFreeCorner_data()
    {
        QTest::addColumn<int>("dock");
        for (const auto edge : {Qt::BottomEdge, Qt::TopEdge, Qt::LeftEdge, Qt::RightEdge})
            QTest::newRow(qPrintable(QString::number(int(edge)))) << int(edge);
    }
    void resizeFromFreeCorner()
    {
        QFETCH(int, dock);
        QQmlEngine engine;
        QQmlComponent component(&engine);
        component.setData(R"(
            import QtQuick
            import QtQuick.Layouts
            import org.kde.plasma.core as PlasmaCore
            PlasmaCore.Dialog {
                visible: false
                type: PlasmaCore.Dialog.AppletPopup
                floating: 4
                mainItem: Item {
                    width: 420; height: 620
                    Layout.minimumWidth: 320
                    Layout.minimumHeight: 200
                }
            }
        )", QUrl());
        QScopedPointer<QObject> object(component.create());
        QVERIFY2(object, qPrintable(component.errorString()));
        auto *window = qobject_cast<QQuickWindow *>(object.data());
        TrayPanel panel(window);
        const auto *screen = qGuiApp->primaryScreen();
        auto anchor = screen->geometry().center();
        if (dock == Qt::TopEdge) anchor.setY(screen->geometry().top() + 1);
        else if (dock == Qt::BottomEdge) anchor.setY(screen->geometry().bottom() - 1);
        else if (dock == Qt::LeftEdge) anchor.setX(screen->geometry().left() + 1);
        else anchor.setX(screen->geometry().right() - 1);
        panel.show(anchor);
        QTest::qWait(60);
        const auto start = panelGeometry(anchor, screen->geometry(), screen->availableGeometry(), {420, 620});
        const bool left = dock != Qt::LeftEdge;
        const bool top = dock != Qt::TopEdge;
        const QPoint press(left ? 2 : window->width() - 3, top ? 2 : window->height() - 3);
        const QPoint delta(left ? -40 : 40, top ? -60 : 60);
        const auto send = [window](QEvent::Type type, QPoint local, Qt::MouseButton button, Qt::MouseButtons buttons) {
            // Deliberately omit global coordinates, as layer-shell cannot supply them.
            QMouseEvent event(type, local, QPointF(), button, buttons, Qt::NoModifier);
            QCoreApplication::sendEvent(window, &event);
        };
        send(QEvent::MouseButtonPress, press, Qt::LeftButton, Qt::LeftButton);
        send(QEvent::MouseMove, press + delta, Qt::NoButton, Qt::LeftButton);
        QTest::qWait(60);
        QCOMPARE(window->size(), start.size() + QSize(40, 60));
        QRect resized = start.adjusted(left ? -40 : 0, top ? -60 : 0, left ? 0 : 40, top ? 0 : 60);
        if (QGuiApplication::platformName().startsWith("wayland")) {
            const auto area = screen->geometry();
            const auto *layer = LayerShellQt::Window::get(window);
            QCOMPARE(layer->margins(), QMargins(dock == Qt::RightEdge ? 0 : resized.left() - area.left(),
                dock == Qt::BottomEdge ? 0 : resized.top() - area.top(),
                dock == Qt::RightEdge ? area.right() - resized.right() : 0,
                dock == Qt::BottomEdge ? area.bottom() - resized.bottom() : 0));
            QVERIFY(layer->anchors().testFlag(dock == Qt::BottomEdge ? LayerShellQt::Window::AnchorBottom
                : dock == Qt::TopEdge ? LayerShellQt::Window::AnchorTop
                : dock == Qt::LeftEdge ? LayerShellQt::Window::AnchorLeft : LayerShellQt::Window::AnchorRight));
        } else QCOMPARE(window->geometry(), resized);
        send(QEvent::MouseButtonRelease, press, Qt::LeftButton, Qt::NoButton);
        panel.toggle(anchor);
        panel.toggle(anchor);
        QTest::qWait(60);
        QCOMPARE(window->size(), resized.size());
        // Shrinking far past the opposite corner respects the minimum size.
        send(QEvent::MouseButtonPress, QPoint(window->width() - 3, 2), Qt::LeftButton, Qt::LeftButton);
        send(QEvent::MouseMove, QPoint(-5000, 5000), Qt::NoButton, Qt::LeftButton);
        send(QEvent::MouseButtonRelease, QPoint(2, 2), Qt::LeftButton, Qt::NoButton);
        QTest::qWait(60);
        if (dock != Qt::RightEdge) QCOMPARE(window->width(), window->minimumWidth());
        if (dock != Qt::TopEdge) QCOMPARE(window->height(), window->minimumHeight());
    }
};
int main(int argc, char **argv)
{
    QTemporaryDir config;
    qputenv("XDG_CONFIG_HOME", config.path().toUtf8());
    QApplication app(argc, argv);
    PanelTest test;
    return QTest::qExec(&test, argc, argv);
}
#include "panel_test.moc"
