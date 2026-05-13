/// Feature capabilities that vary by AirPods model.
///
/// Used by widgets and CLI to conditionally show controls that only
/// apply to certain hardware (e.g. ANC controls hidden for AirPods 3).
pub fn model_features(model_number: &str) -> Vec<&'static str> {
    match model_number {
        // AirPods 1
        "A1523" | "A1722" => vec![],
        // AirPods 2
        "A2031" | "A2032" => vec![],
        // AirPods 3
        "A2564" | "A2565" => vec![],
        // AirPods 4 (no ANC)
        "A3050" | "A3053" | "A3054" | "A3058" => vec![],
        // AirPods 4 ANC
        "A3055" | "A3056" | "A3057" | "A3059" => vec!["anc", "adaptive", "ca", "one_bud_anc"],
        // AirPods Pro 1 — ANC but no adaptive/CA/one-bud
        "A2083" | "A2084" | "A2190" => vec!["anc"],
        // AirPods Pro 2 (Lightning + USB-C)
        "A2698" | "A2699" | "A2700" | "A2931"
        | "A2968" | "A3047" | "A3048" | "A3049" => vec!["anc", "adaptive", "ca", "one_bud_anc"],
        // AirPods Pro 3
        "A3063" | "A3064" | "A3065" | "A3122" => vec!["anc", "adaptive", "ca", "one_bud_anc"],
        // AirPods Max (Lightning) — ANC but no adaptive/CA
        "A2096" => vec!["anc"],
        // AirPods Max 2 (USB-C) — ANC + adaptive + CA, no one-bud (single unit)
        "A3184" => vec!["anc", "adaptive", "ca"],
        // Unknown model — expose everything, let firmware decide
        _ => vec!["anc", "adaptive", "ca", "one_bud_anc"],
    }
}

/// Map Apple model numbers to human-readable product names.
///
/// Sources: <https://support.apple.com/en-us/109525>, LibrePods project
pub fn model_display_name(model_number: &str) -> &str {
    match model_number {
        // AirPods (1st generation)
        "A1523" | "A1722" => "AirPods 1",

        // AirPods (2nd generation)
        "A2031" | "A2032" => "AirPods 2",

        // AirPods (3rd generation)
        "A2564" | "A2565" => "AirPods 3",

        // AirPods 4
        "A3050" | "A3053" | "A3054" | "A3058" => "AirPods 4",

        // AirPods 4 (ANC)
        "A3055" | "A3056" | "A3057" | "A3059" => "AirPods 4 ANC",

        // AirPods Pro (1st generation)
        "A2083" | "A2084" | "A2190" => "AirPods Pro",

        // AirPods Pro 2 (Lightning)
        "A2698" | "A2699" | "A2700" | "A2931" => "AirPods Pro 2",

        // AirPods Pro 2 (USB-C)
        "A2968" | "A3047" | "A3048" | "A3049" => "AirPods Pro 2",

        // AirPods Pro 3
        "A3063" | "A3064" | "A3065" | "A3122" => "AirPods Pro 3",

        // AirPods Max (Lightning)
        "A2096" => "AirPods Max",

        // AirPods Max (USB-C)
        "A3184" => "AirPods Max 2",

        // Charging cases (standalone, without earbuds context)
        "A1602" | "A1938" => "AirPods Case",
        "A2566" | "A2897" => "AirPods 3 Case",

        _ => model_number,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_models() {
        assert_eq!(model_display_name("A2698"), "AirPods Pro 2");
        assert_eq!(model_display_name("A2699"), "AirPods Pro 2");
        assert_eq!(model_display_name("A3047"), "AirPods Pro 2");
        assert_eq!(model_display_name("A3048"), "AirPods Pro 2");
        assert_eq!(model_display_name("A2084"), "AirPods Pro");
        assert_eq!(model_display_name("A2564"), "AirPods 3");
        assert_eq!(model_display_name("A3055"), "AirPods 4 ANC");
        assert_eq!(model_display_name("A3050"), "AirPods 4");
        assert_eq!(model_display_name("A2096"), "AirPods Max");
        assert_eq!(model_display_name("A3184"), "AirPods Max 2");
        assert_eq!(model_display_name("A3063"), "AirPods Pro 3");
    }

    #[test]
    fn test_unknown_model_returns_raw() {
        assert_eq!(model_display_name("A9999"), "A9999");
        assert_eq!(model_display_name("UNKNOWN"), "UNKNOWN");
    }

    #[test]
    fn test_features_no_anc() {
        // AirPods 1/2/3/4 (non-ANC) have no active noise features
        assert!(model_features("A1523").is_empty());
        assert!(model_features("A2032").is_empty());
        assert!(model_features("A2564").is_empty());
        assert!(model_features("A3050").is_empty());
    }

    #[test]
    fn test_features_anc_only() {
        // Pro 1 and Max 1 have ANC but no adaptive/CA
        let f = model_features("A2084");
        assert!(f.contains(&"anc"));
        assert!(!f.contains(&"adaptive"));
        assert!(!f.contains(&"ca"));
    }

    #[test]
    fn test_features_full() {
        // Pro 2/3 and AirPods 4 ANC have everything
        let f = model_features("A2698");
        assert!(f.contains(&"anc"));
        assert!(f.contains(&"adaptive"));
        assert!(f.contains(&"ca"));
        assert!(f.contains(&"one_bud_anc"));
    }

    #[test]
    fn test_features_max2_no_one_bud() {
        let f = model_features("A3184");
        assert!(f.contains(&"anc"));
        assert!(f.contains(&"ca"));
        assert!(!f.contains(&"one_bud_anc"));
    }

    #[test]
    fn test_features_unknown_exposes_all() {
        let f = model_features("A9999");
        assert!(f.contains(&"anc"));
        assert!(f.contains(&"adaptive"));
        assert!(f.contains(&"ca"));
        assert!(f.contains(&"one_bud_anc"));
    }
}
