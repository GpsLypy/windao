//! A bounded, non-executing parser. Only accepted expressions become numeric operations.
use super::{Auth, Extractor, Headers, Op, Result, UNSUPPORTED, label};
use std::collections::HashSet;

pub(super) fn tokens(mut src: &str) -> Result<Vec<&str>> {
    let mut result = Vec::new();
    while !src.is_empty() {
        src = src.trim_start();
        if src.is_empty() {
            break;
        }
        if let Some(after) = src.strip_prefix("//") {
            src = after.split_once(['\n', '\r']).map_or("", |(_, rest)| rest);
            continue;
        }
        if let Some(after) = src.strip_prefix("/*") {
            src = after.split_once("*/").ok_or(UNSUPPORTED)?.1;
            continue;
        }
        if result.len() >= 1024 {
            return Err(UNSUPPORTED);
        }
        let first = src.as_bytes()[0];
        let len = if first == b'"' || first == b'\'' {
            let end = src[1..].find(first as char).ok_or(UNSUPPORTED)? + 1;
            if src[1..end].contains('\\') || src[1..end].chars().any(char::is_control) {
                return Err(UNSUPPORTED);
            }
            end + 1
        } else if first.is_ascii_alphanumeric() || first == b'_' {
            src.bytes()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == b'_')
                .count()
        } else if first.is_ascii() {
            1
        } else {
            return Err(UNSUPPORTED);
        };
        let (part, rest) = src.split_at(len);
        result.push(part);
        src = rest;
    }
    Ok(result)
}
fn string(token: &str) -> Option<&str> {
    (token.starts_with(['\'', '"']) && token.len() >= 2).then(|| &token[1..token.len() - 1])
}
pub(super) fn preset(input: &[&str], reference: &str) -> bool {
    // Only translated fallback labels vary in the shipped New API / Sub2API presets.
    let Ok(reference) = tokens(reference) else {
        return false;
    };
    let canonical = |tokens: &[&str]| -> Vec<String> {
        tokens
            .iter()
            .enumerate()
            .filter(|(i, t)| {
                !(**t == "," && tokens.get(i + 1) == Some(&"}")
                    || **t == ";" && *i + 1 == tokens.len())
            })
            .map(|(_, t)| string(t).map_or_else(|| t.to_string(), |s| format!("'{s}")))
            .collect()
    };
    let (input, reference) = (canonical(input), canonical(&reference));
    input.len() == reference.len()
        && input.iter().zip(reference).all(|(actual, expected)| {
            actual == &expected
                || matches!(expected.as_str(), "'Default Plan" | "'Query failed")
                    && actual.starts_with('\'')
        })
}
struct Parser<'a> {
    tokens: &'a [&'a str],
}
impl<'a> Parser<'a> {
    fn next(&mut self) -> Result<&'a str> {
        let (t, rest) = self.tokens.split_first().ok_or(UNSUPPORTED)?;
        self.tokens = rest;
        Ok(t)
    }
    fn eat(&mut self, t: &str) -> bool {
        if self.tokens.first() == Some(&t) {
            self.tokens = &self.tokens[1..];
            true
        } else {
            false
        }
    }
    fn need(&mut self, t: &str) -> Result<()> {
        self.eat(t).then_some(()).ok_or(UNSUPPORTED)
    }
    fn literal(&mut self) -> Result<&'a str> {
        string(self.next()?).ok_or(UNSUPPORTED)
    }
    fn object(&mut self, mut property: impl FnMut(&mut Self, &'a str) -> Result<()>) -> Result<()> {
        self.need("{")?;
        let mut keys = HashSet::new();
        while !self.eat("}") {
            let key = self.next()?;
            let key = string(key).unwrap_or(key);
            if !keys.insert(key) {
                return Err(UNSUPPORTED);
            }
            self.need(":")?;
            property(self, key)?;
            if !self.eat(",") {
                self.need("}")?;
                break;
            }
        }
        Ok(())
    }
    fn request(&mut self) -> Result<(String, Headers, bool)> {
        let (mut url, mut auth, mut user, mut get) = (None, None, false, false);
        let (mut agent, mut json) = (false, false);
        self.object(|p, key| {
            match key {
                "url" => url = Some(p.literal()?.to_owned()),
                "method" if p.literal()? == "GET" => get = true,
                "headers" => p.object(|p, key| {
                    let v = p.literal()?;
                    match (key.to_ascii_lowercase().as_str(), v) {
                        ("authorization", "Bearer {{apiKey}}" | "Bearer {{accessToken}}")
                            if auth.is_none() =>
                        {
                            auth = Some(Auth::Bearer)
                        }
                        ("authorization", "{{apiKey}}" | "{{accessToken}}") if auth.is_none() => {
                            auth = Some(Auth::Raw)
                        }
                        ("x-api-key", "{{apiKey}}") if auth.is_none() => auth = Some(Auth::ApiKey),
                        ("new-api-user", "{{userId}}") if !user => user = true,
                        ("user-agent", "cc-switch/1.0") => agent = true,
                        ("content-type", "application/json") => json = true,
                        ("accept", "application/json") => {}
                        _ => return Err(UNSUPPORTED),
                    }
                    Ok(())
                })?,
                _ => return Err(UNSUPPORTED),
            }
            Ok(())
        })?;
        if !get {
            return Err(UNSUPPORTED);
        }
        Ok((
            url.ok_or(UNSUPPORTED)?,
            Headers {
                auth: auth.ok_or(UNSUPPORTED)?,
                agent,
                json,
            },
            user,
        ))
    }
    fn field(&mut self) -> Result<String> {
        self.need("response")?;
        let mut path = String::new();
        loop {
            let key = if self.eat(".") {
                self.next()?
            } else if self.eat("[") {
                let index = self.next()?;
                if !index.bytes().all(|c| c.is_ascii_digit()) {
                    return Err(UNSUPPORTED);
                }
                self.need("]")?;
                index
            } else {
                break;
            };
            if !key.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_') {
                return Err(UNSUPPORTED);
            }
            path.push('/');
            path.push_str(key);
        }
        if path.is_empty() || path.len() > 200 {
            return Err(UNSUPPORTED);
        }
        Ok(path)
    }
    fn expression(&mut self, min_precedence: u8, depth: u8, out: &mut Vec<Op>) -> Result<()> {
        if depth > 24 {
            return Err(UNSUPPORTED);
        }
        if self.eat("(") {
            self.expression(0, depth + 1, out)?;
            self.need(")")?;
        } else if self.tokens.first() == Some(&"response") {
            out.push(Op::Field(self.field()?));
        } else {
            let mut literal = self.next()?.to_string();
            if !literal.bytes().all(|c| c.is_ascii_digit()) {
                return Err(UNSUPPORTED);
            }
            if self.eat(".") {
                literal.push('.');
                let fraction = self.next()?;
                if !fraction.bytes().all(|c| c.is_ascii_digit()) {
                    return Err(UNSUPPORTED);
                }
                literal.push_str(fraction);
            }
            let n = literal
                .parse::<f64>()
                .ok()
                .filter(|n| n.is_finite())
                .ok_or(UNSUPPORTED)?;
            out.push(Op::Number(n));
        }
        while let Some(&token) = self.tokens.first() {
            let (precedence, op) = match token {
                "+" => (1, Op::Add),
                "-" => (1, Op::Sub),
                "*" => (2, Op::Mul),
                "/" => (2, Op::Div),
                _ => break,
            };
            if precedence < min_precedence {
                break;
            }
            self.next()?;
            self.expression(precedence + 1, depth + 1, out)?;
            out.push(op);
        }
        Ok(())
    }
    fn extractor(&mut self) -> Result<Extractor> {
        let function = self.eat("function");
        let parens = self.eat("(");
        self.need("response")?;
        if parens {
            self.need(")")?;
        }
        if !function {
            self.need("=")?;
            self.need(">")?;
        }
        let block = self.eat("{");
        if block {
            self.need("return")?;
        } else {
            self.need("(")?;
        }
        let (mut remaining, mut used, mut total, mut unit, mut valid) =
            (None, None, None, None, None);
        self.object(|p, key| {
            match key {
                "remaining" | "used" | "total" => {
                    let mut ops = Vec::new();
                    p.expression(0, 0, &mut ops)?;
                    if !ops.iter().any(|op| matches!(op, Op::Field(_))) {
                        return Err(UNSUPPORTED);
                    }
                    *match key {
                        "remaining" => &mut remaining,
                        "used" => &mut used,
                        _ => &mut total,
                    } = Some(ops);
                }
                "unit" => unit = Some(label(p.literal()?, 12)?.to_owned()),
                "isValid" => {
                    if !p.eat("true") {
                        let path = p.field()?;
                        if p.eat("|") {
                            p.need("|")?;
                            p.need("true")?;
                        } else {
                            valid = Some(path);
                        }
                    }
                }
                _ => return Err(UNSUPPORTED),
            }
            Ok(())
        })?;
        if block {
            self.eat(";");
            self.need("}")?;
        } else {
            self.need(")")?;
        }
        Ok(Extractor::Fields {
            remaining: remaining.ok_or(UNSUPPORTED)?,
            used,
            total,
            unit: unit.ok_or(UNSUPPORTED)?,
            valid,
        })
    }
}
pub(super) fn custom(tokens: &[&str]) -> Result<(String, Headers, bool, Extractor)> {
    let mut p = Parser { tokens };
    let wrapped = p.eat("(");
    let (mut request, mut extractor) = (None, None);
    p.object(|p, key| {
        match key {
            "request" => request = Some(p.request()?),
            "extractor" => extractor = Some(p.extractor()?),
            _ => return Err(UNSUPPORTED),
        }
        Ok(())
    })?;
    if wrapped {
        p.need(")")?;
    }
    p.eat(";");
    if !p.tokens.is_empty() {
        return Err(UNSUPPORTED);
    }
    let (url, auth, user) = request.ok_or(UNSUPPORTED)?;
    Ok((url, auth, user, extractor.ok_or(UNSUPPORTED)?))
}
