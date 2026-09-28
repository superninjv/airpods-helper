//! Mapping AAP's primary/secondary bud reports onto left/right.

use crate::BatteryComponent;
use crate::parser::{BatteryEntry, BatteryUpdate, EarDetectionUpdate};

/// Ear-detection packets are ordered (primary, secondary). The primary is the
/// bud talking to the host; it's listed first in battery notifications and
/// swaps when, e.g., the right bud goes back in the case. This tracks it so
/// ear status lands on the right side.
#[derive(Debug, Default)]
pub struct BudTracker {
    primary: Option<BatteryComponent>,
    ear: Option<EarDetectionUpdate>,
}

impl BudTracker {
    /// Feed a battery update. Returns re-mapped `(left_in_ear, right_in_ear)`
    /// if the primary bud changed and ear status is already known.
    pub fn on_battery(&mut self, b: &BatteryUpdate) -> Option<(bool, bool)> {
        if b.primary.is_none() || b.primary == self.primary {
            return None;
        }
        self.primary = b.primary;
        self.ear.map(|ear| self.map(ear))
    }

    /// Feed an ear-detection update; returns `(left_in_ear, right_in_ear)`.
    pub fn on_ear(&mut self, ear: EarDetectionUpdate) -> (bool, bool) {
        self.ear = Some(ear);
        self.map(ear)
    }

    fn map(&self, ear: EarDetectionUpdate) -> (bool, bool) {
        // Until the first battery packet, assume the right bud is primary
        // (the default on every model seen so far).
        if self.primary == Some(BatteryComponent::Left) {
            (ear.primary.is_in_ear(), ear.secondary.is_in_ear())
        } else {
            (ear.secondary.is_in_ear(), ear.primary.is_in_ear())
        }
    }
}

impl BatteryEntry {
    /// Level for display: `-1` when the component reports itself
    /// disconnected (e.g. a bud in a closed case), since its level is stale.
    pub fn display_level(&self) -> i32 {
        if self.connected {
            self.level as i32
        } else {
            -1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EarStatus;

    fn battery(primary: BatteryComponent) -> BatteryUpdate {
        BatteryUpdate {
            primary: Some(primary),
            left: None,
            right: None,
            case: None,
        }
    }

    #[test]
    fn follows_primary_swaps() {
        let mut t = BudTracker::default();
        let ear = EarDetectionUpdate {
            primary: EarStatus::InEar,
            secondary: EarStatus::InCase,
        };
        // Default: right is primary → right in ear, left in case.
        assert_eq!(t.on_ear(ear), (false, true));
        // Left becomes primary: the same report now means the left bud is in.
        assert_eq!(
            t.on_battery(&battery(BatteryComponent::Left)),
            Some((true, false))
        );
        assert_eq!(t.on_battery(&battery(BatteryComponent::Left)), None);
        assert_eq!(t.on_ear(ear), (true, false));
    }
}
