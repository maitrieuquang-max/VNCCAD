//! VNCCad plot pens: what a colour-dependent plot style table (CTB) does when plotting. Each of
//! the 255 colour indices may replace the plotted colour, the lineweight, and screen (lighten)
//! the colour. Tables are read from `.ctb` files by the io crate; two are built in.

use cadcraft_color::Rgb;

/// What plotting does with objects of one colour index.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Pen {
    /// Plot in this colour (`None` = the object's colour).
    pub color: Option<Rgb>,
    /// Plot with this lineweight in mm (`None` = the object's lineweight).
    pub lineweight: Option<f32>,
    /// Ink intensity 0..=100 (100 = full colour, 0 = white).
    pub screen: u8,
}

/// A colour-dependent plot style table: pens for colour indices 1..=255.
#[derive(Clone, Debug, PartialEq)]
pub struct PenTable {
    pub name: String,
    /// Index 0 is unused; 1..=255 are the colour indices.
    pub pens: Vec<Pen>,
}

impl PenTable {
    fn uniform(name: &str, pen: Pen) -> PenTable {
        PenTable { name: name.into(), pens: vec![pen; 256] }
    }
    /// Every colour plots black (VNCCad's own definition of the usual monochrome table).
    pub fn monochrome() -> PenTable {
        PenTable::uniform("monochrome", Pen { color: Some(Rgb(0, 0, 0)), lineweight: None, screen: 100 })
    }
    /// Every colour plots as the grey of its own brightness.
    pub fn grayscale() -> PenTable {
        let mut t = PenTable::uniform("grayscale", Pen { color: None, lineweight: None, screen: 100 });
        for (i, p) in t.pens.iter_mut().enumerate().skip(1) {
            let c = cadcraft_color::aci_rgb(u8::try_from(i).unwrap_or(7));
            let y = (0.299 * f64::from(c.0) + 0.587 * f64::from(c.1) + 0.114 * f64::from(c.2)).round();
            // Light colours would vanish on white paper: darken them like a monochrome plotter.
            let g = (y * 0.8).clamp(0.0, 255.0) as u8;
            p.color = Some(Rgb(g, g, g));
        }
        t
    }
    /// The built-in table for a name, if it is one.
    pub fn builtin(name: &str) -> Option<PenTable> {
        let n = name.trim().to_lowercase();
        let n = n.trim_end_matches(".ctb");
        match n {
            "monochrome" | "don sac" | "đơn sắc" | "den trang" | "đen trắng" => Some(PenTable::monochrome()),
            "grayscale" | "thang xam" | "thang xám" => Some(PenTable::grayscale()),
            _ => None,
        }
    }
    /// Apply the pen of colour index `aci` (0 = a true colour: only lineweight-free defaults).
    pub fn apply(&self, aci: u8, rgb: Rgb, lw: f32) -> (Rgb, f32) {
        let Some(p) = self.pens.get(usize::from(aci)).filter(|_| aci != 0) else { return (rgb, lw) };
        let c = p.color.unwrap_or(rgb);
        let s = f64::from(p.screen.min(100)) / 100.0;
        let mix = |v: u8| (255.0 - (255.0 - f64::from(v)) * s).round().clamp(0.0, 255.0) as u8;
        (Rgb(mix(c.0), mix(c.1), mix(c.2)), p.lineweight.unwrap_or(lw))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_and_screening() {
        let m = PenTable::builtin("Monochrome.CTB").unwrap();
        assert_eq!(m.apply(1, Rgb(255, 0, 0), 0.25), (Rgb(0, 0, 0), 0.25));
        // True colours (no index) keep their colour.
        assert_eq!(m.apply(0, Rgb(10, 20, 30), 0.5), (Rgb(10, 20, 30), 0.5));
        let mut t = PenTable::monochrome();
        t.pens[3] = Pen { color: None, lineweight: Some(0.7), screen: 50 };
        let (c, lw) = t.apply(3, Rgb(0, 0, 0), 0.25);
        assert_eq!((c, lw), (Rgb(128, 128, 128), 0.7));
        let g = PenTable::grayscale();
        let (c, _) = g.apply(1, Rgb(255, 0, 0), 0.25);
        assert!(c.0 == c.1 && c.1 == c.2 && c.0 < 200);
        assert!(PenTable::builtin("acad.ctb").is_none());
    }
}
