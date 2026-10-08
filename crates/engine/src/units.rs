//! Linear and angular unit formatting and parsing (LUNITS/LUPREC, AUNITS/AUPREC).

use std::f64::consts::PI;

/// Format a distance per LUNITS (1 scientific, 2 decimal, 3 engineering, 4 architectural,
/// 5 fractional) and LUPREC.
pub fn format_distance(v: f64, lunits: i64, luprec: i64) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    let prec = luprec.clamp(0, 8) as usize;
    match lunits {
        1 => format!("{:.*E}", prec, v),
        3 => {
            let neg = v < 0.0;
            let a = v.abs();
            let ft = (a / 12.0).floor();
            let inch = a - ft * 12.0;
            format!("{}{}'-{:.*}\"", if neg { "-" } else { "" }, ft as i64, prec, inch)
        }
        4 => {
            let neg = v < 0.0;
            let a = v.abs();
            let denom = 1i64 << prec.min(8);
            let total = (a * denom as f64).round() as i64;
            let ft = total / (12 * denom);
            let rem = total - ft * 12 * denom;
            let inch = rem / denom;
            let frac = rem % denom;
            let fs = if frac == 0 {
                String::new()
            } else {
                let g = gcd(frac, denom);
                format!(" {}/{}", frac / g, denom / g)
            };
            format!("{}{}'-{}{}\"", if neg { "-" } else { "" }, ft, inch, fs)
        }
        5 => {
            let neg = v < 0.0;
            let a = v.abs();
            let denom = 1i64 << prec.min(8);
            let total = (a * denom as f64).round() as i64;
            let whole = total / denom;
            let frac = total % denom;
            let fs = if frac == 0 {
                String::new()
            } else {
                let g = gcd(frac, denom);
                format!(" {}/{}", frac / g, denom / g)
            };
            format!("{}{}{}", if neg { "-" } else { "" }, whole, fs)
        }
        _ => format!("{:.*}", prec, v),
    }
}

fn gcd(a: i64, b: i64) -> i64 {
    if b == 0 { a.abs().max(1) } else { gcd(b, a % b) }
}

/// Format an angle (radians) per AUNITS (0 decimal degrees, 1 deg/min/sec, 2 grads, 3 radians,
/// 4 surveyor's) and AUPREC.
pub fn format_angle(a: f64, aunits: i64, auprec: i64) -> String {
    let prec = auprec.clamp(0, 8) as usize;
    let deg = cadcraft_geom::norm_angle(a).to_degrees();
    match aunits {
        1 => {
            let d = deg.floor();
            let m = ((deg - d) * 60.0).floor();
            let s = (deg - d - m / 60.0) * 3600.0;
            format!("{}d{}'{:.*}\"", d as i64, m as i64, prec.saturating_sub(4), s)
        }
        2 => format!("{:.*}g", prec, deg / 0.9),
        3 => format!("{:.*}r", prec, cadcraft_geom::norm_angle(a)),
        4 => {
            // N/S <angle from N/S toward E/W> E/W
            let (ns, ew, ang) = if deg <= 90.0 {
                ("N", "E", 90.0 - deg)
            } else if deg <= 180.0 {
                ("N", "W", deg - 90.0)
            } else if deg <= 270.0 {
                ("S", "W", 270.0 - deg)
            } else {
                ("S", "E", deg - 270.0)
            };
            format!("{ns} {:.*}d {ew}", prec, ang)
        }
        _ => format!("{:.*}", prec, deg),
    }
}

/// Parse a distance typed by the user: decimal, `1'6"`, `1'-6 1/2"`, `6 1/2`, `3/4`, `1e3`.
pub fn parse_distance(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    if let Ok(v) = t.parse::<f64>() {
        return v.is_finite().then_some(v);
    }
    let (neg, t) = match t.strip_prefix('-') {
        Some(r) => (true, r.trim()),
        None => (false, t),
    };
    let mut total = 0.0;
    let mut rest = t;
    if let Some(i) = rest.find('\'') {
        let ft: f64 = rest.get(..i)?.trim().parse().ok()?;
        total += ft * 12.0;
        rest = rest.get(i + 1..)?.trim_start_matches('-').trim();
    }
    let rest = rest.trim_end_matches('"').trim();
    if !rest.is_empty() {
        total += parse_mixed(rest)?;
    }
    let v = if neg { -total } else { total };
    v.is_finite().then_some(v)
}

fn parse_mixed(s: &str) -> Option<f64> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    let frac = |p: &str| -> Option<f64> {
        if let Some((a, b)) = p.split_once('/') {
            let d: f64 = b.trim().parse().ok()?;
            if d == 0.0 {
                return None;
            }
            Some(a.trim().parse::<f64>().ok()? / d)
        } else {
            p.parse().ok()
        }
    };
    match parts.as_slice() {
        [a] => frac(a),
        [a, b] => Some(a.parse::<f64>().ok()? + frac(b)?),
        _ => None,
    }
}

/// Parse an angle typed by the user (degrees by default): `45`, `45d30'`, `0.5r`, `100g`, `N45dE`.
pub fn parse_angle(s: &str) -> Option<f64> {
    let t = s.trim().to_ascii_lowercase();
    if t.is_empty() {
        return None;
    }
    if let Some(r) = t.strip_suffix('r') {
        return r.trim().parse::<f64>().ok().filter(|v| v.is_finite());
    }
    if let Some(g) = t.strip_suffix('g') {
        return g.trim().parse::<f64>().ok().filter(|v| v.is_finite()).map(|v| (v * 0.9).to_radians());
    }
    if let Some(rest) = t.strip_prefix('n').or_else(|| t.strip_prefix('s')) {
        let north = t.starts_with('n');
        let (ang_s, east) = if let Some(a) = rest.strip_suffix('e') {
            (a, true)
        } else {
            let a = rest.strip_suffix('w')?;
            (a, false)
        };
        let a = parse_dms(ang_s.trim())?;
        let deg = match (north, east) {
            (true, true) => 90.0 - a,
            (true, false) => 90.0 + a,
            (false, false) => 270.0 - a,
            (false, true) => 270.0 + a,
        };
        return Some(deg.to_radians());
    }
    parse_dms(&t).map(f64::to_radians)
}

fn parse_dms(t: &str) -> Option<f64> {
    if let Ok(v) = t.parse::<f64>() {
        return v.is_finite().then_some(v);
    }
    let (d, rest) = t.split_once('d').unwrap_or((t, ""));
    let mut deg: f64 = d.trim().parse().ok()?;
    let rest = rest.trim();
    if !rest.is_empty() {
        let (m, rest2) = rest.split_once('\'').unwrap_or((rest, ""));
        if !m.trim().is_empty() {
            deg += m.trim().parse::<f64>().ok()? / 60.0;
        }
        let s = rest2.trim().trim_end_matches('"');
        if !s.is_empty() {
            deg += s.parse::<f64>().ok()? / 3600.0;
        }
    }
    deg.is_finite().then_some(deg)
}

pub const DEG: f64 = PI / 180.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_and_arch() {
        assert_eq!(format_distance(28.98524, 2, 4), "28.9852");
        assert_eq!(format_distance(18.5, 4, 4), "1'-6 1/2\"");
        assert_eq!(format_distance(6.25, 5, 2), "6 1/4");
        assert_eq!(format_distance(30.0, 3, 2), "2'-6.00\"");
    }

    #[test]
    fn parse_distances() {
        assert_eq!(parse_distance("12.5"), Some(12.5));
        assert_eq!(parse_distance("1'6\""), Some(18.0));
        assert_eq!(parse_distance("1'-6 1/2\""), Some(18.5));
        assert_eq!(parse_distance("3/4"), Some(0.75));
        assert_eq!(parse_distance("1/0"), None);
        assert_eq!(parse_distance("abc"), None);
        assert_eq!(parse_distance(""), None);
    }

    #[test]
    fn angles() {
        assert!((parse_angle("90").unwrap() - PI / 2.0).abs() < 1e-12);
        assert!((parse_angle("45d30'").unwrap().to_degrees() - 45.5).abs() < 1e-9);
        assert!((parse_angle("N45dE").unwrap().to_degrees() - 45.0).abs() < 1e-9);
        assert!((parse_angle("1.5r").unwrap() - 1.5).abs() < 1e-12);
        assert_eq!(format_angle(PI / 2.0, 0, 0), "90");
        assert_eq!(format_angle(PI / 4.0, 4, 0), "N 45d E");
    }
}
