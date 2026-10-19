#include "panel.h"

#include <Plasma/Plasma>
#include <KConfigGroup>
#include <KSharedConfig>
#include <LayerShellQt/Window>
#include <QCursor>
#include <QGuiApplication>
#include <QQuickWindow>
#include <QQuickItem>
#include <QScreen>
#include <QTimer>
#include <QMouseEvent>

static bool hasSystemTray(const KConfigGroup &panel)
{
    if (panel.readEntry("plugin", QString{}) != "org.kde.panel") return false;
    const auto applets = panel.group("Applets");
    for (const auto &applet : applets.groupList())
        if (applets.group(applet).readEntry("plugin", QString{}) == "org.kde.plasma.systemtray") return true;
    return false;
}

QList<Qt::Edge> trayPanelEdges(const KConfigGroup &containments)
{
    QList<Qt::Edge> edges;
    for (const auto &id : containments.groupList()) {
        const auto panel = containments.group(id);
        if (!hasSystemTray(panel)) continue;
        Qt::Edge edge;
        switch (panel.readEntry("location", 0)) {
        case Plasma::Types::BottomEdge: edge = Qt::BottomEdge; break;
        case Plasma::Types::TopEdge: edge = Qt::TopEdge; break;
        case Plasma::Types::LeftEdge: edge = Qt::LeftEdge; break;
        case Plasma::Types::RightEdge: edge = Qt::RightEdge; break;
        default: continue;
        }
        if (!edges.contains(edge)) edges.append(edge);
    }
    return edges;
}

TrayPanelLayout trayPanelLayout(const KConfigGroup &containments, const KConfigGroup &views, Qt::Edge edge)
{
    const auto location = edge == Qt::BottomEdge ? Plasma::Types::BottomEdge
        : edge == Qt::TopEdge ? Plasma::Types::TopEdge
        : edge == Qt::LeftEdge ? Plasma::Types::LeftEdge : Plasma::Types::RightEdge;
    TrayPanelLayout layout;
    for (const auto &id : containments.groupList()) {
        const auto panel = containments.group(id);
        if (!hasSystemTray(panel) || panel.readEntry("location", 0) != location) continue;
        const auto view = views.group("Panel " + id);
        layout.found = true;
        layout.floatingApplets = view.readEntry("floatingApplets", false);
        const int thickness = view.group("Defaults").readEntry("thickness", 0);
        if (thickness > 0) layout.inset = qMax(layout.inset, thickness
            + (layout.floatingApplets && view.readEntry("floating", false) ? 8 : 0));
    }
    return layout;
}

TrayPanel::TrayPanel(QQuickWindow *window) : QObject(window), m_window(window)
{
    window->installEventFilter(this);
    connect(qGuiApp, &QGuiApplication::focusWindowChanged, this, [this] {
        // Let tray activation arrive first; a native child popup can keep focus.
        QTimer::singleShot(100, this, [this] {
            auto *focus = QGuiApplication::focusWindow();
            while (focus && focus != m_window) focus = focus->transientParent();
            if (!focus && !m_window->property("pinned").toBool()) m_window->setProperty("visible", false);
        });
    });
    // Plasma supplies the frame/effects; anchor this standalone popup outside plasmashell.
    if (QGuiApplication::platformName().startsWith("wayland")) {
        m_layer = LayerShellQt::Window::get(window);
        m_layer->setScope("airpods-helper-panel");
        m_layer->setLayer(LayerShellQt::Window::LayerOverlay);
        m_layer->setExclusiveZone(-1);
        m_layer->setKeyboardInteractivity(LayerShellQt::Window::KeyboardInteractivityOnDemand);
    }
}

void TrayPanel::show(QPoint anchor)
{
    if (anchor.isNull()) anchor = QCursor::pos();
    auto *screen = QGuiApplication::screenAt(anchor);
    if (!screen) screen = QGuiApplication::primaryScreen();
    if (!screen) return;
    m_window->setScreen(screen);
    const auto config = KSharedConfig::openConfig("plasma-org.kde.plasma.desktop-appletsrc");
    config->reparseConfiguration();
    const KConfigGroup containments(config, "Containments");
    const auto edges = trayPanelEdges(containments);
    const auto edge = panelEdge(anchor, screen->geometry(), edges);
    m_dockEdge = edge;
    // Qt's Wayland work area can include Plasma's reserved or floating dock.
    const auto shellConfig = KSharedConfig::openConfig("plasmashellrc");
    shellConfig->reparseConfiguration();
    const int dimension = edge == Qt::BottomEdge || edge == Qt::TopEdge ? screen->geometry().height() : screen->geometry().width();
    const auto layout = trayPanelLayout(containments, KConfigGroup(shellConfig, "PlasmaViews"), edge);
    if (layout.found) m_window->setProperty("floating", layout.floatingApplets ? 8 : 1);
    m_dockGap = layout.found && !layout.floatingApplets ? 0 : 8;
    const int inset = qBound(0, layout.inset, dimension - 1);
    m_available = panelAvailableGeometry(screen->geometry(), screen->availableGeometry(), edge, inset);
    if (m_layer) {
        m_layer->setAnchors(LayerShellQt::Window::Anchors(edge == Qt::BottomEdge ? LayerShellQt::Window::AnchorBottom : LayerShellQt::Window::AnchorTop)
            | (edge == Qt::RightEdge ? LayerShellQt::Window::AnchorRight : LayerShellQt::Window::AnchorLeft));
    }
    const auto location = edge == Qt::BottomEdge ? Plasma::Types::BottomEdge
        : edge == Qt::TopEdge ? Plasma::Types::TopEdge
        : edge == Qt::LeftEdge ? Plasma::Types::LeftEdge : Plasma::Types::RightEdge;
    m_window->setProperty("location", location);
    if (auto *content = m_window->property("mainItem").value<QQuickItem *>()) {
        const auto polish = [](auto &&self, QQuickItem *item) -> void {
            item->ensurePolished();
            for (auto *child : item->childItems()) self(self, child);
            item->ensurePolished();
        };
        polish(polish, content);
    }
    const auto *margins = m_window->property("margins").value<QObject *>();
    const int frameHeight = margins ? margins->property("top").toInt() + margins->property("bottom").toInt() : 0;
    const QSize preferred(420, m_window->property("preferredPanelHeight").isValid()
        ? m_window->property("preferredPanelHeight").toInt() + frameHeight : 620);
    if (m_userSize.isEmpty()) m_userSize = preferred;
    applyGeometry(panelGeometry(anchor, screen->geometry(), m_available, m_userSize, edges, m_dockGap));
    m_window->setProperty("visible", true);
    m_window->raise();
    m_window->requestActivate();
}

void TrayPanel::applyGeometry(QRect geometry)
{
    m_geometry = geometry;
    if (auto *content = m_window->property("mainItem").value<QQuickItem *>()) {
        const auto *margins = m_window->property("margins").value<QObject *>();
        const QSizeF frame(margins->property("left").toReal() + margins->property("right").toReal(),
                           margins->property("top").toReal() + margins->property("bottom").toReal());
        content->setSize(QSizeF(geometry.size()) - frame);
    }
    m_window->resize(geometry.size());
    if (m_layer) {
        const auto screen = m_window->screen()->geometry();
        m_layer->setMargins(QMargins(m_dockEdge == Qt::RightEdge ? 0 : geometry.left() - screen.left(),
            m_dockEdge == Qt::BottomEdge ? 0 : geometry.top() - screen.top(),
            m_dockEdge == Qt::RightEdge ? screen.right() - geometry.right() : 0,
            m_dockEdge == Qt::BottomEdge ? screen.bottom() - geometry.bottom() : 0));
    } else {
        m_window->setPosition(geometry.topLeft());
    }
}

bool TrayPanel::eventFilter(QObject *object, QEvent *event)
{
    if (event->type() == QEvent::Hide) m_resizeEdges = {};
    if (event->type() != QEvent::MouseMove && event->type() != QEvent::MouseButtonPress
        && event->type() != QEvent::MouseButtonRelease) return QObject::eventFilter(object, event);
    const auto *mouse = static_cast<QMouseEvent *>(event);
    if (m_resizeEdges) {
        if (event->type() == QEvent::MouseButtonRelease) {
            m_resizeEdges = {};
            return true;
        }
        // Wayland has no global pointer coordinates; our layer margins supply the origin.
        const auto delta = m_geometry.topLeft() + mouse->position().toPoint() - m_resizeStart;
        auto geometry = m_resizeGeometry;
        const auto available = m_available.adjusted(m_dockEdge == Qt::LeftEdge ? m_dockGap : 8,
            m_dockEdge == Qt::TopEdge ? m_dockGap : 8, m_dockEdge == Qt::RightEdge ? -m_dockGap : -8,
            m_dockEdge == Qt::BottomEdge ? -m_dockGap : -8);
        const auto minimum = m_window->minimumSize().expandedTo(QSize(320, 200)).boundedTo(available.size());
        if (m_resizeEdges & Qt::LeftEdge)
            geometry.setLeft(qBound(available.left(), geometry.left() + delta.x(), geometry.right() - minimum.width() + 1));
        if (m_resizeEdges & Qt::RightEdge)
            geometry.setRight(qBound(geometry.left() + minimum.width() - 1, geometry.right() + delta.x(), available.right()));
        if (m_resizeEdges & Qt::TopEdge)
            geometry.setTop(qBound(available.top(), geometry.top() + delta.y(), geometry.bottom() - minimum.height() + 1));
        if (m_resizeEdges & Qt::BottomEdge)
            geometry.setBottom(qBound(geometry.top() + minimum.height() - 1, geometry.bottom() + delta.y(), available.bottom()));
        m_userSize = geometry.size();
        applyGeometry(geometry);
        return true;
    }
    Qt::Edges edges;
    const auto pos = mouse->position();
    if (pos.x() < 6) edges |= Qt::LeftEdge;
    if (pos.x() >= m_window->width() - 6) edges |= Qt::RightEdge;
    if (pos.y() < 6) edges |= Qt::TopEdge;
    if (pos.y() >= m_window->height() - 6) edges |= Qt::BottomEdge;
    const int location = m_window->property("location").toInt();
    if (location == Plasma::Types::BottomEdge) edges &= ~Qt::Edges(Qt::BottomEdge);
    else if (location == Plasma::Types::TopEdge) edges &= ~Qt::Edges(Qt::TopEdge);
    else if (location == Plasma::Types::LeftEdge) edges &= ~Qt::Edges(Qt::LeftEdge);
    else if (location == Plasma::Types::RightEdge) edges &= ~Qt::Edges(Qt::RightEdge);
    if (edges) {
        m_window->setCursor(edges == (Qt::TopEdge | Qt::LeftEdge) || edges == (Qt::BottomEdge | Qt::RightEdge)
            ? Qt::SizeFDiagCursor : edges == (Qt::TopEdge | Qt::RightEdge) || edges == (Qt::BottomEdge | Qt::LeftEdge)
            ? Qt::SizeBDiagCursor : edges.testFlag(Qt::LeftEdge) || edges.testFlag(Qt::RightEdge)
            ? Qt::SizeHorCursor : Qt::SizeVerCursor);
        if (event->type() == QEvent::MouseButtonPress && mouse->button() == Qt::LeftButton) {
            // Layer-shell surfaces have no compositor-driven interactive resize.
            m_resizeEdges = edges;
            m_resizeStart = m_geometry.topLeft() + mouse->position().toPoint();
            m_resizeGeometry = m_geometry;
            return true;
        }
    } else {
        m_window->unsetCursor();
    }
    return QObject::eventFilter(object, event);
}

void TrayPanel::toggle(QPoint anchor)
{
    if (m_window->isVisible()) m_window->setProperty("visible", false); else show(anchor);
}
