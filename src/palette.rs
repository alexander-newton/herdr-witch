pub type Rgb = [f32; 3];

pub const BG: Rgb = [13.0, 6.0, 20.0];
pub const DEEP: Rgb = [42.0, 15.0, 69.0];
pub const HALO: Rgb = [58.0, 20.0, 96.0];
pub const VIOLET: Rgb = [107.0, 47.0, 163.0];
pub const ORCHID: Rgb = [170.0, 90.0, 220.0];
pub const GOLD: Rgb = [245.0, 197.0, 66.0];
pub const BRIGHT: Rgb = [255.0, 232.0, 150.0];
pub const CYAN: Rgb = [79.0, 240.0, 255.0];
pub const WHITE: Rgb = [235.0, 250.0, 255.0];
pub const SLEEVE: Rgb = [59.0, 22.0, 98.0];
pub const SKIN: Rgb = [232.0, 198.0, 168.0];

pub fn lerp_c(a: Rgb, b: Rgb, t: f32) -> Rgb {
    [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t)
}

pub fn mul(c: Rgb, k: f32) -> Rgb {
    c.map(|v| v * k)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lerp_and_mul() {
        assert_eq!(lerp_c([0.0; 3], [10.0, 20.0, 30.0], 0.5), [5.0, 10.0, 15.0]);
        assert_eq!(mul([1.0, 2.0, 3.0], 2.0), [2.0, 4.0, 6.0]);
    }
}
