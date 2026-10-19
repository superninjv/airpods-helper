#include <QApplication>
#include <QQuickStyle>
#include <QTemporaryDir>
#include <QQuickWindow>
#include <QQmlEngine>
#include <QQmlError>
#include <QtTest>
#include <Plasma/PluginLoader>
#include <Plasma/Containment>
#include <Plasma/Corona>
#include <PlasmaQuick/AppletQuickItem>
#include <KPackage/PackageLoader>
#include <algorithm>
#include "mockbackend.h"

class TestCorona : public Plasma::Corona
{
    QRect screenGeometry(uint) const override { return {0, 0, 1280, 800}; }
};

class PlasmoidTest : public QObject
{
    Q_OBJECT
private slots:
    void appearsInWidgetDiscovery()
    {
        const auto applets = KPackage::PackageLoader::self()->listPackages("Plasma/Applet");
        const auto found = std::find_if(applets.cbegin(), applets.cend(), [](const auto &metadata) {
            return metadata.pluginId() == "org.costa.airpods";
        });
        QVERIFY2(found != applets.cend(), "AirPods Helper is missing from Plasma's widget discovery list");
        QVERIFY(!found->rawData().value("X-Plasma-NotificationArea").toBool());
    }
    void loadsSharedControlsInPlasma_data()
    {
        QTest::addColumn<bool>("connected");
        QTest::newRow("buds") << true;
        QTest::newRow("disconnected") << false;
    }
    void loadsSharedControlsInPlasma()
    {
        QFETCH(bool, connected);
        MockBackend backend;
        if (!connected) {
            backend.state["Connected"] = false;
            backend.state["Features"] = QStringList{};
            backend.state["EqPreset"] = "";
            backend.state["EqStatus"] = "off";
        }
        QStringList warnings;
        TestCorona corona;
        auto shell = KPackage::PackageLoader::self()->loadPackage("Plasma/Shell");
        shell.setPath("org.kde.plasma.desktop");
        corona.setKPackage(shell);
        Plasma::Containment containment(&corona, KPluginMetaData{}, {});
        containment.setLocation(Plasma::Types::BottomEdge);
        containment.setFormFactor(Plasma::Types::Horizontal);
        containment.init();
        auto applet = Plasma::PluginLoader::self()->loadApplet("org.costa.airpods");
        QVERIFY(applet);
        QVERIFY(applet->property("backend").value<QObject *>());
        auto *raw = applet.get();
        containment.addApplet(std::move(applet));
        raw->init();
        raw->flushPendingConstraintsEvents();
        auto *item = PlasmaQuick::AppletQuickItem::itemForApplet(raw);
        QVERIFY(item);
        item->setProperty("ready", false);
        connect(qmlEngine(item), &QQmlEngine::warnings, &corona, [&](const QList<QQmlError> &errors) {
            for (const auto &error : errors) warnings << error.toString();
        });
        QQuickWindow window;
        item->setParent(&window);
        item->setParentItem(window.contentItem());
        item->setSize({48, 48});
        window.resize(64, 64);
        window.show();
        QTRY_VERIFY(item->compactRepresentationItem());
        QTRY_VERIFY(item->compactRepresentationItem()->isVisible());
        QTRY_COMPARE(item->compactRepresentationItem()->size(), QSizeF(48, 48));
        QVERIFY(QTest::qWaitForWindowExposed(&window));
        QTest::mouseClick(&window, Qt::LeftButton, Qt::NoModifier,
            item->compactRepresentationItem()->mapToScene(QPointF(24, 24)).toPoint());
        QTRY_VERIFY(item->isExpanded());
        QTRY_VERIFY(item->fullRepresentationItem());
        item->fullRepresentationItem()->setProperty("backend", QVariant::fromValue(&backend));
        item->setProperty("ready", true);
        QQuickWindow *popup = nullptr;
        for (auto *candidate : qGuiApp->topLevelWindows())
            if (candidate->objectName() == "popupWindow") popup = qobject_cast<QQuickWindow *>(candidate);
        QVERIFY(popup);
        QTRY_VERIFY(popup->isVisible());
        QTest::mouseClick(&window, Qt::LeftButton, Qt::NoModifier, QPoint(24, 24));
        QTRY_VERIFY(!item->isExpanded());
        QTRY_VERIFY(!popup->isVisible());
        window.requestActivate();
        QTRY_VERIFY(window.isActive());
        item->compactRepresentationItem()->forceActiveFocus();
        QTRY_VERIFY(item->compactRepresentationItem()->hasActiveFocus());
        QTest::keyClick(&window, Qt::Key_Space);
        QTRY_VERIFY(item->isExpanded());
        QTRY_VERIFY(popup->isVisible());
        QTRY_VERIFY(item->fullRepresentationItem());
        auto *loader = item->fullRepresentationItem();
        QTRY_VERIFY(loader->property("item").value<QObject *>());
        auto *panel = loader->property("item").value<QObject *>();
        QCOMPARE(panel->objectName(), "panel-content");
        QVERIFY(panel->findChild<QObject *>("equalizer"));
        QTRY_VERIFY(panel->property("plasmaHosted").toBool());
        auto *lastCard = panel->findChild<QQuickItem *>(connected ? "listening-card" : "equalizer");
        auto *scroll = panel->findChild<QQuickItem *>("panel-scroll");
        QVERIFY(lastCard && scroll);
        QTRY_VERIFY2(lastCard->mapToScene(QPointF(0, lastCard->height())).y()
            <= scroll->mapToScene(QPointF(0, scroll->height())).y(),
            "The initial native popup clips its default controls");
        QVERIFY(popup->grabWindow().save(QString("kde-preview-panel-%1.png").arg(QTest::currentDataTag())));
        auto *curve = panel->findChild<QQuickItem *>("eq-curve");
        QVERIFY(curve);
        QQuickItem *endpoint = nullptr;
        for (auto *child : curve->childItems())
            if (child->objectName() == "eq-frequency-20000") endpoint = child;
        QVERIFY(endpoint);
        QVERIFY(endpoint->mapRectToItem(curve, QRectF(QPointF(), endpoint->size())).right() <= curve->width() + 1);
        popup->resize(qMax(480, popup->minimumWidth()), 360);
        const auto userSize = popup->size();
        backend.state["Connected"] = !connected;
        emit backend.stateChanged();
        QTest::qWait(50);
        QCOMPARE(popup->size(), userSize);
        QVERIFY(endpoint->mapRectToItem(curve, QRectF(QPointF(), endpoint->size())).right() <= curve->width() + 1);
        QVERIFY(curve->mapToScene(QPointF(curve->width(), 0)).x()
            <= scroll->mapToScene(QPointF(scroll->width() - scroll->property("rightPadding").toReal(), 0)).x());
        QVERIFY(!panel->findChild<QQuickItem *>("panel-footer")->isVisible());
        auto *login = panel->findChild<QObject *>("setting-login");
        QVERIFY(login);
        QVERIFY(!login->property("visible").toBool());
        panel->setProperty("pinned", true);
        QVERIFY(!item->hideOnWindowDeactivate());
        panel->setProperty("pinned", false);
        QVERIFY(item->hideOnWindowDeactivate());
        QVERIFY(QMetaObject::invokeMethod(panel, "closeRequested"));
        QVERIFY(!item->isExpanded());
        QVERIFY2(warnings.isEmpty(), qPrintable(warnings.join('\n')));
    }
};

int main(int argc, char **argv)
{
    QTemporaryDir config;
    qputenv("XDG_CONFIG_HOME", config.path().toUtf8());
    qputenv("XDG_DATA_HOME", config.path().toUtf8());
    qputenv("XDG_DATA_DIRS", QByteArray(WIDGET_TEST_DATA) + ":/usr/local/share:/usr/share");
    QApplication app(argc, argv);
    QCoreApplication::addLibraryPath(QStringLiteral(WIDGET_TEST_PLUGINS));
    QQuickStyle::setStyle("org.kde.plasma.components");
    PlasmoidTest test;
    return QTest::qExec(&test, argc, argv);
}
#include "plasmoid_test.moc"
