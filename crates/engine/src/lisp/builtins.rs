//! AutoLISP built-in functions: arithmetic, comparison, lists, strings, conversions, output.

use cadcraft_geom::Vec2;

use super::{Ex, R, Run, V, err, to_text};

fn arg(a: &[V], i: usize) -> &V {
    a.get(i).unwrap_or(&V::Nil)
}

fn num(a: &[V], i: usize, f: &str) -> R<f64> {
    arg(a, i).num().ok_or_else(|| Ex::Error(format!("{f}: đối số sai kiểu: {}", to_text(arg(a, i), true, &Default::default()))))
}

fn int(a: &[V], i: usize, f: &str) -> R<i64> {
    match arg(a, i) {
        V::Int(n) => Ok(*n),
        other => err(format!("{f}: cần số nguyên, nhận {}", to_text(other, true, &Default::default()))),
    }
}

fn string<'a>(a: &'a [V], i: usize, f: &str) -> R<&'a str> {
    match arg(a, i) {
        V::Str(s) => Ok(s),
        other => err(format!("{f}: cần chuỗi, nhận {}", to_text(other, true, &Default::default()))),
    }
}

fn point(a: &[V], i: usize, f: &str) -> R<Vec2> {
    arg(a, i).point().ok_or_else(|| Ex::Error(format!("{f}: cần một điểm")))
}

/// Numbers keep integer arithmetic until a real appears (AutoLISP rules).
fn arith(a: &[V], f: &str, op: char) -> R<V> {
    if a.is_empty() {
        return Ok(V::Int(if op == '*' { 1 } else { 0 }));
    }
    let all_int = a.iter().all(|v| matches!(v, V::Int(_)));
    if all_int {
        let ns: Vec<i64> = a.iter().filter_map(|v| if let V::Int(n) = v { Some(*n) } else { None }).collect();
        let first = ns.first().copied().unwrap_or(0);
        if ns.len() == 1 {
            return Ok(V::Int(match op {
                '-' => first.wrapping_neg(),
                '/' => first,
                _ => first,
            }));
        }
        let mut acc = first;
        for n in ns.iter().skip(1) {
            acc = match op {
                '+' => acc.wrapping_add(*n),
                '-' => acc.wrapping_sub(*n),
                '*' => acc.wrapping_mul(*n),
                _ => {
                    if *n == 0 {
                        return err("chia cho 0");
                    }
                    acc.wrapping_div(*n)
                }
            };
        }
        return Ok(V::Int(acc));
    }
    let mut ns = Vec::with_capacity(a.len());
    for i in 0..a.len() {
        ns.push(num(a, i, f)?);
    }
    let first = ns.first().copied().unwrap_or(0.0);
    if ns.len() == 1 {
        return Ok(V::Real(if op == '-' { -first } else { first }));
    }
    let mut acc = first;
    for n in ns.iter().skip(1) {
        acc = match op {
            '+' => acc + n,
            '-' => acc - n,
            '*' => acc * n,
            _ => {
                if *n == 0.0 {
                    return err("chia cho 0");
                }
                acc / n
            }
        };
    }
    Ok(V::Real(acc))
}

fn cmp_vals(a: &V, b: &V) -> Option<std::cmp::Ordering> {
    match (a, b) {
        (V::Str(x), V::Str(y)) => Some(x.cmp(y)),
        _ => a.num()?.partial_cmp(&b.num()?),
    }
}

/// `equal` with an optional fuzz for numbers.
pub fn equal(a: &V, b: &V, fuzz: f64) -> bool {
    match (a, b) {
        (V::List(x), V::List(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| equal(p, q, fuzz)),
        (V::Dotted(x, xt), V::Dotted(y, yt)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| equal(p, q, fuzz)) && equal(xt, yt, fuzz),
        _ => match (a.num(), b.num()) {
            (Some(x), Some(y)) => (x - y).abs() <= fuzz,
            _ => a == b,
        },
    }
}

/// AutoCAD wildcard match (`#` digit, `@` letter, `.` non-alphanumeric, `*`, `?`, `~` not,
/// `[…]`, `` ` `` escape, comma = alternatives); case-insensitive.
pub fn wcmatch(s: &str, pat: &str) -> bool {
    let s: Vec<char> = s.to_uppercase().chars().collect();
    split_alternatives(pat).iter().any(|p| {
        let p: Vec<char> = p.to_uppercase().chars().collect();
        if p.first() == Some(&'~') { !wm(&s, p.get(1..).unwrap_or(&[])) } else { wm(&s, &p) }
    })
}

fn split_alternatives(p: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut esc = false;
    for c in p.chars() {
        if esc {
            if let Some(l) = out.last_mut() {
                l.push('`');
                l.push(c);
            }
            esc = false;
        } else if c == '`' {
            esc = true;
        } else if c == ',' {
            out.push(String::new());
        } else if let Some(l) = out.last_mut() {
            l.push(c);
        }
    }
    out
}

fn wm(s: &[char], p: &[char]) -> bool {
    let Some(&pc) = p.first() else { return s.is_empty() };
    let rest = p.get(1..).unwrap_or(&[]);
    match pc {
        '*' => (0..=s.len()).any(|k| wm(s.get(k..).unwrap_or(&[]), rest)),
        '`' => s.first() == p.get(1) && !s.is_empty() && wm(s.get(1..).unwrap_or(&[]), p.get(2..).unwrap_or(&[])),
        '[' => {
            let Some(&c) = s.first() else { return false };
            let Some(end) = p.iter().position(|x| *x == ']') else { return false };
            let set = p.get(1..end).unwrap_or(&[]);
            let (neg, set) = if set.first() == Some(&'~') { (true, set.get(1..).unwrap_or(&[])) } else { (false, set) };
            let mut hit = false;
            let mut i = 0;
            while i < set.len() {
                if set.get(i + 1) == Some(&'-')
                    && let (Some(a), Some(b)) = (set.get(i), set.get(i + 2))
                {
                    hit |= *a <= c && c <= *b;
                    i += 3;
                } else {
                    hit |= set.get(i) == Some(&c);
                    i += 1;
                }
            }
            hit != neg && wm(s.get(1..).unwrap_or(&[]), p.get(end + 1..).unwrap_or(&[]))
        }
        _ => {
            let Some(&c) = s.first() else { return false };
            let ok = match pc {
                '?' => true,
                '#' => c.is_ascii_digit(),
                '@' => c.is_alphabetic(),
                '.' => !c.is_alphanumeric(),
                x => x == c,
            };
            ok && wm(s.get(1..).unwrap_or(&[]), rest)
        }
    }
}

/// `rtos` / number formatting: mode 2 (decimal) unless another is asked.
pub fn rtos(v: f64, mode: i64, prec: i64) -> String {
    let prec = prec.clamp(0, 15) as usize;
    match mode {
        1 => format!("{v:.prec$E}"),
        _ => format!("{v:.prec$}"),
    }
}

fn substr(s: &str, start: i64, len: Option<i64>) -> String {
    let chars: Vec<char> = s.chars().collect();
    let st = (start.max(1) - 1) as usize;
    let end = match len {
        Some(l) => (st + l.max(0) as usize).min(chars.len()),
        None => chars.len(),
    };
    chars.get(st.min(chars.len())..end.max(st.min(chars.len()))).map(|c| c.iter().collect()).unwrap_or_default()
}

fn cxr(v: &V, path: &str) -> R<V> {
    // path read right to left: "ad" in cadr → cdr then car.
    let mut cur = v.clone();
    for op in path.chars().rev() {
        cur = match (op, &cur) {
            (_, V::Nil) => V::Nil,
            ('a', V::List(x)) | ('a', V::Dotted(x, _)) => x.first().cloned().unwrap_or(V::Nil),
            ('d', V::List(x)) => V::list(x.get(1..).unwrap_or(&[]).to_vec()),
            ('d', V::Dotted(x, t)) => {
                if x.len() > 1 {
                    V::Dotted(x.get(1..).unwrap_or(&[]).to_vec(), t.clone())
                } else {
                    (**t).clone()
                }
            }
            _ => return err("car/cdr: cần một danh sách"),
        };
    }
    Ok(cur)
}

impl Run<'_> {
    pub fn call_builtin(&mut self, name: &str, a: Vec<V>) -> R<V> {
        if let Some(r) = self.core(name, &a) {
            return r;
        }
        if let Some(r) = self.cad(name, &a) {
            return r;
        }
        err(format!("không có hàm: {}", name.to_ascii_uppercase()))
    }

    fn fn_list(&mut self, f: &V, lists: &[V]) -> R<Vec<V>> {
        let ls: Vec<&[V]> = lists.iter().map(|l| l.items().unwrap_or(&[])).collect();
        let n = ls.iter().map(|l| l.len()).min().unwrap_or(0);
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let args: Vec<V> = ls.iter().filter_map(|l| l.get(i).cloned()).collect();
            out.push(self.call(f, args)?);
        }
        Ok(out)
    }

    fn core(&mut self, name: &str, a: &[V]) -> Option<R<V>> {
        let r: R<V> = match name {
            "+" => arith(a, name, '+'),
            "-" => arith(a, name, '-'),
            "*" => arith(a, name, '*'),
            "/" => arith(a, name, '/'),
            "1+" => arith(&[arg(a, 0).clone(), V::Int(1)], name, '+'),
            "1-" => arith(&[arg(a, 0).clone(), V::Int(1)], name, '-'),
            "abs" => match arg(a, 0) {
                V::Int(n) => Ok(V::Int(n.wrapping_abs())),
                _ => num(a, 0, name).map(|x| V::Real(x.abs())),
            },
            "min" | "max" => {
                if a.is_empty() {
                    return Some(Ok(V::Int(0)));
                }
                let all_int = a.iter().all(|v| matches!(v, V::Int(_)));
                let mut best = match num(a, 0, name) {
                    Ok(v) => v,
                    Err(e) => return Some(Err(e)),
                };
                for i in 1..a.len() {
                    let v = match num(a, i, name) {
                        Ok(v) => v,
                        Err(e) => return Some(Err(e)),
                    };
                    if (name == "min" && v < best) || (name == "max" && v > best) {
                        best = v;
                    }
                }
                Ok(if all_int { V::Int(best as i64) } else { V::Real(best) })
            }
            "rem" => match (arg(a, 0), arg(a, 1)) {
                (V::Int(x), V::Int(y)) if *y != 0 => Ok(V::Int(x % y)),
                _ => match (num(a, 0, name), num(a, 1, name)) {
                    (Ok(x), Ok(y)) if y != 0.0 => Ok(V::Real(x % y)),
                    (Ok(_), Ok(_)) => err("chia cho 0"),
                    (Err(e), _) | (_, Err(e)) => Err(e),
                },
            },
            "gcd" => match (int(a, 0, name), int(a, 1, name)) {
                (Ok(mut x), Ok(mut y)) => {
                    while y != 0 {
                        let t = x % y;
                        x = y;
                        y = t;
                    }
                    Ok(V::Int(x.abs()))
                }
                (Err(e), _) | (_, Err(e)) => Err(e),
            },
            "fix" => num(a, 0, name).map(|x| V::Int(x.trunc() as i64)),
            "float" => num(a, 0, name).map(V::Real),
            "sqrt" => num(a, 0, name).and_then(|x| if x < 0.0 { err("sqrt: số âm") } else { Ok(V::Real(x.sqrt())) }),
            "sin" => num(a, 0, name).map(|x| V::Real(x.sin())),
            "cos" => num(a, 0, name).map(|x| V::Real(x.cos())),
            "atan" => match a.len() {
                0 | 1 => num(a, 0, name).map(|x| V::Real(x.atan())),
                _ => match (num(a, 0, name), num(a, 1, name)) {
                    (Ok(y), Ok(x)) => Ok(V::Real(y.atan2(x))),
                    (Err(e), _) | (_, Err(e)) => Err(e),
                },
            },
            "exp" => num(a, 0, name).map(|x| V::Real(x.exp())),
            "log" => num(a, 0, name).and_then(|x| if x <= 0.0 { err("log: số không dương") } else { Ok(V::Real(x.ln())) }),
            "expt" => match (arg(a, 0), arg(a, 1)) {
                (V::Int(x), V::Int(y)) if *y >= 0 && *y < 64 => Ok(V::Int(x.wrapping_pow(*y as u32))),
                _ => match (num(a, 0, name), num(a, 1, name)) {
                    (Ok(x), Ok(y)) => Ok(V::Real(x.powf(y))),
                    (Err(e), _) | (_, Err(e)) => Err(e),
                },
            },
            "logand" | "logior" => {
                let mut acc: i64 = if name == "logand" { -1 } else { 0 };
                for i in 0..a.len() {
                    match int(a, i, name) {
                        Ok(n) => acc = if name == "logand" { acc & n } else { acc | n },
                        Err(e) => return Some(Err(e)),
                    }
                }
                Ok(V::Int(if a.is_empty() { 0 } else { acc }))
            }
            "~" => int(a, 0, name).map(|n| V::Int(!n)),
            "lsh" => match (int(a, 0, name), int(a, 1, name)) {
                (Ok(x), Ok(n)) => Ok(V::Int(if n >= 0 { x.wrapping_shl(n as u32) } else { x.wrapping_shr((-n) as u32) })),
                (Err(e), _) | (_, Err(e)) => Err(e),
            },
            "=" | "/=" | "<" | ">" | "<=" | ">=" => {
                use std::cmp::Ordering::*;
                if a.len() < 2 {
                    return Some(Ok(V::T));
                }
                if name == "/=" {
                    return Some(Ok(V::from_bool(cmp_vals(arg(a, 0), arg(a, 1)) != Some(Equal))));
                }
                let mut ok = true;
                for w in a.windows(2) {
                    let o = w.first().zip(w.get(1)).and_then(|(x, y)| cmp_vals(x, y));
                    let pass = matches!(
                        (name, o),
                        ("=", Some(Equal)) | ("<", Some(Less)) | (">", Some(Greater)) | ("<=", Some(Less | Equal)) | (">=", Some(Greater | Equal))
                    );
                    ok &= pass;
                }
                Ok(V::from_bool(ok))
            }
            "eq" => Ok(V::from_bool(arg(a, 0) == arg(a, 1))),
            "equal" => Ok(V::from_bool(equal(arg(a, 0), arg(a, 1), arg(a, 2).num().unwrap_or(0.0)))),
            "not" | "null" => Ok(V::from_bool(!arg(a, 0).truthy())),
            "minusp" => num(a, 0, name).map(|x| V::from_bool(x < 0.0)),
            "zerop" => num(a, 0, name).map(|x| V::from_bool(x == 0.0)),
            "numberp" => Ok(V::from_bool(arg(a, 0).num().is_some())),
            "listp" => Ok(V::from_bool(matches!(arg(a, 0), V::Nil | V::List(_) | V::Dotted(..)))),
            "vl-consp" => Ok(V::from_bool(matches!(arg(a, 0), V::List(_) | V::Dotted(..)))),
            "atom" => Ok(V::from_bool(!matches!(arg(a, 0), V::List(_) | V::Dotted(..)))),
            "boundp" => Ok(V::from_bool(matches!(arg(a, 0), V::Sym(s) if self.lookup(s).truthy()))),
            "type" => Ok(match arg(a, 0) {
                V::Nil => V::Nil,
                v => V::Sym(v.type_name().to_ascii_lowercase()),
            }),
            // Lists.
            "car" => cxr(arg(a, 0), "a"),
            "cdr" => cxr(arg(a, 0), "d"),
            "cadr" => cxr(arg(a, 0), "ad"),
            "cddr" => cxr(arg(a, 0), "dd"),
            "caar" => cxr(arg(a, 0), "aa"),
            "cdar" => cxr(arg(a, 0), "da"),
            "caddr" => cxr(arg(a, 0), "add"),
            "cadar" => cxr(arg(a, 0), "ada"),
            "cdddr" => cxr(arg(a, 0), "ddd"),
            "caadr" => cxr(arg(a, 0), "aad"),
            "cddar" => cxr(arg(a, 0), "dda"),
            "cadddr" => cxr(arg(a, 0), "addd"),
            "list" => Ok(V::list(a.to_vec())),
            "cons" => Ok(V::pair(arg(a, 0).clone(), arg(a, 1).clone())),
            "length" => match arg(a, 0) {
                V::Nil => Ok(V::Int(0)),
                V::List(v) => Ok(V::Int(v.len() as i64)),
                _ => err("length: cần danh sách"),
            },
            "nth" => match (int(a, 0, name), arg(a, 1).items()) {
                (Ok(i), Some(items)) => Ok(usize::try_from(i).ok().and_then(|i| items.get(i)).cloned().unwrap_or(V::Nil)),
                (Err(e), _) => Err(e),
                _ => err("nth: cần danh sách"),
            },
            "last" => Ok(arg(a, 0).items().and_then(<[V]>::last).cloned().unwrap_or(V::Nil)),
            "reverse" => Ok(V::list(arg(a, 0).items().map(|v| v.iter().rev().cloned().collect()).unwrap_or_default())),
            "append" => {
                let mut out = Vec::new();
                for l in a {
                    match l.items() {
                        Some(v) => out.extend_from_slice(v),
                        None => return Some(err("append: cần danh sách")),
                    }
                }
                Ok(V::list(out))
            }
            "member" => {
                let items = arg(a, 1).items().unwrap_or(&[]);
                Ok(items.iter().position(|x| equal(x, arg(a, 0), 0.0)).map_or(V::Nil, |i| V::list(items.get(i..).unwrap_or(&[]).to_vec())))
            }
            "assoc" => {
                let key = arg(a, 0);
                let items = arg(a, 1).items().unwrap_or(&[]);
                Ok(items
                    .iter()
                    .find(|e| match e {
                        V::List(x) | V::Dotted(x, _) => x.first().is_some_and(|k| equal(k, key, 0.0)),
                        _ => false,
                    })
                    .cloned()
                    .unwrap_or(V::Nil))
            }
            "subst" => {
                let (new, old) = (arg(a, 0), arg(a, 1));
                fn sub(v: &V, old: &V, new: &V) -> V {
                    if equal(v, old, 0.0) {
                        return new.clone();
                    }
                    match v {
                        V::List(x) => V::List(x.iter().map(|y| sub(y, old, new)).collect()),
                        V::Dotted(x, t) => V::Dotted(x.iter().map(|y| sub(y, old, new)).collect(), Box::new(sub(t, old, new))),
                        o => o.clone(),
                    }
                }
                Ok(sub(arg(a, 2), old, new))
            }
            "vl-position" => Ok(arg(a, 1).items().and_then(|v| v.iter().position(|x| equal(x, arg(a, 0), 0.0))).map_or(V::Nil, |i| V::Int(i as i64))),
            "vl-remove" => Ok(V::list(arg(a, 1).items().unwrap_or(&[]).iter().filter(|x| !equal(x, arg(a, 0), 0.0)).cloned().collect())),
            "vl-list*" => {
                let mut items: Vec<V> = a.to_vec();
                let tail = items.pop().unwrap_or(V::Nil);
                let mut out = tail;
                for x in items.into_iter().rev() {
                    out = V::pair(x, out);
                }
                Ok(out)
            }
            "mapcar" => {
                let f = arg(a, 0).clone();
                return Some(self.fn_list(&f, a.get(1..).unwrap_or(&[])).map(V::list));
            }
            "apply" => {
                let f = arg(a, 0).clone();
                let args = arg(a, 1).items().map(<[V]>::to_vec).unwrap_or_default();
                return Some(self.call(&f, args));
            }
            "vl-remove-if" | "vl-remove-if-not" | "vl-member-if" | "vl-some" | "vl-every" => {
                let f = arg(a, 0).clone();
                let items = arg(a, 1).items().map(<[V]>::to_vec).unwrap_or_default();
                let mut out = Vec::new();
                for (i, x) in items.iter().enumerate() {
                    let t = match self.call(&f, vec![x.clone()]) {
                        Ok(t) => t.truthy(),
                        Err(e) => return Some(Err(e)),
                    };
                    match name {
                        "vl-remove-if" if !t => out.push(x.clone()),
                        "vl-remove-if-not" if t => out.push(x.clone()),
                        "vl-member-if" if t => return Some(Ok(V::list(items.get(i..).unwrap_or(&[]).to_vec()))),
                        "vl-some" if t => return Some(Ok(V::T)),
                        "vl-every" if !t => return Some(Ok(V::Nil)),
                        _ => {}
                    }
                }
                Ok(match name {
                    "vl-every" => V::T,
                    "vl-some" | "vl-member-if" => V::Nil,
                    _ => V::list(out),
                })
            }
            "vl-sort" => {
                let f = arg(a, 1).clone();
                let mut items = arg(a, 0).items().map(<[V]>::to_vec).unwrap_or_default();
                // Insertion sort through the user's predicate (stable, errors propagate).
                let mut i = 1;
                while i < items.len() {
                    let mut j = i;
                    while j > 0 {
                        let (Some(x), Some(y)) = (items.get(j).cloned(), items.get(j - 1).cloned()) else { break };
                        match self.call(&f, vec![x, y]) {
                            Ok(t) if t.truthy() => items.swap(j, j - 1),
                            Ok(_) => break,
                            Err(e) => return Some(Err(e)),
                        }
                        j -= 1;
                    }
                    i += 1;
                    if i > 20_000 {
                        break;
                    }
                }
                Ok(V::list(items))
            }
            // Strings.
            "strcat" => {
                let mut out = String::new();
                for i in 0..a.len() {
                    match string(a, i, name) {
                        Ok(s) => out.push_str(s),
                        Err(e) => return Some(Err(e)),
                    }
                }
                Ok(V::Str(out))
            }
            "strlen" => {
                let mut n = 0;
                for i in 0..a.len() {
                    match string(a, i, name) {
                        Ok(s) => n += s.chars().count(),
                        Err(e) => return Some(Err(e)),
                    }
                }
                Ok(V::Int(n as i64))
            }
            "substr" => match (string(a, 0, name), int(a, 1, name)) {
                (Ok(s), Ok(st)) => Ok(V::Str(substr(s, st, if let V::Int(l) = arg(a, 2) { Some(*l) } else { None }))),
                (Err(e), _) | (_, Err(e)) => Err(e),
            },
            "strcase" => string(a, 0, name).map(|s| V::Str(if arg(a, 1).truthy() { s.to_lowercase() } else { s.to_uppercase() })),
            "ascii" => string(a, 0, name).map(|s| V::Int(s.chars().next().map_or(0, |c| i64::from(u32::from(c))))),
            "chr" => int(a, 0, name).map(|n| V::Str(char::from_u32(n as u32).map(String::from).unwrap_or_default())),
            "atoi" => string(a, 0, name).map(|s| {
                let t: String = s
                    .trim()
                    .chars()
                    .enumerate()
                    .take_while(|(i, c)| c.is_ascii_digit() || (*i == 0 && (*c == '-' || *c == '+')))
                    .map(|x| x.1)
                    .collect();
                V::Int(t.parse().unwrap_or(0))
            }),
            "atof" => string(a, 0, name).map(|s| {
                let t = s.trim();
                let end = (1..=t.len()).rev().find(|k| t.get(..*k).is_some_and(|p| p.parse::<f64>().is_ok())).unwrap_or(0);
                V::Real(t.get(..end).and_then(|p| p.parse().ok()).unwrap_or(0.0))
            }),
            "itoa" => int(a, 0, name).map(|n| V::Str(n.to_string())),
            "rtos" => num(a, 0, name).map(|x| {
                let mode = if let V::Int(m) = arg(a, 1) { *m } else { 2 };
                let prec = if let V::Int(p) = arg(a, 2) { *p } else { 4 };
                V::Str(rtos(x, mode, prec))
            }),
            "angtos" => num(a, 0, name).map(|x| {
                let prec = if let V::Int(p) = arg(a, 2) { *p } else { 0 };
                let mode = if let V::Int(m) = arg(a, 1) { *m } else { 0 };
                let d = cadcraft_geom::norm_angle(x).to_degrees();
                V::Str(match mode {
                    3 => rtos(cadcraft_geom::norm_angle(x), 2, prec),
                    1 => {
                        let deg = d.floor();
                        let m = ((d - deg) * 60.0).floor();
                        let sec = ((d - deg) * 60.0 - m) * 60.0;
                        format!("{deg}d{m}'{}\"", rtos(sec, 2, prec))
                    }
                    _ => rtos(d, 2, prec),
                })
            }),
            "distof" => string(a, 0, name).map(|s| crate::units::parse_distance(s).map_or(V::Nil, V::Real)),
            "angtof" => string(a, 0, name).map(|s| crate::units::parse_angle(s).map_or(V::Nil, V::Real)),
            "wcmatch" => match (string(a, 0, name), string(a, 1, name)) {
                (Ok(s), Ok(p)) => Ok(V::from_bool(wcmatch(s, p))),
                (Err(e), _) | (_, Err(e)) => Err(e),
            },
            "vl-string-search" => match (string(a, 0, name), string(a, 1, name)) {
                (Ok(pat), Ok(s)) => {
                    let start = if let V::Int(n) = arg(a, 2) { (*n).max(0) as usize } else { 0 };
                    let chars: Vec<char> = s.chars().collect();
                    let hay: String = chars.get(start.min(chars.len())..).map(|c| c.iter().collect()).unwrap_or_default();
                    Ok(hay.find(pat).map_or(V::Nil, |b| V::Int((start + hay.get(..b).map_or(0, |p| p.chars().count())) as i64)))
                }
                (Err(e), _) | (_, Err(e)) => Err(e),
            },
            "vl-string-subst" => match (string(a, 0, name), string(a, 1, name), string(a, 2, name)) {
                (Ok(new), Ok(pat), Ok(s)) => Ok(V::Str(s.replacen(pat, new, 1))),
                (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => Err(e),
            },
            "vl-string-trim" | "vl-string-left-trim" | "vl-string-right-trim" => match (string(a, 0, name), string(a, 1, name)) {
                (Ok(set), Ok(s)) => {
                    let set: Vec<char> = set.chars().collect();
                    Ok(V::Str(match name {
                        "vl-string-left-trim" => s.trim_start_matches(|c| set.contains(&c)).to_string(),
                        "vl-string-right-trim" => s.trim_end_matches(|c| set.contains(&c)).to_string(),
                        _ => s.trim_matches(|c| set.contains(&c)).to_string(),
                    }))
                }
                (Err(e), _) | (_, Err(e)) => Err(e),
            },
            "vl-string->list" => string(a, 0, name).map(|s| V::list(s.chars().map(|c| V::Int(i64::from(u32::from(c)))).collect())),
            "vl-list->string" => Ok(V::Str(
                arg(a, 0).items().unwrap_or(&[]).iter().filter_map(|v| if let V::Int(n) = v { char::from_u32(*n as u32) } else { None }).collect(),
            )),
            "vl-princ-to-string" => Ok(V::Str(to_text(arg(a, 0), false, &self.lisp.sets))),
            "vl-prin1-to-string" => Ok(V::Str(to_text(arg(a, 0), true, &self.lisp.sets))),
            "vl-symbol-name" => match arg(a, 0) {
                V::Sym(s) => Ok(V::Str(s.to_ascii_uppercase())),
                _ => err("vl-symbol-name: cần ký hiệu"),
            },
            "read" => string(a, 0, name).and_then(|s| super::read_all(s).map_err(Ex::Error)).map(|f| f.into_iter().next().unwrap_or(V::Nil)),
            "eval" => {
                let x = arg(a, 0).clone();
                return Some(self.eval(&x));
            }
            "set" => match arg(a, 0) {
                V::Sym(n) => {
                    let n = n.clone();
                    self.set(&n, arg(a, 1).clone());
                    Ok(arg(a, 1).clone())
                }
                _ => err("set: cần ký hiệu"),
            },
            "vl-load-com" | "vl-load-reactors" | "textscr" | "graphscr" | "redraw" | "gc" => Ok(V::Nil),
            "exit" | "quit" => Err(Ex::Quit),
            "*error*" => Ok(V::Nil),
            // Output.
            "princ" | "prin1" | "print" => {
                let Some(v) = a.first() else { return Some(Ok(V::Sym(String::new()))) };
                let text = to_text(v, name != "princ", &self.lisp.sets);
                if name == "print" {
                    self.print("\n");
                }
                self.print(&text);
                if name == "print" {
                    self.print(" ");
                }
                Ok(v.clone())
            }
            "prompt" => string(a, 0, name).map(|s| {
                let s = s.to_string();
                self.print(&s);
                V::Nil
            }),
            "terpri" => {
                self.print("\n");
                Ok(V::Nil)
            }
            "alert" => string(a, 0, name).map(|s| {
                let s = format!("\n[Thông báo] {s}\n");
                self.print(&s);
                V::Nil
            }),
            // Geometry.
            "distance" => match (point(a, 0, name), point(a, 1, name)) {
                (Ok(p), Ok(q)) => Ok(V::Real(p.dist(q))),
                (Err(e), _) | (_, Err(e)) => Err(e),
            },
            "angle" => match (point(a, 0, name), point(a, 1, name)) {
                (Ok(p), Ok(q)) => Ok(V::Real(cadcraft_geom::norm_angle((q - p).angle()))),
                (Err(e), _) | (_, Err(e)) => Err(e),
            },
            "polar" => match (point(a, 0, name), num(a, 1, name), num(a, 2, name)) {
                (Ok(p), Ok(ang), Ok(d)) => {
                    let q = p + Vec2::from_angle(ang) * d;
                    let z = arg(a, 0).items().and_then(|v| v.get(2)).and_then(V::num).unwrap_or(0.0);
                    Ok(V::pt3(q.x, q.y, z))
                }
                (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => Err(e),
            },
            "inters" => match (point(a, 0, name), point(a, 1, name), point(a, 2, name), point(a, 3, name)) {
                (Ok(p1), Ok(p2), Ok(p3), Ok(p4)) => {
                    let on_seg = a.len() < 5 || arg(a, 4).truthy();
                    let d1 = p2 - p1;
                    let d2 = p4 - p3;
                    let den = d1.cross(d2);
                    if den.abs() < 1e-14 {
                        return Some(Ok(V::Nil));
                    }
                    let t = (p3 - p1).cross(d2) / den;
                    let u = (p3 - p1).cross(d1) / den;
                    if on_seg && !((-1e-12..=1.0 + 1e-12).contains(&t) && (-1e-12..=1.0 + 1e-12).contains(&u)) {
                        return Some(Ok(V::Nil));
                    }
                    let q = p1 + d1 * t;
                    Ok(V::pt3(q.x, q.y, 0.0))
                }
                _ => err("inters: cần 4 điểm"),
            },
            _ => return None,
        };
        Some(r)
    }
}
