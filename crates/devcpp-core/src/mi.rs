//! GDB/Machine-Interface value parser.
//!
//! GDB/MI records look like:
//!
//! ```text
//! &"GNU gdb 18.1"
//! =breakpoint-created,bkpt={number="1",file="main.c",line="3"}
//! *stopped,reason="breakpoint-hit",frame={addr="0x...",func="main",line="3"}
//! ^done,stack=[frame={level="0",func="main",line="3"}],variables=[{name="x",value="1"}]
//! ```
//!
//! The grammar is small: a record is a list of `key=value` pairs, where a
//! value is a string, a tuple (`{...}`) or a list (`[...]`). List items may
//! themselves be `key=value` pairs (that is how GDB encodes stack frames).

use std::collections::BTreeMap;

/// A parsed MI value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// A quoted string.
    Str(String),
    /// A tuple: ordered key/value pairs.
    Tuple(BTreeMap<String, Value>),
    /// A list of values.
    List(Vec<Value>),
}

impl Value {
    /// Returns the string content, if this is a string.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s),
            _ => None,
        }
    }

    /// Returns the tuple content, if this is a tuple.
    #[must_use]
    pub fn as_tuple(&self) -> Option<&BTreeMap<String, Value>> {
        match self {
            Self::Tuple(t) => Some(t),
            _ => None,
        }
    }

    /// Returns the list content, if this is a list.
    #[must_use]
    pub fn as_list(&self) -> Option<&[Value]> {
        match self {
            Self::List(l) => Some(l),
            _ => None,
        }
    }

    /// Looks up a key inside a tuple.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.as_tuple().and_then(|t| t.get(key))
    }

    /// Looks up a key inside a tuple and returns it as a string.
    #[must_use]
    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Value::as_str)
    }

    /// Looks up a nested key path.
    #[must_use]
    pub fn path(&self, keys: &[&str]) -> Option<&Value> {
        let mut current = self;
        for key in keys {
            current = current.get(key)?;
        }
        Some(current)
    }
}

/// A parsed MI record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// Record type: `=`, `*`, `+`, `&` or `^`.
    pub kind: char,
    /// Record class, e.g. `done`, `running`, `stopped`, `breakpoint-created`.
    pub class: String,
    /// Key/value pairs of the record.
    pub fields: BTreeMap<String, Value>,
}

impl Record {
    /// Looks up a top-level field.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.fields.get(key)
    }

    /// Looks up a top-level field and returns it as a string.
    #[must_use]
    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Value::as_str)
    }

    /// Looks up a nested key path.
    #[must_use]
    pub fn path(&self, keys: &[&str]) -> Option<&Value> {
        let mut current = self.get(keys.first()?)?;
        for key in &keys[1..] {
            current = current.get(key)?;
        }
        Some(current)
    }
}

/// Cursor over the record text.
struct Scanner {
    chars: Vec<char>,
    pos: usize,
}

impl Scanner {
    fn new(text: &str) -> Self {
        Self {
            chars: text.chars().collect(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn next(&mut self) -> Option<char> {
        let ch = self.peek();
        if ch.is_some() {
            self.pos += 1;
        }
        ch
    }

    fn eat(&mut self, ch: char) -> bool {
        if self.peek() == Some(ch) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(c) if c.is_whitespace()) {
            self.pos += 1;
        }
    }

    /// Reads an identifier-like token (letters, digits, `_`, `-`, `.`).
    fn read_token(&mut self) -> String {
        let start = self.pos;
        while let Some(ch) = self.peek() {
            if ch.is_alphanumeric() || ch == '_' || ch == '-' || ch == '.' || ch == '*' {
                self.pos += 1;
            } else {
                break;
            }
        }
        self.chars[start..self.pos].iter().collect()
    }

    fn read_quoted(&mut self) -> String {
        let mut out = String::new();
        // The opening quote is consumed by the caller.
        while let Some(ch) = self.next() {
            match ch {
                '"' => break,
                '\\' => match self.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some('r') => out.push('\r'),
                    Some('\\') => out.push('\\'),
                    Some('"') => out.push('"'),
                    Some('0') => out.push('\0'),
                    Some(other) => {
                        out.push('\\');
                        out.push(other);
                    }
                    None => break,
                },
                other => out.push(other),
            }
        }
        out
    }

    fn read_value(&mut self) -> Value {
        self.skip_ws();
        match self.peek() {
            Some('"') => {
                self.pos += 1;
                Value::Str(self.read_quoted())
            }
            Some('{') => {
                self.pos += 1;
                Value::Tuple(self.read_pairs('}'))
            }
            Some('[') => {
                self.pos += 1;
                Value::List(self.read_items(']'))
            }
            _ => {
                let token = self.read_token();
                Value::Str(token)
            }
        }
    }

    /// Reads `key=value` pairs until `close` (consumed) or end of input.
    fn read_pairs(&mut self, close: char) -> BTreeMap<String, Value> {
        let mut map = BTreeMap::new();
        loop {
            self.skip_ws();
            if self.peek().is_none() {
                break;
            }
            if self.eat(close) {
                break;
            }
            if self.eat(',') {
                continue;
            }
            let key = self.read_token();
            if key.is_empty() {
                // Unexpected character: skip it to avoid an infinite loop.
                self.pos += 1;
                continue;
            }
            self.skip_ws();
            if !self.eat('=') {
                continue;
            }
            let value = self.read_value();
            map.insert(key, value);
            self.skip_ws();
            self.eat(',');
        }
        map
    }

    /// Reads list items until `close` (consumed) or end of input.
    ///
    /// Items are either plain values or `key=value` pairs; the latter are
    /// stored as one-field tuples so that callers can treat them uniformly.
    fn read_items(&mut self, close: char) -> Vec<Value> {
        let mut items = Vec::new();
        loop {
            self.skip_ws();
            if self.peek().is_none() {
                break;
            }
            if self.eat(close) {
                break;
            }
            if self.eat(',') {
                continue;
            }
            // Look ahead for `key=`.
            let save = self.pos;
            let key = self.read_token();
            self.skip_ws();
            if !key.is_empty() && self.peek() == Some('=') {
                self.pos += 1;
                let value = self.read_value();
                let mut tuple = BTreeMap::new();
                tuple.insert(key, value);
                items.push(Value::Tuple(tuple));
            } else {
                self.pos = save;
                items.push(self.read_value());
            }
            self.skip_ws();
            self.eat(',');
        }
        items
    }
}

/// Parses one MI output line.
///
/// Returns `None` for lines that are not MI records (for example the greeting
/// printed by GDB at startup).
#[must_use]
pub fn parse(line: &str) -> Option<Record> {
    let mut scanner = Scanner::new(line);
    let kind = scanner.next()?;
    if !matches!(kind, '=' | '*' | '+' | '&' | '^') {
        return None;
    }
    scanner.skip_ws();

    // A result record carries its payload as a quoted string: &"GNU gdb 18.1".
    if scanner.peek() == Some('"') {
        scanner.pos += 1;
        let class = scanner.read_quoted();
        return Some(Record {
            kind,
            class,
            fields: BTreeMap::new(),
        });
    }

    let class = scanner.read_token();
    let fields = scanner.read_pairs('\0');
    Some(Record {
        kind,
        class,
        fields,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn parses_result_record() {
        let r = parse("&\"GNU gdb 18.1\"").expect("record");
        assert_eq!(r.kind, '&');
        assert_eq!(r.class, "GNU gdb 18.1");
    }

    #[test]
    fn parses_breakpoint_created() {
        let r = parse("=breakpoint-created,bkpt={number=\"1\",file=\"main.c\",line=\"3\"}")
            .expect("record");
        assert_eq!(r.kind, '=');
        assert_eq!(r.class, "breakpoint-created");
        let bkpt = r.get("bkpt").expect("bkpt");
        assert_eq!(bkpt.get_str("number"), Some("1"));
        assert_eq!(bkpt.get_str("file"), Some("main.c"));
        assert_eq!(bkpt.get_str("line"), Some("3"));
    }

    #[test]
    fn parses_stopped_with_frame() {
        let line = "*stopped,reason=\"breakpoint-hit\",disp=\"keep\",bkptno=\"1\",\
                    frame={addr=\"0x0000555555555169\",func=\"main\",file=\"main.c\",line=\"3\"}";
        let r = parse(line).expect("record");
        assert_eq!(r.kind, '*');
        assert_eq!(r.class, "stopped");
        assert_eq!(r.get_str("reason"), Some("breakpoint-hit"));
        let frame = r.get("frame").expect("frame");
        assert_eq!(frame.get_str("func"), Some("main"));
        assert_eq!(frame.get_str("line"), Some("3"));
    }

    #[test]
    fn parses_done_with_stack_and_variables() {
        let line = concat!(
            "^done,stack=[frame={level=\"0\",func=\"main\",file=\"main.c\",line=\"3\"},",
            "frame={level=\"1\",func=\"foo\",file=\"foo.c\",line=\"9\"}],",
            "variables=[{name=\"a\",value=\"1\"},{name=\"b\",value=\"2\"}]"
        );
        let r = parse(line).expect("record");
        assert_eq!(r.kind, '^');
        assert_eq!(r.class, "done");
        let stack = r.get("stack").and_then(Value::as_list).expect("stack");
        assert_eq!(stack.len(), 2);
        // GDB wraps every frame as `frame={...}`.
        let frame0 = stack[0].get("frame").expect("frame wrapper");
        assert_eq!(frame0.get_str("func"), Some("main"));
        let frame1 = stack[1].get("frame").expect("frame wrapper");
        assert_eq!(frame1.get_str("func"), Some("foo"));
        let vars = r.get("variables").and_then(Value::as_list).expect("vars");
        assert_eq!(vars.len(), 2);
        assert_eq!(vars[1].get_str("name"), Some("b"));
    }

    #[test]
    fn parses_running_class() {
        let r = parse("^running").expect("record");
        assert_eq!(r.class, "running");
    }

    #[test]
    fn rejects_non_records() {
        assert!(parse("hello").is_none());
        assert!(parse("Reading symbols...").is_none());
    }

    #[test]
    fn unescapes_quoted_values() {
        let r = parse("=x,msg=\"line1\\nline2\"").expect("record");
        assert_eq!(r.get_str("msg"), Some("line1\nline2"));
    }

    #[test]
    fn parses_empty_list() {
        let r = parse("^done,variables=[]").expect("record");
        assert_eq!(
            r.get("variables")
                .and_then(Value::as_list)
                .map(<[Value]>::len),
            Some(0)
        );
    }
}
