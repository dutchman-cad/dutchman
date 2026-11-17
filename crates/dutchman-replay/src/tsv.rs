//! Bounded UTF-8 data for fixed script dispatchers.
/// One typed TSV argument.
#[non_exhaustive]
pub enum Arg<'a> {
    /// Finite floating-point number.
    Num(f64),
    /// Signed integer.
    Int(i64),
    /// Reversibly encoded UTF-8 text.
    Text(&'a str),
    /// Boolean flag encoded as zero or one.
    Flag(bool),
    /// Absent argument encoded as a dash.
    None,
}
/// Bounded UTF-8 step records with CRLF terminators.
#[non_exhaustive]
pub struct Tsv {
    text: String,
    lines: usize,
}
const LIMIT: &str = "the rebuild steps exceed the script limit";
impl Tsv {
    /// Start a step stream with the target header.
    pub fn new(target: &str) -> Self {
        Self {
            text: format!("dutchman-steps/1\t{}\r\n", encode(target)),
            lines: 0,
        }
    }
    /// Append one numbered operation; failed calls append nothing.
    pub fn push(&mut self, op: &'static str, args: &[Arg<'_>]) -> Result<(), String> {
        if op.is_empty()
            || !op
                .bytes()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_')
        {
            return Err("the step operation needs an uppercase identifier".into());
        }
        if self.lines >= 200_000 {
            return Err(LIMIT.into());
        }
        let mut line = format!("{}\t{op}", self.lines + 1);
        for arg in args {
            line.push('\t');
            match arg {
                Arg::Num(n) => {
                    if !n.is_finite() {
                        return Err("a value is not a finite number".into());
                    }
                    line.push_str(&if *n == 0.0 { "0".into() } else { n.to_string() });
                }
                Arg::Int(n) => line.push_str(&n.to_string()),
                Arg::Text(t) => {
                    for byte in t.bytes() {
                        append_byte(&mut line, byte);
                        if line.len() > 8_000 {
                            return Err(LIMIT.into());
                        }
                    }
                }
                Arg::Flag(v) => line.push(if *v { '1' } else { '0' }),
                Arg::None => line.push('-'),
            }
            if line.len() > 8_000 {
                return Err(LIMIT.into());
            }
        }
        if line.len() > 8_000 {
            return Err(LIMIT.into());
        }
        self.text.push_str(&line);
        self.text.push_str("\r\n");
        self.lines += 1;
        Ok(())
    }
    /// Borrow the encoded records.
    pub fn as_str(&self) -> &str {
        &self.text
    }
    /// Return the encoded records.
    pub fn finish(self) -> String {
        self.text
    }
}
/// Percent-encode UTF-8 text bytes reversibly.
pub fn encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        append_byte(&mut out, byte);
    }
    out
}
fn append_byte(out: &mut String, byte: u8) {
    if !(0x20..=0x7e).contains(&byte) || matches!(byte, b'%' | b'=' | b'+' | b'-' | b'@') {
        use std::fmt::Write;
        let _ = write!(out, "%{byte:02X}");
    } else {
        out.push(char::from(byte));
    }
}
