#include <QtTest>
#include "battery.h"

class BatteryTest : public QObject
{
    Q_OBJECT
private slots:
    void colors()
    {
        for (int level : {0, 20})
            QCOMPARE(batteryColor(level), QColor("#ff453a"));
        for (int level : {21, 50})
            QCOMPARE(batteryColor(level), QColor("#ff9f0a"));
        for (int level : {51, 100})
            QCOMPARE(batteryColor(level), QColor("#30d158"));
    }
    void level()
    {
        QVariantMap state{{"Connected", true}, {"BatteryLeft", 80}, {"BatteryRight", 25}, {"BatteryCase", 100}};
        QCOMPARE(batteryLevel(state), 53);
        state["BatteryCase"] = 0;
        QCOMPARE(batteryLevel(state), 53);
        state["BatteryLeft"] = -1;
        QCOMPARE(batteryLevel(state), 25);
        state["BatteryRight"] = -1;
        QCOMPARE(batteryLevel(state), -1);
        state["BatteryLeft"] = 0;
        QCOMPARE(batteryLevel(state), 0);
        state["BatteryLeft"] = 100;
        QCOMPARE(batteryLevel(state), 100);
        state["BatteryLeft"] = 101;
        QCOMPARE(batteryLevel(state), -1);
        state["Connected"] = false;
        state["BatteryLeft"] = 80;
        QCOMPARE(batteryLevel(state), -1);
        QCOMPARE(batteryLevel({}), -1);
    }
};
QTEST_GUILESS_MAIN(BatteryTest)
#include "battery_test.moc"
