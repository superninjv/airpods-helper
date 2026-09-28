// AirPods Max report a single "headphones" battery (component 0x01).

use aap::parser::{AapEvent, parse};

#[test]
fn headphone_battery() {
    for level in [0, 73, 100] {
        for (status, charging, connected) in [
            (0x01, true, true),
            (0x02, false, true),
            (0x04, false, false),
        ] {
            let packet = [
                0x04, 0x00, 0x04, 0x00, 0x04, 0x00, // battery notification
                0x01, // one battery
                0x01, 0x01, level, status, 0x01, // headphones component
            ];
            let AapEvent::Battery(battery) = parse(&packet).unwrap() else {
                panic!("expected battery notification");
            };
            for entry in [battery.left, battery.right] {
                let entry = entry.expect("headphone battery must be exposed to clients");
                assert_eq!(entry.level, level);
                assert_eq!(entry.charging, charging);
                assert_eq!(entry.connected, connected);
            }
            assert!(battery.case.is_none(), "AirPods Max has no case battery");
            assert!(battery.primary.is_none(), "headphones have no primary bud");
            for length in 6..packet.len() {
                assert!(parse(&packet[..length]).is_err());
            }
        }
    }
}
