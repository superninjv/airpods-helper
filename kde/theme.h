#pragma once
#include <QApplication>
#include <QIcon>
#include <QQuickStyle>
#include <QStyleFactory>
#include <KColorScheme>
#include <KConfigGroup>
#include <KSharedConfig>

inline void applyDesktopTheme()
{
    auto config = KSharedConfig::openConfig("kdeglobals");
    config->reparseConfiguration();
    // KColorScheme also reads through the application config, which includes kdeglobals.
    KSharedConfig::openConfig()->reparseConfiguration();
    const auto style = KConfigGroup(config, "KDE").readEntry("widgetStyle", QString{});
    if (!style.isEmpty() && QStyleFactory::keys().contains(style, Qt::CaseInsensitive))
        QApplication::setStyle(style);
    QFont font;
    const auto configuredFont = KConfigGroup(config, "General").readEntry("font", QString{});
    if (!configuredFont.isEmpty() && font.fromString(configuredFont))
        QApplication::setFont(font);
    const auto icons = KConfigGroup(config, "Icons").readEntry("Theme", QString{});
    if (!icons.isEmpty()) QIcon::setThemeName(icons);
    QApplication::setPalette(KColorScheme::createApplicationPalette(config));
}
