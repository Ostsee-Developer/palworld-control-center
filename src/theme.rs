use ratatui::style::Color;

pub const BACKGROUND: Color = Color::Rgb(3, 8, 12);
pub const PANEL: Color = Color::Rgb(7, 17, 23);
pub const PANEL_ALT: Color = Color::Rgb(9, 23, 29);
pub const MINT: Color = Color::Rgb(74, 246, 202);
pub const CYAN: Color = Color::Rgb(70, 199, 255);
pub const TEXT: Color = Color::Rgb(204, 230, 235);
pub const MUTED: Color = Color::Rgb(91, 124, 132);
pub const GREEN: Color = Color::Rgb(75, 226, 137);
pub const AMBER: Color = Color::Rgb(255, 190, 80);
pub const RED: Color = Color::Rgb(255, 92, 115);

pub fn usage_color(value: f64) -> Color {
    if value >= 90.0 {
        RED
    } else if value >= 75.0 {
        AMBER
    } else {
        MINT
    }
}
