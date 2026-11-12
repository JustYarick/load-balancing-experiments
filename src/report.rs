//! Terminal tables and charts.

pub mod c {
    pub const RESET: &str = "\x1b[0m";
    pub const BOLD: &str = "\x1b[1m";
    pub const DIM: &str = "\x1b[2m";
    pub const ITALIC: &str = "\x1b[3m";

    pub fn rgb(r: u8, g: u8, b: u8) -> String {
        format!("\x1b[38;2;{r};{g};{b}m")
    }
    pub fn green() -> String {
        rgb(122, 209, 122)
    }
    pub fn red() -> String {
        rgb(240, 113, 120)
    }
    pub fn yellow() -> String {
        rgb(229, 192, 123)
    }
    pub fn blue() -> String {
        rgb(97, 175, 239)
    }
    pub fn cyan() -> String {
        rgb(86, 182, 194)
    }
    pub fn magenta() -> String {
        rgb(198, 120, 221)
    }
    pub fn grey() -> String {
        rgb(140, 145, 155)
    }
    pub fn white() -> String {
        rgb(220, 223, 228)
    }
    pub fn orange() -> String {
        rgb(209, 154, 102)
    }
}

pub fn vlen(s: &str) -> usize {
    let mut n = 0usize;
    let mut it = s.chars().peekable();
    while let Some(ch) = it.next() {
        if ch == '\x1b' {
            if it.peek() == Some(&'[') {
                it.next();
                while let Some(&nc) = it.peek() {
                    it.next();
                    if ('@'..='~').contains(&nc) {
                        break;
                    }
                }
            }
            continue;
        }
        n += 1;
    }
    n
}

#[derive(Clone, Copy, PartialEq)]
pub enum Align {
    L,
    R,
    C,
}

pub fn table(
    title: Option<&str>,
    headers: &[&str],
    rows: &[Vec<String>],
    aligns: &[Align],
) -> String {
    let ncol = headers.len();
    let mut w = vec![0usize; ncol];
    for (i, h) in headers.iter().enumerate() {
        w[i] = vlen(h);
    }
    for r in rows {
        for i in 0..ncol {
            let cell = r.get(i).map(|s| s.as_str()).unwrap_or("");
            w[i] = w[i].max(vlen(cell));
        }
    }
    let mut out = String::new();
    let line = |l: &str, m: &str, r: &str, w: &Vec<usize>| {
        let mut s = String::from(l);
        for (i, wi) in w.iter().enumerate() {
            s.push_str(&"─".repeat(wi + 2));
            s.push_str(if i + 1 == w.len() { r } else { m });
        }
        s
    };
    if let Some(t) = title {
        out.push_str(&format!("{}{}{}\n", c::BOLD, t, c::RESET));
    }
    out.push_str(&line("┌", "┬", "┐", &w));
    out.push('\n');
    out.push_str("│");
    for (i, h) in headers.iter().enumerate() {
        out.push_str(&format!(
            " {}{}{}{} │",
            c::BOLD,
            pad(h, w[i], aligns[i]),
            c::RESET,
            ""
        ));
    }
    out.push('\n');
    out.push_str(&line("├", "┼", "┤", &w));
    out.push('\n');
    for r in rows {
        out.push('│');
        for i in 0..ncol {
            let cell = r.get(i).map(|s| s.as_str()).unwrap_or("");
            out.push_str(&format!(" {} │", pad(cell, w[i], aligns[i])));
        }
        out.push('\n');
    }
    out.push_str(&line("└", "┴", "┘", &w));
    out.push('\n');
    out
}

fn pad(s: &str, width: usize, a: Align) -> String {
    let l = vlen(s);
    if l >= width {
        return s.to_string();
    }
    let pad = width - l;
    match a {
        Align::L => format!("{}{}", s, " ".repeat(pad)),
        Align::R => format!("{}{}", " ".repeat(pad), s),
        Align::C => {
            let left = pad / 2;
            let right = pad - left;
            format!("{}{}{}", " ".repeat(left), s, " ".repeat(right))
        }
    }
}

pub fn bar(v: f64, max: f64, width: usize, color: &str) -> String {
    let frac = if max <= 0.0 {
        0.0
    } else {
        (v / max).clamp(0.0, 1.0)
    };
    let filled = (frac * width as f64).round() as usize;
    format!(
        "{}{}{}{}{}{}",
        color,
        "█".repeat(filled),
        c::DIM,
        "·".repeat(width - filled),
        c::RESET,
        ""
    )
}

pub fn speedup_color(s: f64, p: f64) -> &'static str {
    let eff = if p > 0.0 { s / p } else { 0.0 };
    if eff >= 0.80 {
        "\x1b[38;2;122;209;122m"
    } else if eff >= 0.50 {
        "\x1b[38;2;229;192;123m"
    } else {
        "\x1b[38;2;240;113;120m"
    }
}

pub fn ratio_color(x: f64) -> &'static str {
    if x < 1.10 {
        "\x1b[38;2;122;209;122m"
    } else if x < 1.35 {
        "\x1b[38;2;229;192;123m"
    } else {
        "\x1b[38;2;240;113;120m"
    }
}

pub fn header(text: &str) -> String {
    format!("\n{}{}{}{}\n", c::BOLD, c::blue(), text, c::RESET)
}

pub fn ms(ns: f64) -> String {
    format!("{:.2}", ns / 1e6)
}
