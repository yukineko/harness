//! A tiny seedable PRNG (xorshift64) + Gaussian sampling, so Thompson sampling
//! is reproducible in tests and dependency-free. Seeded from time + episode
//! count in production; from a fixed seed in tests.

pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        // splitmix64 scramble so sequential seeds (1, 2, 3, …) decorrelate on the
        // very first draw — plain xorshift correlates badly on small seeds.
        let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        Self { state: z.max(1) }
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    /// Uniform in [0, 1).
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// One Normal(mean, sd) draw via Box-Muller.
    pub fn normal(&mut self, mean: f64, sd: f64) -> f64 {
        let u1 = self.next_f64().max(1e-12);
        let u2 = self.next_f64();
        let z = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
        mean + sd * z
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_in_range_and_deterministic() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..1000 {
            let x = a.next_f64();
            assert!((0.0..1.0).contains(&x));
            assert_eq!(x, b.next_f64()); // same seed → same sequence
        }
    }

    #[test]
    fn normal_mean_is_roughly_right() {
        let mut r = Rng::new(7);
        let n = 20000;
        let sum: f64 = (0..n).map(|_| r.normal(0.5, 0.1)).sum();
        let mean = sum / n as f64;
        assert!((mean - 0.5).abs() < 0.01, "empirical mean {mean}");
    }
}
