#pragma once

#include <QImageReader>
#include <QPainter>
#include <QQuickImageProvider>

// Tint the bundled SVGs on the CPU too: Qt's software renderer has no mask shader.
class PanelIcons : public QQuickImageProvider
{
public:
    PanelIcons() : QQuickImageProvider(QQuickImageProvider::Image) {}
    QImage requestImage(const QString &id, QSize *size, const QSize &requested) override
    {
        const auto parts = id.split('/');
        if (parts.size() != 2) return {};
        const QColor color("#" + parts[1]);
        if (!color.isValid()) return {};
        QImageReader reader(":/icons/" + parts[0] + ".svg");
        reader.setScaledSize(requested.isValid() ? requested : QSize(48, 48));
        auto image = reader.read().convertToFormat(QImage::Format_ARGB32_Premultiplied);
        if (image.isNull()) return {};
        QPainter painter(&image);
        painter.setCompositionMode(QPainter::CompositionMode_SourceIn);
        painter.fillRect(image.rect(), color);
        painter.end();
        if (size) *size = image.size();
        return image;
    }
};
