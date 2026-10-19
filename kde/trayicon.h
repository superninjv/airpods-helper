#pragma once
#include "icons.h"
#include "battery.h"
#include <QIcon>
#include <QPainter>
#include <QPalette>

inline QIcon batteryIcon(int level, const QPalette &palette)
{
    QPixmap pixmap(64, 64);
    pixmap.fill(Qt::transparent);
    QPainter painter(&pixmap);
    painter.setRenderHint(QPainter::Antialiasing);
    auto track = palette.color(QPalette::WindowText);
    track.setAlphaF(0.2);
    painter.setPen(QPen(track, 5, Qt::SolidLine, Qt::RoundCap));
    painter.drawEllipse(QRectF(4, 4, 56, 56));
    if (level >= 0) {
        painter.setPen(QPen(batteryColor(level), 5, Qt::SolidLine, Qt::RoundCap));
        painter.drawArc(QRectF(4, 4, 56, 56), 270 * 16, -qRound(360.0 * 16 * level / 100));
    }
    PanelIcons icons;
    const auto image = icons.requestImage("airpods/" + palette.color(QPalette::WindowText).name(QColor::HexArgb).mid(1),
                                         nullptr, QSize(34, 34));
    painter.drawImage(QPoint(15, 15), image);
    return QIcon(pixmap);
}
