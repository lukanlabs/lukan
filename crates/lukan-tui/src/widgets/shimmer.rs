use std::sync::OnceLock;
use std::time::Instant;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

static PROCESS_START: OnceLock<Instant> = OnceLock::new();

fn elapsed_since_start() -> std::time::Duration {
    let start = PROCESS_START.get_or_init(Instant::now);
    start.elapsed()
}

/// Blend two RGB colors. `alpha` = 1.0 means full `fg`, 0.0 means full `bg`.
fn blend(fg: (u8, u8, u8), bg: (u8, u8, u8), alpha: f32) -> (u8, u8, u8) {
    let r = (fg.0 as f32 * alpha + bg.0 as f32 * (1.0 - alpha)) as u8;
    let g = (fg.1 as f32 * alpha + bg.1 as f32 * (1.0 - alpha)) as u8;
    let b = (fg.2 as f32 * alpha + bg.2 as f32 * (1.0 - alpha)) as u8;
    (r, g, b)
}

/// Generate shimmer-animated spans for the given text.
///
/// A bright highlight band sweeps left-to-right across the characters
/// in a 2-second cycle, using a cosine falloff for smooth intensity.
/// Characters near the center of the band blend toward white (highlight),
/// while distant characters stay at the base gray color.
pub fn shimmer_spans(text: &str) -> Vec<Span<'static>> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }

    let padding = 10usize;
    let period = chars.len() + padding * 2;
    let sweep_seconds = 2.0f32;
    let pos_f =
        (elapsed_since_start().as_secs_f32() % sweep_seconds) / sweep_seconds * (period as f32);
    let band_half_width = 5.0f32;

    // Base: gray text. Highlight: blend toward white for the "glow" effect.
    let base_color: (u8, u8, u8) = (128, 128, 128);
    let highlight_color: (u8, u8, u8) = (255, 255, 255);

    let mut spans: Vec<Span<'static>> = Vec::with_capacity(chars.len());

    for (i, ch) in chars.iter().enumerate() {
        let i_pos = i as f32 + padding as f32;
        let dist = (i_pos - pos_f).abs();

        let t = if dist <= band_half_width {
            let x = std::f32::consts::PI * (dist / band_half_width);
            0.5 * (1.0 + x.cos())
        } else {
            0.0
        };

        let highlight = t.clamp(0.0, 1.0);
        let (r, g, b) = blend(highlight_color, base_color, highlight * 0.9);

        let style = Style::default()
            .fg(Color::Rgb(r, g, b))
            .add_modifier(Modifier::BOLD);

        spans.push(Span::styled(ch.to_string(), style));
    }

    spans
}
