#pragma once
#include <QDBusArgument>
#include <QDBusMetaType>

struct EqBand { QString type; double freq, q, gain; };
Q_DECLARE_METATYPE(EqBand)
using EqBands = QList<EqBand>;
Q_DECLARE_METATYPE(EqBands)
inline QDBusArgument &operator<<(QDBusArgument &a, const EqBand &b)
{ a.beginStructure(); a << b.type << b.freq << b.q << b.gain; a.endStructure(); return a; }
inline const QDBusArgument &operator>>(const QDBusArgument &a, EqBand &b)
{ a.beginStructure(); a >> b.type >> b.freq >> b.q >> b.gain; a.endStructure(); return a; }
