//! VNCCad: table cell formulas (`=A1+B2*2`, `=SUM(B2:B10)`, `=AVERAGE(…)`, `MIN`, `MAX`,
//! `COUNT`, `ROUND(x; n)`), as AutoCAD tables compute them. A cell whose text starts with `=`
//! shows its value; the formula stays in the cell.

use crate::Table;

/// Column letters and row number of a reference (`B12` → (11, 1)), zero-based.
fn parse_ref(s: &str) -> Option<(usize, usize)> {
    let s = s.trim().to_ascii_uppercase();
    let letters: String = s.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    let digits = s.get(letters.len()..)?;
    if letters.is_empty() || letters.len() > 3 || digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let col = letters.chars().fold(0usize, |a, c| a * 26 + (c as usize - 'A' as usize + 1)) - 1;
    let row = digits.parse::<usize>().ok()?.checked_sub(1)?;
    Some((row, col))
}

/// A number written in a cell ("12.5", "1,5", "1 250,75", "12.5 m").
pub fn cell_number(s: &str) -> Option<f64> {
    let t: String = s.trim().chars().filter(|c| !c.is_whitespace()).collect();
    let t = t.trim_end_matches(|c: char| c.is_alphabetic() || c == '²' || c == '³');
    if t.is_empty() {
        return None;
    }
    let t = if t.contains(',') && !t.contains('.') {
        t.replace(',', ".")
    } else if t.contains(',') && t.contains('.') {
        // 1.250,75 or 1,250.75: the last separator is the decimal one.
        let (dot, comma) = (t.rfind('.').unwrap_or(0), t.rfind(',').unwrap_or(0));
        if comma > dot { t.replace('.', "").replace(',', ".") } else { t.replace(',', "") }
    } else {
        t.to_string()
    };
    t.parse::<f64>().ok().filter(|v| v.is_finite())
}

struct P<'a> {
    t: &'a Table,
    c: Vec<char>,
    i: usize,
    depth: usize,
}

impl P<'_> {
    fn ws(&mut self) {
        while self.c.get(self.i).is_some_and(|c| c.is_whitespace()) {
            self.i += 1;
        }
    }
    fn eat(&mut self, ch: char) -> bool {
        self.ws();
        if self.c.get(self.i) == Some(&ch) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn expr(&mut self) -> Option<f64> {
        let mut v = self.term()?;
        loop {
            if self.eat('+') {
                v += self.term()?;
            } else if self.eat('-') {
                v -= self.term()?;
            } else {
                return Some(v);
            }
        }
    }
    fn term(&mut self) -> Option<f64> {
        let mut v = self.power()?;
        loop {
            if self.eat('*') {
                v *= self.power()?;
            } else if self.eat('/') {
                let d = self.power()?;
                if d == 0.0 {
                    return None;
                }
                v /= d;
            } else {
                return Some(v);
            }
        }
    }
    fn power(&mut self) -> Option<f64> {
        let b = self.unary()?;
        if self.eat('^') { Some(b.powf(self.power()?)) } else { Some(b) }
    }
    fn unary(&mut self) -> Option<f64> {
        if self.eat('-') {
            return Some(-self.unary()?);
        }
        if self.eat('+') {
            return self.unary();
        }
        self.atom()
    }
    fn word(&mut self) -> String {
        self.ws();
        let st = self.i;
        while self.c.get(self.i).is_some_and(|c| c.is_ascii_alphanumeric()) {
            self.i += 1;
        }
        self.c.get(st..self.i).map(|s| s.iter().collect()).unwrap_or_default()
    }
    /// Arguments of a function: numbers, with ranges expanded.
    fn args(&mut self) -> Option<Vec<f64>> {
        let mut out = Vec::new();
        if self.eat(')') {
            return Some(out);
        }
        loop {
            self.ws();
            let save = self.i;
            let w = self.word();
            if let Some(a) = parse_ref(&w)
                && self.eat(':')
            {
                let b = parse_ref(&self.word())?;
                for r in a.0.min(b.0)..=a.0.max(b.0) {
                    for c in a.1.min(b.1)..=a.1.max(b.1) {
                        if let Some(v) = self.cell(r, c) {
                            out.push(v);
                        }
                    }
                }
            } else {
                self.i = save;
                out.push(self.expr()?);
            }
            if self.eat(')') {
                return Some(out);
            }
            if !(self.eat(',') || self.eat(';')) {
                return None;
            }
        }
    }
    fn atom(&mut self) -> Option<f64> {
        self.ws();
        if self.eat('(') {
            let v = self.expr()?;
            return self.eat(')').then_some(v);
        }
        let c = *self.c.get(self.i)?;
        if c.is_ascii_digit() || c == '.' {
            let st = self.i;
            while self.c.get(self.i).is_some_and(|c| c.is_ascii_digit() || *c == '.') {
                self.i += 1;
            }
            let s: String = self.c.get(st..self.i)?.iter().collect();
            return s.parse().ok();
        }
        let w = self.word();
        if self.eat('(') {
            let a = self.args()?;
            return match w.to_ascii_uppercase().as_str() {
                "SUM" => Some(a.iter().sum()),
                "AVERAGE" | "AVG" => (!a.is_empty()).then(|| a.iter().sum::<f64>() / a.len() as f64),
                "MIN" => a.iter().copied().reduce(f64::min),
                "MAX" => a.iter().copied().reduce(f64::max),
                "COUNT" => Some(a.len() as f64),
                "ABS" => a.first().map(|v| v.abs()),
                "SQRT" => a.first().filter(|v| **v >= 0.0).map(|v| v.sqrt()),
                "ROUND" => {
                    let n = a.get(1).copied().unwrap_or(0.0).clamp(-10.0, 10.0);
                    let k = 10f64.powf(n.round());
                    a.first().map(|v| (v * k).round() / k)
                }
                "PI" => Some(std::f64::consts::PI),
                _ => None,
            };
        }
        let (r, c) = parse_ref(&w)?;
        self.cell(r, c)
    }
    fn cell(&self, r: usize, c: usize) -> Option<f64> {
        let text = &self.t.cells.get(r)?.get(c)?.text;
        if let Some(f) = text.trim().strip_prefix('=') {
            if self.depth > 32 {
                return None;
            }
            return eval(self.t, f, self.depth + 1);
        }
        cell_number(text)
    }
}

fn eval(t: &Table, src: &str, depth: usize) -> Option<f64> {
    let mut p = P { t, c: src.chars().collect(), i: 0, depth };
    let v = p.expr()?;
    p.ws();
    (p.i == p.c.len() && v.is_finite()).then_some(v)
}

/// A value as a table shows it: up to 4 decimals, trailing zeros removed.
pub fn format_value(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s == "-0" { "0".into() } else { s }
}

impl Table {
    /// What a cell shows: its text, or the value of its formula (`#ERR` when it cannot be
    /// computed).
    pub fn display_text(&self, r: usize, c: usize) -> String {
        let Some(cell) = self.cells.get(r).and_then(|row| row.get(c)) else { return String::new() };
        match cell.text.trim().strip_prefix('=') {
            Some(f) => eval(self, f, 0).map(format_value).unwrap_or_else(|| "#ERR".into()),
            None => cell.text.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TableCell;

    #[test]
    fn formulas_compute_like_autocad_tables() {
        let c = |t: &str| TableCell { text: t.into(), merged: None };
        let t = Table {
            insert: Default::default(),
            col_widths: vec![1.0; 4],
            row_heights: vec![1.0; 4],
            cells: vec![
                vec![c("Hạng mục"), c("KL"), c("Đơn giá"), c("Thành tiền")],
                vec![c("Bê tông"), c("12,5"), c("1500"), c("=B2*C2")],
                vec![c("Cốt thép"), c("2.75 T"), c("20000"), c("=B3*C3")],
                vec![c("Tổng"), c("=SUM(B2:B3)"), c(""), c("=ROUND(SUM(D2:D3)/1000; 1)")],
            ],
            style: "Standard".into(),
            text_height: 0.2,
            title: false,
            header: true,
        };
        assert_eq!(t.display_text(1, 3), "18750");
        assert_eq!(t.display_text(2, 3), "55000");
        assert_eq!(t.display_text(3, 1), "15.25");
        assert_eq!(t.display_text(3, 3), "73.8");
        assert_eq!(t.display_text(0, 0), "Hạng mục");
        let mut bad = t.clone();
        bad.cells[1][3].text = "=D2+1".into();
        assert_eq!(bad.display_text(1, 3), "#ERR");
    }
}
