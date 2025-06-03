use std::arch::asm;
use std::sync::atomic::{AtomicU64, Ordering};

macro_rules! entries {
    (
        pub enum $name:ident {
            $($entry:ident $(= $value:expr)?),*
            $(,)?
        }
    ) => {
        #[derive(Clone, Copy)]
        pub enum $name {
            $($entry $(= $value)?),*
        }
        impl $name {
            const ALL: &[$name] = &[$($name::$entry),*];
            const COUNT: usize = $name::ALL.len();
            pub fn to_str(&self) -> &'static str {
                match self {
                    $(
                        $name::$entry => stringify!($entry)
                    ),*
                }
            }
        }

    }
}

entries!{
    pub enum Entries {
        Reset,
        ValueCount,
        DescendTo,
        DescendToExisting,
        DescendToValue,
        DescendToByte,
        DescendIndexedBranch,
        DescendFirstByte,
        DescendUntil,
        MoveToPath,
        AscendByte,
        Ascend,
        ToNextSiblingByte,
        ToPrevSiblingByte,
        ToNextStep,
        AscendUntil,
        AscendUntilBranch,
        ToNextVal,
        DescendFirstKPath,
        ToNextKPath,
        ToNextGetValue,
        ForkReadZipper,
    }
}

pub struct Counter {
    pub count: AtomicU64,
    pub cycles: AtomicU64,
}

impl Counter {
    const fn new() -> Self {
        Self {
            count: AtomicU64::new(0),
            cycles: AtomicU64::new(0),
        }
    }
}

pub static COUNTERS: [Counter; Entries::COUNT] =
    [const { Counter::new() }; Entries::COUNT];

pub fn reset_counters() {
    for counter in &COUNTERS {
        counter.count.store(0, Ordering::Relaxed);
        counter.cycles.store(0, Ordering::Relaxed);
    }
}

pub fn print_counters() {
    println!("{:>20},Count,TSCDelta,TSCAverage", "Name");
    for &entry in Entries::ALL {
        let counter = &COUNTERS[entry as usize];
        let count = counter.count.load(Ordering::Relaxed);
        let cycles = counter.cycles.load(Ordering::Relaxed);
        if count == 0 && cycles == 0 { continue; }
        let average = cycles as f64 / count as f64;
        println!("{:>20},{},{},{}", entry.to_str(), count, cycles, average);
    }
}

#[cfg(target_arch = "aarch64")]
#[inline]
fn read_cycle_counter() -> u64 {
    let val: u64;
    unsafe {
        asm!(
            "mrs {}, CNTVCT_EL0",
            out(reg) val,
            options(nostack, nomem)
        );
    }
    val
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[inline]
fn read_cycle_counter() -> u64 {
    let lo: u32;
    let hi: u32;
    unsafe {
        asm!(
            "rdtsc",
            out("eax") lo,
            out("edx") hi,
            options(nostack, nomem)
        );
    }
    ((hi as u64) << 32) | (lo as u64)
}

pub struct TimedSpanGuard<'a> {
    start: u64,
    counter: &'a AtomicU64,
}

impl<'a> TimedSpanGuard<'a> {
    pub fn new(counter: &'a AtomicU64) -> Self {
        Self {
            start: read_cycle_counter(),
            counter,
        }
    }
}

impl<'a> Drop for TimedSpanGuard<'a> {
    fn drop(&mut self) {
        let end = read_cycle_counter();
        self.counter.fetch_add(end - self.start, Ordering::Relaxed);
    }
}

pub const ENABLED: bool = true;

macro_rules! timed_span {
    ($name:ident, $dst:path) => {
        let _guard = if $crate::timed_span::ENABLED {
            let index = $crate::timed_span::Entries::$name as usize;
            let counter = &$dst[index];
            counter.count.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            Some($crate::timed_span::TimedSpanGuard::new(&counter.cycles))
        } else {
            None
        };
    };
    ($name:ident) => {
        $crate::timed_span::timed_span!($name, $crate::timed_span::COUNTERS)
    };
}

pub(crate) use timed_span;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timed_span() {
        reset_counters();
        {
            timed_span!(Reset);
            for ii in 0..100_000 {
                core::hint::black_box(ii);
            }
        }
        print_counters();
    }
}
