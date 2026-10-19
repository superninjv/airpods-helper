#pragma once

#include <QObject>
#include <QRect>
#include <QList>
#include <limits>

class KConfigGroup;

class QQuickWindow;
namespace LayerShellQt { class Window; }

QList<Qt::Edge> trayPanelEdges(const KConfigGroup &containments);
struct TrayPanelLayout {
    int inset = 0;
    bool floatingApplets = false;
    bool found = false;
};
TrayPanelLayout trayPanelLayout(const KConfigGroup &containments, const KConfigGroup &views, Qt::Edge edge);

inline QRect panelAvailableGeometry(QRect screen, QRect available, Qt::Edge edge, int inset)
{
    if (edge == Qt::BottomEdge) screen.adjust(0, 0, 0, -inset);
    else if (edge == Qt::TopEdge) screen.adjust(0, inset, 0, 0);
    else if (edge == Qt::LeftEdge) screen.adjust(inset, 0, 0, 0);
    else screen.adjust(0, 0, -inset, 0);
    return available.intersected(screen);
}

// Restrict corner guesses to edges that actually host a Plasma system tray.
inline Qt::Edge panelEdge(QPoint anchor, QRect screen, const QList<Qt::Edge> &trayEdges = {})
{
    const QPair<Qt::Edge, int> distances[] = {
        {Qt::BottomEdge, qAbs(anchor.y() - screen.bottom())},
        {Qt::TopEdge, qAbs(anchor.y() - screen.top())},
        {Qt::LeftEdge, qAbs(anchor.x() - screen.left())},
        {Qt::RightEdge, qAbs(anchor.x() - screen.right())}
    };
    Qt::Edge edge = Qt::BottomEdge;
    int nearest = std::numeric_limits<int>::max();
    for (const auto &[candidate, distance] : distances) {
        if ((trayEdges.isEmpty() || trayEdges.contains(candidate)) && distance < nearest) {
            edge = candidate;
            nearest = distance;
        }
    }
    return edge;
}

// Keep the popup inside the usable screen, beside the nearest panel edge.
inline QRect panelGeometry(QPoint anchor, QRect screen, QRect available, QSize size, const QList<Qt::Edge> &trayEdges = {}, int dockGap = 8)
{
    const auto edge = panelEdge(anchor, screen, trayEdges);
    available = available.adjusted(edge == Qt::LeftEdge ? dockGap : 8, edge == Qt::TopEdge ? dockGap : 8,
        edge == Qt::RightEdge ? -dockGap : -8, edge == Qt::BottomEdge ? -dockGap : -8);
    size = size.boundedTo(available.size());
    int x = anchor.x() - size.width() / 2;
    int y = anchor.y() - size.height() / 2;
    if (edge == Qt::BottomEdge) y = available.bottom() - size.height() + 1;
    else if (edge == Qt::TopEdge) y = available.top();
    else if (edge == Qt::LeftEdge) x = available.left();
    else x = available.right() - size.width() + 1;
    return {QPoint(qBound(available.left(), x, available.right() - size.width() + 1),
                   qBound(available.top(), y, available.bottom() - size.height() + 1)), size};
}

class TrayPanel : public QObject
{
    Q_OBJECT
public:
    explicit TrayPanel(QQuickWindow *window);
    void show(QPoint anchor = {});
    void toggle(QPoint anchor);
protected:
    bool eventFilter(QObject *object, QEvent *event) override;
private:
    void applyGeometry(QRect geometry);
    QQuickWindow *m_window;
    LayerShellQt::Window *m_layer = nullptr;
    QSize m_userSize;
    QRect m_geometry, m_resizeGeometry;
    QRect m_available;
    QPoint m_resizeStart;
    Qt::Edges m_resizeEdges;
    Qt::Edge m_dockEdge = Qt::BottomEdge;
    int m_dockGap = 8;
};
