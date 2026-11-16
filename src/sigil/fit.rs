use super::sprites::WitchSize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    HalfBlock,
    Octant,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fit {
    pub n: usize,
    pub px_w: usize,
    pub px_h: usize,
    pub sigil_xy: (usize, usize),
    pub witch: Option<(WitchSize, (usize, usize))>,
}

pub fn fit(cols: usize, rows: usize, mode: Mode) -> Option<Fit> {
    if cols < 4 || rows < 2 {
        return None;
    }
    let (px_w, px_h) = match mode {
        Mode::HalfBlock => (cols, rows * 2),
        Mode::Octant => (cols * 2, rows * 4),
    };
    let n = px_w;
    let (size, witch_h) = if n >= 34 {
        (WitchSize::Big, 24)
    } else {
        (WitchSize::Mini, 13)
    };
    let fit = |n, sigil_xy, witch| Fit {
        n,
        px_w,
        px_h,
        sigil_xy,
        witch,
    };
    if n + witch_h <= px_h {
        let y = (px_h - (n + witch_h)) / 2;
        Some(fit(n, (0, y), Some((size, (0, y + n)))))
    } else if n <= px_h {
        Some(fit(n, (0, (px_h - n) / 2), None))
    } else {
        Some(fit(px_h, ((px_w - px_h) / 2, 0), None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_halfblock_27x40() {
        let f = fit(27, 40, Mode::HalfBlock).unwrap();
        assert_eq!((f.n, f.px_w, f.px_h), (27, 27, 80));
        assert_eq!(f.sigil_xy, (0, 20));
        assert_eq!(f.witch, Some((WitchSize::Mini, (0, 47))));
    }

    #[test]
    fn fit_octant_27x40() {
        let f = fit(27, 40, Mode::Octant).unwrap();
        assert_eq!(f.n, 54);
        assert!(matches!(f.witch, Some((WitchSize::Big, _))));
        assert_eq!(f.sigil_xy, (0, (160 - (54 + 24)) / 2));
    }

    #[test]
    fn fit_short_pane_sigil_only() {
        let f = fit(27, 16, Mode::HalfBlock).unwrap();
        assert_eq!(f.n, 27);
        assert_eq!(f.witch, None);
        assert_eq!(f.sigil_xy, (0, (32 - 27) / 2));
    }

    #[test]
    fn fit_very_short_shrinks() {
        let f = fit(27, 10, Mode::HalfBlock).unwrap();
        assert_eq!(f.n, 20);
        assert_eq!(f.witch, None);
        assert_eq!(f.sigil_xy, (3, 0));
    }

    #[test]
    fn fit_degenerate_sizes() {
        for m in [Mode::HalfBlock, Mode::Octant] {
            assert!(fit(0, 0, m).is_none());
            assert!(fit(3, 40, m).is_none());
            assert!(fit(27, 1, m).is_none());
        }
    }
}
