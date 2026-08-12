//! Differential-test harness for an ARM32 interpreter — the binary side of
//! `armdiff.py`.
//!
//! Copy this into your workspace as a `[[bin]]` of whatever crate owns the
//! interpreter, change the two `use` lines and the four calls marked ADAPT, and
//! build it:
//!
//! ```text
//! cargo build --release --bin armdiff
//! python armdiff.py --helper target/release/armdiff --n 8000 --class all
//! ```
//!
//! It reads cases from stdin, executes exactly one instruction of each, and
//! writes the resulting state to stdout. All little-endian:
//!
//! ```text
//!   in:  u32 count, then per case: u32 inst, 16 x u32 regs, u32 cpsr, MEM bytes
//!   out: per case: 16 x u32 regs, u32 cpsr, MEM bytes
//! ```
//!
//! Keep it this dumb. It is a reference point that must not have opinions: no
//! patches, no HLE, no logging — one instruction, one flat window, in and out.

// ADAPT: your interpreter's types.
use arm32_cpu::{reg, Cpu, Memory, Mode};

use std::io::{Read, Write};

/// The window each case runs in: code at 0, scratch data at `DATA`. Must match
/// `SIZE` / `MEM` / `DATA` in armdiff.py.
const SIZE: usize = 0x2000;
const MEM: usize = 256;
const DATA: u32 = 0x1000;

/// A flat, wrapping address space. Wrapping rather than faulting is deliberate:
/// a generated case that computes a wild address should still produce *some*
/// defined answer on both sides instead of aborting the run.
struct Flat(Vec<u8>);

impl Memory for Flat {
    fn r8(&mut self, a: u32) -> u8 {
        self.0[a as usize % SIZE]
    }
    fn r16(&mut self, a: u32) -> u16 {
        let o = a as usize % SIZE;
        u16::from_le_bytes([self.0[o], self.0[(o + 1) % SIZE]])
    }
    fn r32(&mut self, a: u32) -> u32 {
        let o = a as usize % SIZE;
        u32::from_le_bytes([
            self.0[o],
            self.0[(o + 1) % SIZE],
            self.0[(o + 2) % SIZE],
            self.0[(o + 3) % SIZE],
        ])
    }
    fn w8(&mut self, a: u32, v: u8) {
        self.0[a as usize % SIZE] = v;
    }
    fn w16(&mut self, a: u32, v: u16) {
        let o = a as usize % SIZE;
        let b = v.to_le_bytes();
        self.0[o] = b[0];
        self.0[(o + 1) % SIZE] = b[1];
    }
    fn w32(&mut self, a: u32, v: u32) {
        let o = a as usize % SIZE;
        for (i, x) in v.to_le_bytes().iter().enumerate() {
            self.0[(o + i) % SIZE] = *x;
        }
    }
}

fn main() {
    let mut input = Vec::new();
    std::io::stdin().read_to_end(&mut input).expect("read stdin");
    let count = u32::from_le_bytes(input[0..4].try_into().unwrap()) as usize;
    let case_len = 4 + 16 * 4 + 4 + MEM;
    let mut out = Vec::with_capacity(count * (16 * 4 + 4 + MEM));

    for c in 0..count {
        let base = 4 + c * case_len;
        let rd = |i: usize| u32::from_le_bytes(input[base + i..base + i + 4].try_into().unwrap());
        let inst = rd(0);

        let mut mem = Flat(vec![0u8; SIZE]);
        mem.0[0..4].copy_from_slice(&inst.to_le_bytes());
        mem.0[DATA as usize..DATA as usize + MEM]
            .copy_from_slice(&input[base + 4 + 68..base + 4 + 68 + MEM]);

        // ADAPT: construct your CPU and load the case's register file.
        let mut cpu = Cpu::new();
        for r in 0..16u8 {
            cpu.reg_set(Mode::User, r, rd(4 + r as usize * 4));
        }
        cpu.reg_set(Mode::User, reg::CPSR, rd(4 + 64));
        cpu.reg_set(Mode::User, reg::PC, 0);

        // ADAPT: execute exactly one instruction.
        cpu.step(&mut mem);

        // ADAPT: read the register file back out.
        for r in 0..16u8 {
            out.extend_from_slice(&cpu.reg_get(Mode::User, r).to_le_bytes());
        }
        out.extend_from_slice(&cpu.reg_get(Mode::User, reg::CPSR).to_le_bytes());
        out.extend_from_slice(&mem.0[DATA as usize..DATA as usize + MEM]);
    }
    std::io::stdout().write_all(&out).expect("write stdout");
}
