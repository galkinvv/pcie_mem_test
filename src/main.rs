use std::cmp;
use std::env;
use std::error::Error;
use std::fs;
use std::fs::File;
use std::io::{stdout, Write};
use std::time::Instant;

type MemSingleUnit = u32;
const UNIT_COUNT: usize = 8;
const SHOW_RANGE_PRE: usize = 65;
const SHOW_RANGE_POST: usize = if cfg!(test) { 1 } else { 17 } * 1024 * 4;

#[derive(PartialEq, Default, Clone, Copy)]
struct MemUnit([MemSingleUnit; UNIT_COUNT]);

type ReadResultSlice = [MemUnit; SHOW_RANGE_POST];

#[repr(C, align(0x1000))]
struct Context {
    check: ReadResultSlice,
    reread_cache: ReadResultSlice,
    reread_memory: ReadResultSlice,
    reread_memory2: ReadResultSlice,
    iteration: u32,
    first_error_addr: usize,
}

const UNIT_ARRAY_BYTES: usize = std::mem::size_of::<MemUnit>();

impl std::fmt::Display for MemUnit {
    fn fmt(self: &MemUnit, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        for (i, x) in self.0.iter().enumerate() {
            if i > 0 && i % 4 == 0 {
                write!(f, "  ").expect("MemUnit spacing1");
                if i > 0 && i % 16 == 0 {
                    write!(f, "   ").expect("MemUnit spacing1");
                }
            }
            write!(f, "{:08x} ", x).expect("MemUnit Formatter");
        }
        Ok(())
    }
}

fn get_rotated_left_7_hex_digits(x: u32, shift_digits: u32) -> u32 {
    let hex_digit_bits = 4;
    let shift_bits = shift_digits * hex_digit_bits;
    let upper_digits_mask = 0xFFFFFFFFu32 << (32 - shift_bits - hex_digit_bits); //mask for shift_digits digits
    let x_with_bits_pre_shift =
        ((x & upper_digits_mask) << hex_digit_bits) | (x & !upper_digits_mask);
    x_with_bits_pre_shift.rotate_left(shift_bits)
}

fn index_to_single_value(phys_addr: usize) -> MemSingleUnit {
    let shift: u32 = ((1 + phys_addr) as u32) % 13 + 2;
    let rotated = get_rotated_left_7_hex_digits(phys_addr as u32, shift % 7);
    //add shift value as first hex deigit
    (rotated | (shift << (7 * 4))) as MemSingleUnit
}

fn index_to_value(i: usize) -> MemUnit {
    let mut result: MemUnit = Default::default();
    for x in 0..UNIT_COUNT {
        result.0[x] =
            index_to_single_value(i * UNIT_ARRAY_BYTES + x * std::mem::size_of::<MemSingleUnit>());
    }
    result
}

impl Context {
    pub const fn new() -> Self {
        Self {
            check: [MemUnit([0; UNIT_COUNT]); SHOW_RANGE_POST],
            reread_cache: [MemUnit([0; UNIT_COUNT]); SHOW_RANGE_POST],
            reread_memory: [MemUnit([0; UNIT_COUNT]); SHOW_RANGE_POST],
            reread_memory2: [MemUnit([0; UNIT_COUNT]); SHOW_RANGE_POST],
            iteration: 0,
            first_error_addr: 0,
        }
    }

    fn print_address_expected(&self, addr: usize, note: &str) {
        let note_pad = if note.is_empty() { "" } else { "  " };
        println!(
            "{:#011x}: {}{note_pad}{note}",
            addr * UNIT_ARRAY_BYTES,
            index_to_value(addr),
        );
    }

    fn print_cache_reread(&self, addr_offset: usize) {
        let addr = addr_offset + self.first_error_addr;
        let check = self.check[addr_offset];
        let reread_cache = self.reread_cache[addr_offset];
        let expected = index_to_value(addr);
        let note = match (expected == reread_cache, check == reread_cache) {
            (false, false) => "UNSTABLE CACHE",
            (true, false) => "CACHE ACCESS ERROR",
            (false, true) => "CACHED ERROR",
            (true, true) => {
                return;
            }
        };
        self.print_address_expected(addr, "");
        println!(" It{}  MEMBAR {check}FAIL", self.iteration);
        println!(" It{}  CACHED {reread_cache}{note}", self.iteration,);
    }

    fn print_memory_reread(&self, addr_offset: usize) {
        let addr = addr_offset + self.first_error_addr;
        let check = self.check[addr_offset];
        let reread_cache = self.reread_cache[addr_offset];
        let reread_memory = self.reread_memory[addr_offset];
        let reread_memory2 = self.reread_memory2[addr_offset];
        let expected = index_to_value(addr);
        // don't compare check VS reread memory and reread_cache VS reread_memory2,
        // On error-counting scenarious the may be pseudorandoly same
        let note = match (
            expected == reread_cache,
            check == reread_cache,
            reread_memory == reread_cache,
        ) {
            (false, false, false) => {
                if reread_memory == reread_memory2 {
                    "TEMPORAL UNSTABLE CACHE"
                } else {
                    "PERMANENT UNSTABLE CACHE"
                }
            }
            (false, false, true) => {
                if reread_memory == reread_memory2 {
                    "SINGLE UNSTABLE CACHE"
                } else {
                    "RANDOM UNSTABLE CACHE"
                }
            }
            (true, false, false) => {
                if reread_memory == check {
                    "REPRODUCABLE CACHE ACCESS ERROR"
                } else {
                    "RANDOM CACHE ACCESS ERROR"
                }
            }
            (true, false, true) => "SINGLE CACHE ACCESS ERROR",
            (false, true, true) => "STORED AND CACHED ERROR",
            (false, true, false) => {
                if reread_memory == reread_memory2 {
                    "NON-STORED BUT CACHED ERROR"
                } else {
                    "SOMETIMES CACHED ERROR"
                }
            }
            (true, true, false) => {
                self.print_address_expected(addr, "");
                println!(" It{}  MEMBAR {check}", self.iteration);
                println!(" It{}  CACHED {reread_cache}", self.iteration);
                "NEW FAIL"
            }
            (true, true, true) => {
                self.print_address_expected(addr, "OK");
                return;
            }
        };
        println!(" It{}  UNCACH {reread_memory}{note}", self.iteration,);
        println!(" It{}  REREAD {reread_memory2}", self.iteration,);
    }

    fn test_file(&mut self, file_to_test: &File) -> Result<bool, Box<dyn Error>> {
        let mmaped_ro =
            unsafe { memmap2::Mmap::map(file_to_test) }.expect("mmap_ro_prepare_failed");
        let mut mmaped =
            unsafe { memmap2::MmapMut::map_mut(file_to_test) }.expect("mmap_mut_prepare_failed");
        let len_in_units =
            file_to_test.metadata().expect("metadata").len() as usize / UNIT_ARRAY_BYTES;
        let mem_ro_as_units_slices = unsafe {
            std::slice::from_raw_parts(mmaped_ro.as_ptr() as *const MemUnit, len_in_units)
        };
        let mem_as_units_slices = unsafe {
            std::slice::from_raw_parts_mut(mmaped.as_mut_ptr() as *mut MemUnit, len_in_units)
        };
        for (addr, value) in mem_as_units_slices.iter_mut().enumerate() {
            *value = index_to_value(addr);
        }
        let max_iteration = 3;
        let show_range_post = cmp::min(SHOW_RANGE_POST, len_in_units / 2); //limit show_range_post to use other half for cache cleaning effects
        for iteration in 0..max_iteration {
            self.iteration = iteration;
            let time_start = Instant::now();
            for (first_error_addr, value) in mem_as_units_slices.iter().enumerate() {
                self.check[0] = *value;
                if self.check[0] != index_to_value(first_error_addr) {
                    let checkreader = |storage: &mut Self, addr_offset: usize| {
                        storage.check[addr_offset] =
                            mem_as_units_slices[addr_offset + first_error_addr]
                    };
                    let rereader = |storage: &mut ReadResultSlice, addr_offset: usize| {
                        storage[addr_offset] =
                            mem_ro_as_units_slices[addr_offset + first_error_addr];
                        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst)
                    };
                    rereader(&mut self.reread_cache, 0);
                    self.first_error_addr = first_error_addr;
                    println!(
                        "FAIL: error found at iteration {iteration} address {:#011x}",
                        self.first_error_addr * UNIT_ARRAY_BYTES
                    );
                    for ok_addr in (cmp::max(SHOW_RANGE_PRE, first_error_addr) - SHOW_RANGE_PRE)
                        ..first_error_addr
                    {
                        self.print_address_expected(ok_addr, "OK");
                    }
                    self.print_cache_reread(0);
                    stdout().lock().flush()?;
                    for bad_addr in
                        1..cmp::min(show_range_post, len_in_units - self.first_error_addr)
                    {
                        checkreader(self, bad_addr);
                        rereader(&mut self.reread_cache, bad_addr);
                    }

                    // Make cache clean by reading other half of units
                    let mut ok_readings = 0;
                    let reading_estimates_count = len_in_units / 2;

                    for other_addr in 0..reading_estimates_count {
                        let effective_addr = (other_addr + len_in_units / 2) % len_in_units;
                        let expected = index_to_value(effective_addr);
                        let actual = mem_ro_as_units_slices[effective_addr];
                        if actual == expected {
                            ok_readings += 1;
                        }
                    }

                    rereader(&mut self.reread_memory, 0);
                    rereader(&mut self.reread_memory2, 0);
                    self.print_memory_reread(0);
                    let error_percent = (reading_estimates_count - ok_readings) as f64 * 100.0
                        / reading_estimates_count as f64;
                    println!("Error percent estimation: {error_percent:.9}% out of tested");
                    for bad_addr in
                        1..cmp::min(show_range_post, len_in_units - self.first_error_addr)
                    {
                        rereader(&mut self.reread_memory, bad_addr);
                        rereader(&mut self.reread_memory2, bad_addr);
                        self.print_cache_reread(bad_addr);
                        self.print_memory_reread(bad_addr);
                    }
                    return Ok(false);
                }
            }
            if iteration == 0 {
                let elapsed = time_start.elapsed();
                print!(
                    "First pass done without miscompares in {} milliseconds, ",
                    elapsed.as_millis()
                );
                println!(
                    "all {max_iteration} iterations are expected to be done in {} seconds... ",
                    (elapsed * max_iteration).as_secs()
                );
                stdout().lock().flush()?;
            }
        }
        println!("PASS: iterations {max_iteration}");
        Ok(true)
    }
}
static mut GLOBAL_MUT_CONTEXT: Context = Context::new();
fn main() -> Result<(), Box<dyn Error>> {
    print!("usage: boot with console=null and run as root passing path with a pcie memory bar mapping as a single argument. By https://github.com/galkinvv/pcie_mem_test  ");
    println!(
        "Typical example: {} /sys/bus/pci/devices/0000:01:00.0/resource1",
        env::args_os()
            .next()
            .unwrap_or("./pcie_mem_test".into())
            .to_string_lossy()
    );
    if env::args_os().len() != 2 {
        return Err("Expected exactly one command line agument".into());
    }
    let file_name = env::args_os()
        .nth(1)
        .expect("Single file argument is expected");
    println!(
        "Testing {0:?}, size {1}=0x{1:X} ... ",
        file_name,
        fs::metadata(&file_name)?.len()
    );
    stdout().lock().flush()?;
    let file = File::options()
        .read(true)
        .write(true)
        .open(file_name)
        .expect("Failed opening file for Read+Write");
    unsafe {
        GLOBAL_MUT_CONTEXT.test_file(&file)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time;
    use tempfile;

    #[test]
    fn test_index_to_value() {
        assert_eq!(index_to_single_value(0x00000000), 0x30000000);
        assert_eq!(index_to_single_value(0x00000001), 0x40010000);
        assert_eq!(index_to_single_value(0x0000000F), 0x50F00000);
        assert_eq!(index_to_single_value(0x000001F5), 0xA01F5000);
        assert_eq!(index_to_single_value(0x000000F0), 0x9000F000);
        assert_eq!(index_to_single_value(0x00000100), 0xC0000001);
        assert_eq!(index_to_single_value(0x00000F00), 0x8000F000);
        assert_eq!(index_to_single_value(0x00001000), 0x40000001);
        assert_eq!(index_to_single_value(0x00010000), 0x60001000);
        assert_eq!(index_to_single_value(0x00100000), 0xC0001000);
        assert_eq!(index_to_single_value(0x01000000), 0x40001000);
        assert_eq!(index_to_single_value(0x0E000006), 0xA0006E00);
    }

    #[test]
    fn test_on_modifying_file() -> Result<(), Box<dyn Error>> {
        let modifying = tempfile::tempfile().expect("failed creating temp file");
        let len: usize = 256 * 1024 * 1024;
        modifying.set_len(len as u64).expect("set_len");
        let mut mmap_concurrent =
            unsafe { memmap2::MmapMut::map_mut(&modifying) }.expect("mmap_concurrent_failed");
        thread::spawn(move || {
            thread::sleep(time::Duration::from_millis(
                25000 / std::mem::size_of::<MemUnit>() as u64,
            ));
            mmap_concurrent[len / 100] = 0x42;
        });
        assert!(!unsafe { GLOBAL_MUT_CONTEXT.test_file(&modifying) }?);
        Ok(())
    }

    #[test]
    fn test_on_static_file() -> Result<(), Box<dyn Error>> {
        let static_file = tempfile::tempfile().expect("failed creating temp file");
        let len: usize = 256 * 1024 * 1024;
        static_file.set_len(len as u64).expect("set_len");
        assert!(Context::new().test_file(&static_file)?);
        Ok(())
    }
}
