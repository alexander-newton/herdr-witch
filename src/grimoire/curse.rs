use fastrand::Rng;
use unicode_width::UnicodeWidthChar;

const UP: [u32; 50] = [
    0x30d, 0x30e, 0x304, 0x305, 0x33f, 0x311, 0x306, 0x310, 0x352, 0x357, 0x351, 0x307, 0x308,
    0x30a, 0x342, 0x343, 0x344, 0x34a, 0x34b, 0x34c, 0x303, 0x302, 0x30c, 0x350, 0x300, 0x301,
    0x30b, 0x30f, 0x312, 0x313, 0x314, 0x33d, 0x309, 0x363, 0x364, 0x365, 0x366, 0x367, 0x368,
    0x369, 0x36a, 0x36b, 0x36c, 0x36d, 0x36e, 0x36f, 0x33e, 0x35b, 0x346, 0x31a,
];
const DN: [u32; 40] = [
    0x316, 0x317, 0x318, 0x319, 0x31c, 0x31d, 0x31e, 0x31f, 0x320, 0x324, 0x325, 0x326, 0x329,
    0x32a, 0x32b, 0x32c, 0x32d, 0x32e, 0x32f, 0x330, 0x331, 0x332, 0x333, 0x339, 0x33a, 0x33b,
    0x33c, 0x345, 0x347, 0x348, 0x349, 0x34d, 0x34e, 0x353, 0x354, 0x355, 0x356, 0x359, 0x35a,
    0x323,
];
const MID: [u32; 22] = [
    0x315, 0x31b, 0x340, 0x341, 0x358, 0x321, 0x322, 0x327, 0x328, 0x334, 0x335, 0x336, 0x34f,
    0x35c, 0x35d, 0x35e, 0x35f, 0x360, 0x362, 0x338, 0x337, 0x361,
];

fn pick(table: &[u32], rng: &mut Rng) -> char {
    char::from_u32(table[rng.usize(..table.len())]).expect("combining mark")
}

/// Stack combining marks after each ASCII letter; `level` 0 returns the word unchanged.
pub fn curse(word: &str, level: u8, rng: &mut Rng) -> String {
    if level == 0 {
        return word.to_string();
    }
    let lvl = f32::from(level);
    let mut out = String::with_capacity(word.len() * 8);
    for ch in word.chars() {
        out.push(ch);
        if !ch.is_ascii_alphabetic() {
            continue;
        }
        let up = (rng.f32() * (lvl + 0.6)) as usize;
        let down = (rng.f32() * (lvl + 0.3)) as usize;
        let mid = usize::from(rng.f32() < lvl * 0.06);
        out.extend((0..up).map(|_| pick(&UP, rng)));
        out.extend((0..down).map(|_| pick(&DN, rng)));
        out.extend((0..mid).map(|_| pick(&MID, rng)));
    }
    out
}

/// Terminal columns used by `s`: base characters by unicode-width, combining marks zero.
pub fn display_width(s: &str) -> usize {
    s.chars()
        .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
        .map(|c| c.width().unwrap_or(0))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip(s: &str) -> String {
        s.chars()
            .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
            .collect()
    }

    #[test]
    fn curse_preserves_base() {
        let mut rng = Rng::with_seed(1);
        for i in 0..100 {
            let w = format!("Word{i}-of'Power");
            assert_eq!(strip(&curse(&w, 4, &mut rng)), w);
        }
    }

    #[test]
    fn curse_level_density() {
        let mut rng = Rng::with_seed(2);
        let letters = 10_000;
        let marks: usize = (0..letters)
            .map(|_| curse("a", 4, &mut rng).chars().count() - 1)
            .sum();
        let mean = marks as f64 / f64::from(letters);
        assert!((3.0..=5.0).contains(&mean), "mean {mean}");
        assert_eq!(curse("Spirit", 0, &mut rng), "Spirit");
    }

    #[test]
    fn marks_only_after_ascii_letters() {
        let mut rng = Rng::with_seed(3);
        assert_eq!(curse("1 -'.", 4, &mut rng), "1 -'.");
    }

    #[test]
    fn width_ignores_combining() {
        let mut rng = Rng::with_seed(4);
        assert_eq!(display_width(&curse("Spirit", 4, &mut rng)), 6);
        assert_eq!(display_width("abc"), 3);
    }
}
