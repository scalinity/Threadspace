//! Deterministic randomness and time for the harness: no wall clock, no OS
//! entropy. Every run is a pure function of its seed.

/// SplitMix64: small, fast and fully specified by its seed.
#[derive(Debug, Clone)]
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `0..bound` (bound > 0).
    pub fn below(&mut self, bound: usize) -> usize {
        (self.next_u64() % bound as u64) as usize
    }

    pub fn chance(&mut self, numerator: u64, denominator: u64) -> bool {
        self.next_u64() % denominator < numerator
    }
}

/// A virtual clock advanced explicitly; tests never sleep to create time.
#[derive(Debug, Clone)]
pub struct VirtualClock {
    now_ms: i64,
    step_ms: i64,
}

impl VirtualClock {
    pub const EPOCH_MS: i64 = 1_791_000_000_000;

    pub fn new() -> Self {
        Self {
            now_ms: Self::EPOCH_MS,
            step_ms: 10,
        }
    }

    pub fn now_ms(&self) -> i64 {
        self.now_ms
    }

    pub fn tick(&mut self) -> i64 {
        self.now_ms += self.step_ms;
        self.now_ms
    }

    pub fn advance(&mut self, ms: i64) -> i64 {
        self.now_ms += ms;
        self.now_ms
    }
}

impl Default for VirtualClock {
    fn default() -> Self {
        Self::new()
    }
}
