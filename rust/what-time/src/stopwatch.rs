//! A wasm-safe stopwatch: the native build measures real elapsed time,
//! while the wasm32-unknown-unknown build (no clock) reports zeros so the
//! timings fields keep their shape in every target.

#[derive(Clone, Copy)]
pub struct Stopwatch {
    #[cfg(not(target_arch = "wasm32"))]
    started: std::time::Instant,
}

impl Stopwatch {
    pub fn start() -> Stopwatch {
        Stopwatch {
            #[cfg(not(target_arch = "wasm32"))]
            started: std::time::Instant::now(),
        }
    }

    /// Milliseconds since [`Stopwatch::start`]; always zero on wasm.
    pub fn elapsed_ms(&self) -> f64 {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.started.elapsed().as_secs_f64() * 1000.0
        }
        #[cfg(target_arch = "wasm32")]
        {
            0.0
        }
    }
}
