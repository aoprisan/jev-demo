//! A small, explicit PRNG.
//!
//! Hand-rolled rather than pulled from a crate so that a seed pins the output
//! exactly, across platforms and across dependency updates. SplitMix64 is
//! enough for synthetic prices and is short enough to read.

/// A seeded SplitMix64 stream.
#[derive(Debug, Clone)]
pub struct Rng {
    state: u64,
    spare_normal: Option<f64>,
}

impl Rng {
    /// A stream from `seed`.
    pub fn new(seed: u64) -> Self {
        Self { state: seed.wrapping_add(0x9E37_79B9_7F4A_7C15), spare_normal: None }
    }

    /// A stream derived from `seed` and a named purpose.
    ///
    /// Each concern draws from its own stream, so adding a headline does not
    /// shift the prices generated after it.
    pub fn stream(seed: u64, purpose: &str) -> Self {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in purpose.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        Rng::new(seed ^ h)
    }

    /// The next raw value.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `[0, 1)`.
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform in `[lo, hi)`.
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + self.unit() * (hi - lo)
    }

    /// True with probability `p`.
    pub fn chance(&mut self, p: f64) -> bool {
        self.unit() < p
    }

    /// An integer in `[lo, hi)`.
    pub fn int(&mut self, lo: i64, hi: i64) -> i64 {
        if hi <= lo {
            return lo;
        }
        lo + (self.next_u64() % (hi - lo) as u64) as i64
    }

    /// Standard normal, by Box-Muller. The second deviate of each pair is kept.
    pub fn normal(&mut self) -> f64 {
        if let Some(spare) = self.spare_normal.take() {
            return spare;
        }
        let u1 = self.unit().max(f64::MIN_POSITIVE);
        let u2 = self.unit();
        let r = (-2.0 * u1.ln()).sqrt();
        let theta = std::f64::consts::TAU * u2;
        self.spare_normal = Some(r * theta.sin());
        r * theta.cos()
    }

    /// One of `items`, uniformly.
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[(self.next_u64() % items.len() as u64) as usize]
    }

    /// An index into a weighted set.
    pub fn weighted(&mut self, weights: &[f64]) -> usize {
        let total: f64 = weights.iter().sum();
        let mut draw = self.unit() * total;
        for (i, w) in weights.iter().enumerate() {
            draw -= w;
            if draw <= 0.0 {
                return i;
            }
        }
        weights.len() - 1
    }
}
