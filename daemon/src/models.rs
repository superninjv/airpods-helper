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
}
