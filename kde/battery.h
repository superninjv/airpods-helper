#pragma once

#include <QVariantMap>
#include <QColor>

inline QColor batteryColor(int level)
{
    // Match the bright battery palette used by the Tauri UI and tray ring.
    return level <= 20 ? QColor(255, 69, 58)
        : level <= 50 ? QColor(255, 159, 10) : QColor(48, 209, 88);
}

// Average reported earbud batteries. Never include the case or unknown values.
inline int batteryLevel(const QVariantMap &state)
{
    if (!state.value("Connected").toBool())
        return -1;
    int total = 0, count = 0;
    for (const auto &key : {"BatteryLeft", "BatteryRight"}) {
        const int level = state.value(key, -1).toInt();
        if (level >= 0 && level <= 100) { total += level; ++count; }
    }
    return count ? qRound(double(total) / count) : -1;
}
