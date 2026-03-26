/// Map Apple model numbers to human-readable product names.
pub fn model_display_name(model_number: &str) -> &str {
    match model_number {
        "A1523" | "A1722" => "AirPods 1",
        "A2031" | "A2032" => "AirPods 2",
        "A2564" | "A2565" => "AirPods 3",
        "A3050" | "A3053" | "A3054" | "A3058" => "AirPods 4",
        "A3055" | "A3056" | "A3057" | "A3059" => "AirPods 4 ANC",
        "A2083" | "A2084" | "A2190" => "AirPods Pro",
        "A2698" | "A2699" | "A2700" | "A2931" => "AirPods Pro 2",
        "A2968" | "A3047" | "A3048" | "A3049" => "AirPods Pro 2",
        "A3063" | "A3064" | "A3065" | "A3122" => "AirPods Pro 3",
        "A2096" => "AirPods Max",
        "A3184" => "AirPods Max 2",
        "A1602" | "A1938" => "AirPods Case",
        "A2566" | "A2897" => "AirPods 3 Case",
        _ => model_number,
    }
}
