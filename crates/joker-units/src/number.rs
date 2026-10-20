/// The parts of a decimal literal: `digits ("." digits)?`, with an optional
/// sign and exponent when the grammar allows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Scanned {
    pub(crate) end: usize,
    pub(crate) int_end: usize,
    pub(crate) frac: Option<(usize, usize)>,
}

/// Scans a number at the start of `text` and returns where it ends, or `None`
/// if there is no well-formed number there.
pub(crate) fn scan(text: &str, allow_sign: bool, allow_exponent: bool) -> Option<Scanned> {
    let bytes = text.as_bytes();
    let mut i = 0;
    if allow_sign && bytes.first() == Some(&b'-') {
        i += 1;
    }
    let int_start = i;
    while bytes.get(i).is_some_and(u8::is_ascii_digit) {
        i += 1;
    }
    if i == int_start {
        return None;
    }
    let int_end = i;
    let mut frac = None;
    if bytes.get(i) == Some(&b'.') {
        let frac_start = i + 1;
        let mut j = frac_start;
        while bytes.get(j).is_some_and(u8::is_ascii_digit) {
            j += 1;
        }
        if j == frac_start {
            return None;
        }
        frac = Some((frac_start, j));
        i = j;
    }
    if allow_exponent && matches!(bytes.get(i), Some(b'e' | b'E')) {
        let mut j = i + 1;
        if matches!(bytes.get(j), Some(b'+' | b'-')) {
            j += 1;
        }
        let exp_start = j;
        while bytes.get(j).is_some_and(u8::is_ascii_digit) {
            j += 1;
        }
        if j == exp_start {
            return None;
        }
        i = j;
    }
    Some(Scanned {
        end: i,
        int_end,
        frac,
    })
}
