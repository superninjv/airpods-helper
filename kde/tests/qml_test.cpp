#include <QApplication>
#include <QQmlApplicationEngine>
#include <QQmlComponent>
#include <QQmlContext>
#include <QQuickWindow>
#include <QQuickItem>
#include <QQuickPaintedItem>
#include <QPalette>
#include <QtTest>
#include "icons.h"
#include "trayicon.h"
#include "theme.h"
#include "panel.h"
#include <QScreen>
#include <QMouseEvent>
#include <Plasma/Theme>
#include <Plasma/Plasma>
#include <QTemporaryDir>
#include <QFile>
#include <QDir>
#include <QQuickStyle>
#include <QJSValue>
#include <QStandardPaths>

#include "mockbackend.h"

static QQuickItem *findItem(QQuickItem *parent, const QString &name)
{
    if (parent->objectName() == name) return parent;
    for (auto *child : parent->childItems())
        if (auto *found = findItem(child, name)) return found;
    return nullptr;
}

class TrayPreview : public QQuickPaintedItem
{
public:
    TrayPreview(const QIcon &icon, QQuickItem *parent) : QQuickPaintedItem(parent), m_icon(icon) {}
    void paint(QPainter *painter) override
    {
        m_icon.paint(painter, QRect(8, 8, 64, 64));
    }
private:
    QIcon m_icon;
};

class QmlTest : public QObject
{
    Q_OBJECT
private slots:
    void screenshots_data()
    {
        QTest::addColumn<bool>("dark");
        QTest::newRow("light") << false;
        QTest::newRow("dark") << true;
    }
    void screenshots()
    {
        QFETCH(bool, dark);
        const auto output = qEnvironmentVariable("AIRPODS_SCREENSHOTS");
        if (output.isEmpty()) QSKIP("Set AIRPODS_SCREENSHOTS to capture documentation images.");
        QVERIFY(QDir().mkpath(output));
        QApplication::setFont(QFont("Sans Serif", 10));
        QIcon::setThemeName("breeze");
        QFile scheme(QStandardPaths::locate(QStandardPaths::GenericDataLocation,
            dark ? "color-schemes/BreezeDark.colors" : "color-schemes/BreezeLight.colors"));
        QVERIFY(scheme.open(QIODevice::ReadOnly));
        QFile globals(qEnvironmentVariable("XDG_CONFIG_HOME") + "/kdeglobals");
        QVERIFY(globals.open(QIODevice::WriteOnly | QIODevice::Truncate));
        globals.write(scheme.readAll());
        globals.close();
        applyDesktopTheme();
        Plasma::Theme theme;
        theme.setThemeName(dark ? "breeze-dark" : "breeze-light");
        MockBackend backend;
        backend.devices[0] = QVariantMap{{"address", "AA:BB:CC:DD:EE:FF"}, {"name", "Alex’s AirPods"}};
        backend.state["Firmware"] = "7B21";
        backend.state["EqBackend"] = "pipewire";
        backend.state["ResumeOnInsert"] = true;
        backend.state["EqAutoLoad"] = true;
        backend.state["MicSource"] = true;
        backend.presets.removeLast();
        QQmlApplicationEngine engine;
        engine.setProperty("_kirigamiTheme", QStringLiteral("Plasma"));
        engine.addImageProvider("airpods-glyph", new PanelIcons);
        engine.rootContext()->setContextProperty("backend", &backend);
        QQmlComponent component(&engine, QUrl("qrc:/qt/qml/AirPods/qml/PanelContent.qml"));
        QQuickWindow window;
        window.setColor(qApp->palette().color(QPalette::Window));
        auto *content = qobject_cast<QQuickItem *>(component.create());
        QVERIFY(content);
        content->setParent(&window);
        content->setParentItem(window.contentItem());
        content->setProperty("plasmaHosted", true);
        content->setSize(QSizeF(480, 820));
        window.resize(480, 820);
        window.setVisible(true);
        for (const auto &page : {"controls", "devices", "settings", "equalizer"}) {
            const int height = QString(page) == "equalizer" ? 980 : QString(page) == "devices" ? 920 : 820;
            content->setHeight(height);
            window.setHeight(height);
            if (QString(page) == "equalizer") {
                QVERIFY(QMetaObject::invokeMethod(findItem(content, "equalizer"), "startEditor", Q_ARG(QVariant, false)));
            } else {
                QVERIFY(QMetaObject::invokeMethod(content, "navigate", Q_ARG(QVariant, QString(page))));
                if (QString(page) == "devices")
                    QVERIFY(QMetaObject::invokeMethod(findItem(content, "rename-AA:BB:CC:DD:EE:FF"), "clicked"));
            }
            auto *tab = findItem(content, "toggle-" + (QString(page) == "equalizer" ? QString("controls") : QString(page)));
            auto *underline = findItem(content, "tab-underline");
            QVERIFY(tab && underline);
            QTRY_VERIFY(qAbs(underline->mapToScene(QPointF()).x() - tab->mapToScene(QPointF()).x()) < 0.5);
            QTRY_VERIFY(qAbs(underline->width() - tab->width()) < 0.5);
            QTest::qWait(50);
            QVERIFY(window.grabWindow().save(output + (dark ? "/kde-dark-" : "/kde-") + page + ".png"));
        }
        QQuickWindow iconWindow;
        iconWindow.setColor(qApp->palette().color(QPalette::Window));
        auto *tray = new TrayPreview(batteryIcon(batteryLevel(backend.state), qApp->palette()), iconWindow.contentItem());
        tray->setSize(QSizeF(80, 80));
        iconWindow.resize(80, 80);
        iconWindow.show();
        QTest::qWait(50);
        QVERIFY(iconWindow.grabWindow().save(output + (dark ? "/kde-dark-tray.png" : "/kde-tray.png")));
    }
    void iconsWorkWithoutGpuTinting()
    {
        PanelIcons icons;
        QSize size;
        const auto image = icons.requestImage("off/ffffff", &size, QSize(24, 24));
        QCOMPARE(size, QSize(24, 24));
        QVERIFY(!image.isNull());
        QCOMPARE(image.pixelColor(12, 12).red(), 255);
        QVERIFY(image.pixelColor(12, 12).alpha() > 0);
        QVERIFY(icons.requestImage("off/invalid", &size, {}).isNull());
        const auto full = batteryIcon(100, qApp->palette()).pixmap(64, 64).toImage();
        const auto half = batteryIcon(50, qApp->palette()).pixmap(64, 64).toImage();
        const auto empty = batteryIcon(-1, qApp->palette()).pixmap(64, 64).toImage();
        int glyphPixels = 0;
        for (int y = 15; y < 49; ++y)
            for (int x = 15; x < 49; ++x)
                if (full.pixelColor(x, y).alpha() > 100) ++glyphPixels;
        QVERIFY(glyphPixels > 100);
        QVERIFY(full != half);
        QVERIFY(half != empty);
        const auto quarter = batteryIcon(25, qApp->palette()).pixmap(64, 64).toImage();
        QVERIFY(quarter.pixelColor(32, 60).alpha() > 200);
        QVERIFY(quarter.pixelColor(4, 32).alpha() > 200);
        QVERIFY(quarter.pixelColor(32, 4).alpha() < 100);
        QVERIFY(quarter.pixelColor(60, 32).alpha() < 100);
        QCOMPARE(full.pixelColor(60, 32).alpha(), 255);
        QVERIFY(half.pixelColor(60, 32).alpha() < full.pixelColor(60, 32).alpha());
        QVERIFY(full.save("kde-tray-full.png"));
        QVERIFY(half.save("kde-tray-half.png"));
    }
    void windowLoads_data()
    {
        QTest::addColumn<QString>("scenario");
        QTest::addColumn<bool>("dark");
        QTest::newRow("light") << QString("buds") << false;
        QTest::newRow("dark") << QString("buds") << true;
        QTest::newRow("disconnected") << QString("disconnected") << true;
        QTest::newRow("disconnected-large") << QString("disconnected-large") << true;
        QTest::newRow("headphones") << QString("headphones") << false;
        QTest::newRow("unknown") << QString("unknown") << true;
        QTest::newRow("large-font") << QString("large") << true;
    }
    void windowLoads()
    {
        QFETCH(QString, scenario);
        QFETCH(bool, dark);
        QFile globals(qEnvironmentVariable("XDG_CONFIG_HOME") + "/kdeglobals");
        QVERIFY(globals.open(QIODevice::WriteOnly | QIODevice::Truncate));
        const auto background = dark ? "#20242b" : "#eff0f1";
        const auto foreground = dark ? "#edf0f4" : "#232629";
        for (const auto &group : {"Window", "View", "Button"}) {
            globals.write(QByteArray("[Colors:") + group + "]\nBackgroundNormal=" + background + "\nForegroundNormal=" + foreground + "\n");
        }
        globals.write("[Colors:Selection]\nBackgroundNormal=#3870c0\nForegroundNormal=#ffffff\n");
        globals.write(QByteArray("[Icons]\nTheme=breeze\n[General]\nfont=Sans Serif,") + (scenario.contains("large") ? "18" : "10") + ",-1,5,400,0,0,0,0,0,0,0,0,0,0,1\n");
        globals.close();
        applyDesktopTheme();
        // Plasma's colors can intentionally differ from the application scheme.
        for (bool themeDark : {false, true}) {
            const auto name = themeDark ? "airpods-dark" : "airpods-light";
            const auto path = qEnvironmentVariable("XDG_DATA_HOME") + "/plasma/desktoptheme/" + name;
            QVERIFY(QDir().mkpath(path));
            QFile metadata(path + "/metadata.json");
            QVERIFY(metadata.open(QIODevice::WriteOnly));
            metadata.write(QByteArray("{\"KPlugin\":{\"Id\":\"") + name + "\",\"Name\":\"Panel test\"},\"X-Plasma-API\":\"5.0\"}");
            metadata.close();
            QFile colors(path + "/colors");
            QVERIFY(colors.open(QIODevice::WriteOnly));
            for (const auto &group : {"Window", "View", "Button", "Tooltip"}) {
                colors.write(QByteArray("[Colors:") + group + "]\nBackgroundNormal=" + (themeDark ? "#283242" : "#dce8f5") + "\nForegroundNormal=" + (themeDark ? "#f7e6d5" : "#263a50") + "\n");
            }
            colors.write("[Colors:Selection]\nBackgroundNormal=#3870c0\nForegroundNormal=#ffffff\n");
        }
        QFile plasmaConfig(qEnvironmentVariable("XDG_CONFIG_HOME") + "/plasmarc");
        QVERIFY(plasmaConfig.open(QIODevice::WriteOnly));
        plasmaConfig.write(QByteArray("[Theme]\nname=") + (dark ? "airpods-dark" : "airpods-light") + "\n");
        plasmaConfig.close();
        Plasma::Theme plasmaTheme;
        plasmaTheme.setThemeName(dark ? "airpods-dark" : "airpods-light");
        MockBackend backend;
        const bool disconnected = scenario.startsWith("disconnected");
        if (disconnected) {
            backend.state["Connected"] = false;
            backend.state["Features"] = QStringList{};
        }
        if (scenario == "headphones") {
            backend.state["Features"] = QStringList{"anc", "headphones"};
            backend.state["ModelName"] = "AirPods Max";
        }
        if (scenario == "unknown") backend.state["BatteryLeft"] = -1;
        QQmlApplicationEngine engine;
        engine.setProperty("_kirigamiTheme", QStringLiteral("Plasma"));
        engine.addImageProvider("airpods-glyph", new PanelIcons);
        QStringList warnings;
        connect(&engine, &QQmlEngine::warnings, this, [&](const QList<QQmlError> &errors) {
            for (const auto &error : errors) warnings << error.toString();
        });
        engine.rootContext()->setContextProperty("backend", &backend);
        engine.load(QUrl("qrc:/qt/qml/AirPods/qml/Main.qml"));
        QVERIFY2(!engine.rootObjects().isEmpty(), qPrintable(warnings.join('\n')));
        auto *window = qobject_cast<QQuickWindow *>(engine.rootObjects().first());
        QVERIFY(window);
        TrayPanel panel(window);
        const auto screen = qGuiApp->primaryScreen()->geometry();
        panel.show(QPoint(screen.center().x(), screen.bottom() - 1));
        QTest::qWait(150);
        QVERIFY2(warnings.isEmpty(), qPrintable(warnings.join('\n')));
        auto *panelError = findItem(window->contentItem(), "panel-error");
        QVERIFY(panelError && !panelError->isVisible());
        auto *panelContent = findItem(window->contentItem(), "panel-content");
        QVERIFY(panelContent);
        panelContent->setProperty("previewErrors", true);
        QTRY_VERIFY(panelError->isVisible());
        QCOMPARE(panelError->property("text").toString(), "Error preview: Could not connect to your AirPods. This is a test; nothing was changed.");
        QVERIFY(panelError->property("showCloseButton").toBool());
        panelContent->setProperty("previewErrors", false);
        QTRY_VERIFY(!panelError->isVisible());
        auto *title = findItem(window->contentItem(), "panel-title");
        QVERIFY(title);
        QCOMPARE(title->property("text").toString(), disconnected ? "AirPods Helper" : "Test AirPods");
        auto *modelLabel = findItem(window->contentItem(), "panel-model");
        QVERIFY(modelLabel);
        QCOMPARE(modelLabel->isVisible(), !disconnected);
        if (!disconnected) {
            QCOMPARE(modelLabel->property("text").toString(), backend.state.value("ModelName").toString());
            QVERIFY(modelLabel->property("font").value<QFont>().pointSizeF() < title->property("font").value<QFont>().pointSizeF());
        }
        QCOMPARE(title->property("color").value<QColor>(), QColor(dark ? "#f7e6d5" : "#263a50"));
        QCOMPARE(title->property("color").value<QColor>(), plasmaTheme.color(Plasma::Theme::TextColor));
        QVERIFY(title->property("color").value<QColor>() != qApp->palette().color(QPalette::WindowText));
        auto *panelIcon = findItem(window->contentItem(), "panel-icon");
        QVERIFY(panelIcon);
        QVERIFY(panelIcon->width() >= 40 && panelIcon->height() >= 40);
        QVERIFY(panelIcon->property("source").toUrl().toString().endsWith(title->property("color").value<QColor>().name().mid(1)));
        if (!disconnected) {
            QCOMPARE(findItem(window->contentItem(), "connection-dot")->property("color").value<QColor>(), batteryColor(100));
            auto *mic = findItem(window->contentItem(), "mic-status");
            auto *micLabel = findItem(mic, "mic-status-label");
            auto *micValue = findItem(mic, "mic-status-value");
            auto *option = findItem(window->contentItem(), "volume-swipe");
            QVERIFY(mic && micLabel && micValue && option);
            QCOMPARE(mic->property("leftPadding"), option->property("leftPadding"));
            QCOMPARE(mic->height(), option->implicitHeight());
            QCOMPARE(micValue->property("text").toString(), "Ready");
            QVERIFY(micValue->x() >= micLabel->x() + micLabel->width());
            QCOMPARE(micLabel->property("font").value<QFont>(), option->property("font").value<QFont>());
            backend.state["MicStatus"] = "error";
            backend.state["MicError"] = "Test microphone error";
            emit backend.stateChanged();
            QTRY_COMPARE(micValue->property("text").toString(), "Error");
            auto *micError = findItem(mic, "mic-status-error");
            QVERIFY(micError->isVisible());
            QCOMPARE(micError->property("text").toString(), "Test microphone error");
            backend.state["MicStatus"] = "off";
            emit backend.stateChanged();
            QTRY_VERIFY(!mic->isVisible());
            backend.state["MicStatus"] = "idle";
            backend.state["MicError"] = "";
            emit backend.stateChanged();
            auto *charge = findItem(window->contentItem(), "charge-Left");
            QVERIFY(charge);
            QCOMPARE(charge->property("text").toString(), scenario == "unknown" ? "—" : "82%");
            if (scenario == "headphones") QVERIFY(!findItem(window->contentItem(), "charge-Case"));
            else {
                for (const auto &part : {"Left", "Right"}) {
                    auto *name = findItem(window->contentItem(), QString("battery-name-") + part);
                    auto *ear = findItem(window->contentItem(), QString("in-ear-") + part);
                    QVERIFY(name && ear && ear->isVisible());
                    QCOMPARE(ear->parentItem(), name->parentItem());
                    QVERIFY(ear->x() >= name->x() + name->width());
                    if (backend.state.value(QString("Battery") + part).toInt() >= 0) {
                        auto *tile = findItem(window->contentItem(), QString("battery-tile-") + part);
                        auto *bar = findItem(tile, QString("battery-bar-") + part);
                        QVERIFY(tile->height() - bar->mapToItem(tile, QPointF(0, bar->height())).y() <= 11);
                    }
                }
                backend.state["EarLeft"] = false;
                emit backend.stateChanged();
                QTRY_VERIFY(!findItem(window->contentItem(), "in-ear-Left")->isVisible());
                backend.state["EarLeft"] = true;
                emit backend.stateChanged();
                auto *bolt = findItem(window->contentItem(), "charging-Case");
                auto *caseCharge = findItem(window->contentItem(), "charge-Case");
                QVERIFY(bolt && bolt->isVisible());
                QCOMPARE(bolt->property("paintedWidth").toReal(), bolt->property("paintedHeight").toReal());
                QVERIFY(bolt->property("source").toUrl().toString().endsWith("30d158"));
                QVERIFY(bolt->x() >= caseCharge->x() + caseCharge->width());
                QVERIFY(!findItem(window->contentItem(), "charging-Left")->isVisible());
                for (const auto &part : {"Left", "Right", "Case"}) {
                    auto *fill = findItem(window->contentItem(), QString("battery-fill-") + part);
                    QVERIFY(fill);
                    QCOMPARE(fill->property("color").value<QColor>(), batteryColor(backend.state.value(QString("Battery") + part).toInt()));
                }
                backend.state["BatteryCase"] = 20;
                emit backend.stateChanged();
                QTRY_COMPARE(findItem(window->contentItem(), "battery-fill-Case")->property("color").value<QColor>(), batteryColor(20));
                backend.state["BatteryCase"] = 43;
                emit backend.stateChanged();
            }
        }
        QVERIFY(window->flags().testFlag(Qt::FramelessWindowHint));
        QCOMPARE(window->property("backgroundHints").toInt(), 1);
        QCOMPARE(window->property("location").toInt(), int(Plasma::Types::BottomEdge));
        QVERIFY(findItem(window->contentItem(), "anc-noise")->property("checked").toBool());
        QVERIFY(findItem(window->contentItem(), "eq-bass")->property("checked").toBool());
        QCOMPARE(window->property("page").toString(), "controls");
        auto *scrollView = findItem(window->contentItem(), "panel-scroll");
        auto *tabs = findItem(window->contentItem(), "panel-tabs");
        QVERIFY(tabs);
        auto *content = findItem(window->contentItem(), "panel-content");
        QVERIFY(qAbs(tabs->width() - content->width()) <= 2);
        auto *divider = findItem(window->contentItem(), "tab-divider");
        auto *underline = findItem(window->contentItem(), "tab-underline");
        QVERIFY(divider && underline);
        QCOMPARE(divider->width(), content->width());
        QCOMPARE(divider->height(), 1);
        QCOMPARE(underline->height(), 2);
        QCOMPARE(divider->mapToScene(QPointF(0, divider->height())).y(), underline->mapToScene(QPointF(0, underline->height())).y());
        for (const auto &name : {"toggle-controls", "toggle-devices", "toggle-settings"}) {
            auto *tab = findItem(window->contentItem(), name);
            QVERIFY(tab);
            QVERIFY(qAbs(tab->width() * 3 - tabs->width()) <= 2);
            QCOMPARE(tab->property("display").toInt(), 0); // IconOnly
            QVERIFY(!engine.newQObject(tab).property("icon").property("name").toString().isEmpty());
        }
        auto *lastCard = findItem(window->contentItem(), disconnected ? "equalizer" : "listening-card");
        const auto bottom = lastCard->mapToScene(QPointF(0, lastCard->height())).y();
        const auto viewportBottom = scrollView->mapToScene(QPointF(0, scrollView->height())).y();
        if (bottom > viewportBottom && window->property("preferredPanelHeight").toInt() > window->height()) {
            // Large fonts plus the error banner may exceed the screen; the content must remain reachable.
            auto *flickable = scrollView->property("contentItem").value<QQuickItem *>();
            QVERIFY(flickable);
            const auto bannerTop = panelError->mapToScene(QPointF()).y();
            flickable->setProperty("contentY", bottom - viewportBottom);
            QTRY_VERIFY(lastCard->mapToScene(QPointF(0, lastCard->height())).y() <= viewportBottom + 1);
            QCOMPARE(panelError->mapToScene(QPointF()).y(), bannerTop);
            flickable->setProperty("contentY", 0);
        } else {
            QVERIFY2(bottom <= viewportBottom,
            qPrintable(QString("Default size clips the final card: bottom %1, viewport %2, size %3, preferred %4")
                .arg(bottom).arg(scrollView->mapToScene(QPointF(0, scrollView->height())).y())
                .arg(window->height()).arg(window->property("preferredPanelHeight").toInt())));
        }
        QVERIFY(!findItem(window->contentItem(), "eq-flat"));
        if (!scenario.contains("large")) {
            const auto row = findItem(window->contentItem(), "eq-")->y();
            for (const auto &id : {"airpods-pro-crinacle", "bass", "vocal"})
                QCOMPARE(findItem(window->contentItem(), QString("eq-") + id)->y(), row);
        }
        auto *curve = findItem(window->contentItem(), "eq-curve");
        QVERIFY(curve);
        QVERIFY(curve->mapToScene(QPointF(curve->width(), 0)).x()
            <= scrollView->mapToScene(QPointF(scrollView->width() - scrollView->property("rightPadding").toReal(), 0)).x());
        const auto checkChart = [curve] {
            for (const auto &name : {"eq-gain--12", "eq-gain-0", "eq-gain-12", "eq-frequency-20", "eq-frequency-100", "eq-frequency-1000", "eq-frequency-10000", "eq-frequency-20000"}) {
                auto *label = findItem(curve, name);
                QVERIFY(label);
                QVERIFY2(QRectF(QPointF(), curve->size()).adjusted(-1, -1, 1, 1).contains(
                    label->mapRectToItem(curve, QRectF(QPointF(), label->size()))), name);
                QVERIFY2(label->property("paintedWidth").toReal() <= label->width() + 1, name);
            }
            auto *ten = findItem(curve, "eq-frequency-10000");
            auto *twenty = findItem(curve, "eq-frequency-20000");
            const auto polygon = [curve](QQuickItem *label) {
                QPolygonF result;
                for (const auto &point : {QPointF(), QPointF(label->width(), 0), QPointF(label->width(), label->height()), QPointF(0, label->height())})
                    result << label->mapToItem(curve, point);
                return result;
            };
            QVERIFY(polygon(ten).intersected(polygon(twenty)).isEmpty());
            QCOMPARE(ten->y(), twenty->y());
            QCOMPARE(ten->rotation(), qreal(60));
            auto *plot = findItem(curve, "eq-plot");
            QVERIFY(plot);
            QCOMPARE(plot->property("canvasSize").toSizeF(), plot->size());
            QVERIFY(QRectF(QPointF(), curve->size()).contains(QRectF(plot->position(), plot->size())));
        };
        checkChart();
        for (const auto &name : {"toggle-controls", "toggle-devices", "toggle-settings", "pin-panel"}) {
            auto *button = findItem(window->contentItem(), name);
            QVERIFY(button);
            QVERIFY(button->mapToScene(QPointF(button->width(), button->height())).y() <= scrollView->mapToScene(QPointF()).y());
        }
        auto *pin = findItem(window->contentItem(), "pin-panel");
        QVERIFY(!window->property("pinned").toBool());
        const auto clickPin = [window, pin] {
            QTest::mouseClick(window, Qt::LeftButton, Qt::NoModifier,
                pin->mapToScene(QPointF(pin->width() / 2, pin->height() / 2)).toPoint());
        };
        QCOMPARE(engine.newQObject(pin).property("icon").property("name").toString(), "window-pin");
        clickPin();
        QVERIFY(window->property("pinned").toBool());
        QCOMPARE(engine.newQObject(pin).property("icon").property("name").toString(), "window-unpin");
        clickPin();
        QVERIFY(!window->property("pinned").toBool());
        for (const auto &name : {"refresh", "pin"}) {
            auto *tip = window->findChild<QObject *>(QString("%1-tooltip").arg(name));
            auto *button = findItem(window->contentItem(), QString("%1-panel").arg(name));
            QVERIFY(tip && button);
            if (QString(name) == "pin") QCOMPARE(tip->property("text").toString(), "Keep Open");
            tip->setProperty("delay", 0);
            tip->setProperty("visible", true);
            QTRY_VERIFY(tip->property("opened").toBool());
            auto *tipContent = tip->property("contentItem").value<QQuickItem *>();
            QVERIFY(tipContent);
            const auto tipTop = tipContent->mapToScene(QPointF());
            const auto tipBottom = tipContent->mapToScene(QPointF(tipContent->width(), tipContent->height()));
            QVERIFY2(tipTop.y() >= button->mapToScene(QPointF(0, button->height())).y(),
                qPrintable(QString("%1: tip top %2, button bottom %3, y %4, type %5").arg(name).arg(tipTop.y())
                    .arg(button->mapToScene(QPointF(0, button->height())).y()).arg(tip->property("y").toReal()).arg(tip->metaObject()->className())));
            QVERIFY(QRectF(QPointF(), window->size()).contains(QRectF(tipTop, tipBottom)));
            tip->setProperty("visible", false);
        }
        QHash<int, qreal> iconRows;
        for (const auto &mode : {"off", "noise", "transparency", "adaptive"}) {
            auto *button = findItem(window->contentItem(), QString("anc-%1").arg(mode));
            if (!button || !button->isVisible()) continue;
            auto *label = findItem(button, QString("anc-label-%1").arg(mode));
            auto *icon = findItem(button, QString("anc-icon-%1").arg(mode));
            QVERIFY(label && icon);
            const auto labelBottom = label->mapToItem(button, QPointF(0, label->height())).y();
            const auto iconBottom = icon->mapToItem(button, QPointF(0, icon->height())).y();
            QVERIFY2(labelBottom <= button->height(), mode);
            QVERIFY2(iconBottom <= label->mapToItem(button, QPointF()).y(), mode);
            QVERIFY2(label->property("paintedHeight").toReal() <= label->height() + 1, mode);
            QVERIFY2(!label->property("truncated").toBool(), mode);
            QCOMPARE(label->property("lineCount").toInt(), 1);
            QCOMPARE(label->property("font").value<QFont>().pointSizeF(),
                findItem(window->contentItem(), "anc-label-off")->property("font").value<QFont>().pointSizeF());
            QVERIFY2(label->property("paintedWidth").toReal() <= label->width() + 1,
                qPrintable(QString("%1: painted %2, available %3, font %4, minimum %5").arg(mode)
                    .arg(label->property("paintedWidth").toReal()).arg(label->width())
                    .arg(label->property("font").value<QFont>().toString()).arg(label->property("minimumPixelSize").toInt())));
            QVERIFY(icon->width() >= 48 && icon->height() >= 48);
            const int row = qRound(button->mapToScene(QPointF()).y());
            const auto iconY = icon->mapToScene(QPointF()).y();
            if (iconRows.contains(row)) QCOMPARE(iconY, iconRows.value(row));
            else iconRows.insert(row, iconY);
            if (!button->property("checked").toBool())
                QCOMPARE(label->property("color"), findItem(window->contentItem(), "anc-label-off")->property("color"));
        }
        const auto image = window->grabWindow();
        QVERIFY(!image.isNull());
        QVERIFY(image.pixelColor(0, 0).alpha() < 20);
        QVERIFY(image.pixelColor(image.width() - 1, 0).alpha() < 20);
        QVERIFY(image.save(QString("kde-preview-%1.png").arg(QTest::currentDataTag())));
        if (!disconnected) {
            auto *selected = findItem(window->contentItem(), "anc-noise");
            const auto fill = findItem(selected, "anc-background-noise")->property("color").value<QColor>();
            // The sides of a tall selected tile must match its fill without repeated gradient seams.
            for (int y : {int(selected->height() / 3), int(selected->height() / 2), int(selected->height() * 2 / 3)}) {
                for (int x : {2, int(selected->width()) - 3}) {
                    const auto pixel = selected->mapToScene(QPointF(x, y)) * image.devicePixelRatio();
                    QCOMPARE(image.pixelColor(pixel.toPoint()), fill);
                }
            }
        }
        if (scenario == "buds" && !dark) {
            auto *mode = findItem(window->contentItem(), "anc-transparency");
            QTest::mouseClick(window, Qt::LeftButton, Qt::NoModifier,
                mode->mapToScene(QPointF(mode->width() / 2, mode->height() / 2)).toPoint());
            QCOMPARE(backend.lastMethod, "SetAncMode");
            QCOMPARE(backend.lastValue.toString(), "transparency");
            QVERIFY(!mode->property("checked").toBool());
            const auto connectedSize = window->size();
            const auto features = backend.state["Features"];
            backend.state["Connected"] = false;
            backend.state["Features"] = QStringList{};
            emit backend.stateChanged();
            QCoreApplication::processEvents();
            QCOMPARE(window->size(), connectedSize);
            backend.state["Connected"] = true;
            backend.state["Features"] = features;
            emit backend.stateChanged();
            QTRY_COMPARE(window->size(), connectedSize);
        }
        const auto click = [window](QQuickItem *item) {
            QTest::mouseClick(window, Qt::LeftButton, Qt::NoModifier,
                item->mapToScene(QPointF(item->width() / 2, item->height() / 2)).toPoint());
        };
        click(findItem(window->contentItem(), "toggle-settings"));
        QTRY_COMPARE(window->property("page").toString(), "settings");
        QTest::qWait(60);
        auto *settingsPage = findItem(window->contentItem(), "settings-page");
        QVERIFY(settingsPage->isVisible());
        auto *preferredCard = findItem(window->contentItem(), "preferred-device-card");
        QVERIFY(preferredCard && !preferredCard->isVisible());
        const auto settingsTop = settingsPage->mapToScene(QPointF()).y();
        QVERIFY(settingsTop >= scrollView->mapToScene(QPointF()).y());
        QVERIFY(settingsTop < scrollView->mapToScene(QPointF()).y() + 50);
        auto *setting = findItem(window->contentItem(), "setting-AutoReconnect");
        click(setting);
        QCOMPARE(backend.lastMethod, "AutoReconnect");
        QCOMPARE(backend.lastValue.toBool(), false);
        QVERIFY(setting->property("checked").toBool());
        QVERIFY(window->grabWindow().save(QString("kde-settings-%1.png").arg(QTest::currentDataTag())));
        emit backend.error("Test connection failure");
        QTRY_VERIFY(panelError->isVisible());
        QCOMPARE(panelError->property("text").toString(), "Test connection failure");
        QVERIFY(panelError->mapToScene(QPointF(0, panelError->height())).y() <= scrollView->mapToScene(QPointF()).y());
        panelError->setVisible(false);
        emit backend.error("Test connection failure");
        QVERIFY(panelError->isVisible());
        QCOMPARE(panelError->property("text").toString(), "Test connection failure");
        panelError->setVisible(false);
        click(findItem(window->contentItem(), "toggle-devices"));
        QTRY_COMPARE(window->property("page").toString(), "devices");
        QTest::qWait(60);
        auto *devicesPage = findItem(window->contentItem(), "devices-page");
        QVERIFY(devicesPage->isVisible());
        QVERIFY(preferredCard->isVisible());
        auto *picker = findItem(preferredCard, "preferred-picker");
        QVERIFY(picker);
        picker->setProperty("currentIndex", 0);
        QVERIFY(QMetaObject::invokeMethod(picker, "activated", Q_ARG(int, 0)));
        QCOMPARE(backend.lastMethod, "PreferredDevice");
        QVERIFY(backend.lastValue.toString().isEmpty());
        auto *preferredName = findItem(preferredCard, "preferred-name");
        auto *preferredAddress = findItem(preferredCard, "preferred-address");
        QVERIFY(preferredName && preferredAddress);
        QCOMPARE(preferredName->property("text").toString(), "Test AirPods");
        QCOMPARE(preferredAddress->property("text").toString(), "AA:BB:CC:DD:EE:FF");
        QVERIFY(preferredName->property("font").value<QFont>().pointSizeF() > preferredAddress->property("font").value<QFont>().pointSizeF());
        auto *badge = findItem(window->contentItem(), "device-preferred-AA:BB:CC:DD:EE:FF");
        QVERIFY(badge && badge->isVisible());
        auto *pairedName = findItem(window->contentItem(), "device-name-AA:BB:CC:DD:EE:FF");
        QVERIFY(pairedName);
        QVERIFY(badge->x() - pairedName->x() - pairedName->width() <= 10);
        auto *rename = findItem(window->contentItem(), "rename-AA:BB:CC:DD:EE:FF");
        QVERIFY(rename && QMetaObject::invokeMethod(rename, "clicked"));
        auto *renameName = findItem(window->contentItem(), "rename-name-AA:BB:CC:DD:EE:FF");
        QVERIFY(renameName && renameName->isVisible());
        renameName->setProperty("text", "My AirPods");
        backend.failRename = true;
        QVERIFY(QMetaObject::invokeMethod(findItem(window->contentItem(), "rename-save-AA:BB:CC:DD:EE:FF"), "clicked"));
        QVERIFY(renameName->isVisible());
        QCOMPARE(renameName->property("text").toString(), "My AirPods");
        backend.failRename = false;
        QVERIFY(QMetaObject::invokeMethod(findItem(window->contentItem(), "rename-save-AA:BB:CC:DD:EE:FF"), "clicked"));
        QCOMPARE(backend.lastMethod, "RenameDevice");
        QCOMPARE(backend.lastValue.toMap().value("name").toString(), "My AirPods");
        QTRY_COMPARE(preferredName->property("text").toString(), "My AirPods");
        if (!disconnected) QTRY_COMPARE(title->property("text").toString(), "My AirPods");
        auto *preferred = findItem(preferredCard, "preferred-device");
        QVERIFY(preferred);
        preferred->setProperty("text", "11:22:33:44:55:66");
        QVERIFY(QMetaObject::invokeMethod(preferred, "editingFinished"));
        QCOMPARE(backend.lastMethod, "PreferredDevice");
        QCOMPARE(backend.lastValue.toString(), "11:22:33:44:55:66");
        const auto devicesTop = devicesPage->mapToScene(QPointF()).y();
        QVERIFY(devicesTop >= scrollView->mapToScene(QPointF()).y());
        QVERIFY(devicesTop < scrollView->mapToScene(QPointF()).y() + 50);
        QVERIFY(!settingsPage->isVisible());
        QVERIFY(window->grabWindow().save(QString("kde-devices-%1.png").arg(QTest::currentDataTag())));
        click(findItem(window->contentItem(), "toggle-controls"));
        QTest::qWait(60);
        if (scenario == "buds" && !dark) {
            auto *eq = findItem(window->contentItem(), "equalizer");
            QVERIFY(QMetaObject::invokeMethod(eq, "startEditor", Q_ARG(QVariant, true)));
            QTRY_COMPARE(window->property("page").toString(), "equalizer");
            QTest::qWait(100);
            QCOMPARE(eq->property("editing").toBool(), true);
            auto *name = findItem(window->contentItem(), "eq-name");
            QVERIFY(name->isVisible());
            name->setProperty("text", "Bass Boost");
            QVERIFY(QMetaObject::invokeMethod(findItem(window->contentItem(), "eq-add-band"), "clicked"));
            QCOMPARE(eq->property("preview").value<QJSValue>().property("bands").property("length").toInt(), 1);
            QVERIFY(QMetaObject::invokeMethod(eq, "save"));
            QCOMPARE(backend.saved.value("id").toString(), "bass-boost");
            QCOMPARE(backend.saved.value("bands").toList().size(), 1);
            QVERIFY(backend.activated);
            // A failed save keeps the draft, and navigation must not discard it.
            emit backend.error("Test save failure");
            QVERIFY(eq->property("editing").toBool());
            click(findItem(window->contentItem(), "toggle-settings"));
            click(findItem(window->contentItem(), "toggle-controls"));
            QCOMPARE(window->property("page").toString(), "equalizer");
            QCOMPARE(name->property("text").toString(), "Bass Boost");
            QVERIFY(window->grabWindow().save("kde-eq-editor.png"));
            emit backend.eqSaved("bass-boost");
            QCOMPARE(window->property("page").toString(), "controls");
            QVERIFY(!eq->property("editing").toBool());
            // Built-in customization copies the bands and cannot overwrite its ID.
            QVERIFY(QMetaObject::invokeMethod(eq, "startEditor", Q_ARG(QVariant, false)));
            QVERIFY(eq->property("newPreset").toBool());
            QVERIFY(eq->property("draftId").toString().isEmpty());
            QCOMPARE(name->property("text").toString(), "Bass Boost copy");
            QCOMPARE(eq->property("preview").value<QJSValue>().property("bands").property("length").toInt(), 1);
            name->setProperty("text", "Bass");
            QVERIFY(QMetaObject::invokeMethod(eq, "save"));
            QCOMPARE(backend.saved.value("id").toString(), "bass-2");
            emit backend.eqSaved("bass-2");
            // Deletion requires confirmation and is only available for user presets.
            backend.state["EqPreset"] = "user";
            backend.eqDetail["id"] = "user";
            emit backend.stateChanged();
            emit backend.eqDetailChanged();
            QVERIFY(QMetaObject::invokeMethod(eq, "startEditor", Q_ARG(QVariant, false)));
            QVERIFY(!eq->property("newPreset").toBool());
            QCOMPARE(eq->property("draftId").toString(), "user");
            auto *dialog = window->findChild<QObject *>("eq-delete-dialog");
            QVERIFY(dialog);
            QVERIFY(QMetaObject::invokeMethod(findItem(window->contentItem(), "eq-delete"), "clicked"));
            QTRY_VERIFY(dialog->property("visible").toBool());
            QVERIFY(QMetaObject::invokeMethod(dialog, "reject"));
            QVERIFY(backend.lastMethod != "DeleteEqPreset");
            QVERIFY(eq->property("editing").toBool());
            QVERIFY(QMetaObject::invokeMethod(findItem(window->contentItem(), "eq-delete"), "clicked"));
            QVERIFY(QMetaObject::invokeMethod(dialog, "accept"));
            QCOMPARE(backend.lastMethod, "DeleteEqPreset");
            QCOMPARE(backend.lastValue.toString(), "user");
            QTRY_VERIFY(!dialog->property("visible").toBool());
            emit backend.eqDeleted("user");
            QVERIFY(!eq->property("editing").toBool());
        }
        QVERIFY2(warnings.isEmpty(), qPrintable(warnings.join('\n')));
        if (scenario == "buds" && !dark) {
            plasmaTheme.setThemeName("airpods-dark");
            QTRY_COMPARE(title->property("color").value<QColor>(), QColor("#f7e6d5"));
            QTRY_VERIFY(panelIcon->property("source").toUrl().toString().endsWith("f7e6d5"));
            const QPoint edge(window->width() - 2, window->height() / 2);
            const int shrink = window->width() - window->minimumWidth();
            const auto sendMouse = [window](QEvent::Type type, QPoint pos, Qt::MouseButton button, Qt::MouseButtons buttons) {
                QMouseEvent event(type, pos, QPointF(), button, buttons, Qt::NoModifier);
                QCoreApplication::sendEvent(window, &event);
            };
            sendMouse(QEvent::MouseButtonPress, edge, Qt::LeftButton, Qt::LeftButton);
            sendMouse(QEvent::MouseMove, edge - QPoint(shrink, 0), Qt::NoButton, Qt::LeftButton);
            sendMouse(QEvent::MouseButtonRelease, edge - QPoint(shrink, 0), Qt::LeftButton, Qt::NoButton);
            QTRY_COMPARE(window->width(), window->minimumWidth());
            QTest::qWait(60);
            QVERIFY(curve->mapToScene(QPointF(curve->width(), 0)).x()
                <= scrollView->mapToScene(QPointF(scrollView->width() - scrollView->property("rightPadding").toReal(), 0)).x());
            checkChart();
            for (const auto &mode : {"off", "noise", "transparency", "adaptive"}) {
                auto *label = findItem(window->contentItem(), QString("anc-label-%1").arg(mode));
                QCOMPARE(label->property("lineCount").toInt(), 1);
                QCOMPARE(label->property("font").value<QFont>().pointSizeF(),
                    findItem(window->contentItem(), "anc-label-off")->property("font").value<QFont>().pointSizeF());
                QVERIFY2(label->property("paintedWidth").toReal() <= label->width() + 1, mode);
            }
        }
        if (!disconnected) {
            for (const auto &part : {"Left", "Right", "Case"}) {
                backend.state[QString("Battery") + part] = 100;
                backend.state[QString("Charging") + part] = true;
            }
            emit backend.stateChanged();
            window->setWidth(window->minimumWidth());
            QTest::qWait(60);
            for (const auto &part : {"Left", "Right", "Case"}) {
                auto *tile = findItem(window->contentItem(), QString("battery-tile-") + part);
                if (!tile) continue;
                for (const auto &prefix : {"charge-", "charging-", "battery-bar-", "battery-fill-"}) {
                    auto *item = findItem(tile, QString(prefix) + part);
                    QVERIFY(item);
                    QVERIFY2(QRectF(QPointF(), tile->size()).contains(item->mapRectToItem(tile, QRectF(QPointF(), item->size()))),
                        qPrintable(QString(prefix) + part));
                }
                auto *bar = findItem(tile, QString("battery-bar-") + part);
                auto *background = bar->property("background").value<QQuickItem *>();
                QCOMPARE(engine.newQObject(background).property("border").property("width").toInt(), 0);
                auto *fill = findItem(tile, QString("battery-fill-") + part);
                QCOMPARE(background->width(), bar->width());
                QCOMPARE(fill->width(), background->width());
                const auto trackWidth = background->width();
                backend.state[QString("Battery") + part] = 50;
                emit backend.stateChanged();
                QTRY_COMPARE(fill->width(), trackWidth / 2);
                QCOMPARE(background->width(), trackWidth);
            }
            checkChart();
        }
        QTest::keyClick(window, Qt::Key_Escape);
        QTRY_VERIFY(!window->isVisible());
    }
};
int main(int argc, char **argv)
{
    QTemporaryDir config;
    qputenv("XDG_CONFIG_HOME", config.path().toUtf8());
    QTemporaryDir data;
    qputenv("XDG_DATA_HOME", data.path().toUtf8());
    qputenv("XDG_CONFIG_DIRS", "/etc/xdg");
    QApplication app(argc, argv);
    app.setApplicationVersion("0.3.0");
    QQuickStyle::setStyle("org.kde.plasma.components");
    QmlTest test;
    return QTest::qExec(&test, argc, argv);
}
#include "qml_test.moc"
